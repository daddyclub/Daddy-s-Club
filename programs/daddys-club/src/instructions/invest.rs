//! Позиція інвестора: відкриття обліку (`FR-038`) і підписка
//! (`FR-008`…`FR-010`, `FR-013`).
//!
//! `HolderCheckpoint` — це не запис «про всяк випадок», а перепустка. Список
//! акаунтів гука, покладений у мінт при створенні випуску, резолвить чекпоінти
//! обох сторін переказу; акаунта немає — `execute` впирається в порожній
//! акаунт, і Token-2022 відхиляє передачу цілком, замість того щоб пропустити
//! її повз облік. Тому `open_position` мусить створювати рівно той PDA, який
//! гук шукає, і жодного іншого: адреса зафіксована в `hook_account_metas` і
//! переписати її нічим.
//!
//! Відкриття дозвільне — платник і власник розділені, і власник не підписує.
//! Це не послаблення: усі поля пише програма, тому відкритий кимось стороннім
//! облік нічим не відрізняється від відкритого самим власником, а от вторинці
//! (`FR-024`) розділення дає відкрити облік покупця в тій самій транзакції, що
//! й купівля.
//!
//! Чого в `open_position` навмисно немає — токен-акаунта під бонд. Облік і
//! рахунок це різні речі: рахунок заводить Token-2022 звичайною ATA, а `FR-038`
//! говорить саме про облік.
//!
//! Підписка — друга половина файлу. Перепідписки не існує за побудовою
//! (`FR-009`), тому бонд друкується в тій самій транзакції, що й внесок, а
//! етапу розподілу немає взагалі: скільки прийнято, стільки й надруковано
//! (`FR-013`). Гроші лежать у сховищі підписки і до емітента не доходять доти,
//! доки випуск не зібрано повністю (`FR-008`, `FR-012`) — видачу пише
//! `withdraw_proceeds`.
//!
//! Третя частина — повернення (`FR-011`). Це дзеркало підписки: бонд палиться,
//! внесок іде назад із того самого сховища, origination fee не утримується.
//! Разом із поверненням тут живе й **лінивий** перехід випуску в `Failed`:
//! окремої інструкції «закрити підписку» немає, бо стан на ланцюгу однаково
//! лишається старим, доки хтось не надішле транзакцію.
//!
//! Четверта — виплата (`FR-015`, `FR-016`). Той самий чекпоінт, який відкрив
//! `open_position`, тут нарешті працює: претензія — це різниця індексів,
//! помножена на баланс, і сама виплата нічого не рахує наперед. Ескроу
//! погашення з'являється в цьому файлі вперше й лише тут — підписка й
//! повернення його не бачать, і це навмисно.

use {
    crate::{
        errors::ClubError,
        math,
        state::{HolderCheckpoint, Issue, IssueState, HOLDER_SEED, ISSUE_SEED},
    },
    anchor_lang::prelude::*,
    anchor_spl::{
        token_2022::{
            burn_checked, mint_to, transfer_checked, BurnChecked, MintTo, Token2022,
            TransferChecked,
        },
        token_interface::{Mint, TokenAccount},
    },
};

#[derive(Accounts)]
pub struct OpenPosition<'info> {
    /// Seeds випуску містять `seq` і джерело, яких у цьому наборі немає, — але
    /// перевіряти їх тут і не треба: `Account<Issue>` уже вимагає власника-нашу
    /// програму й дискримінатор `Issue`, а такий акаунт з'являється лише з
    /// `create_issue`. Підробити його поза програмою нічим.
    pub issue: Account<'info, Issue>,

    /// Ті самі seeds, що їх резолвить список гука: випуск і власник. Збіг
    /// доводить `the_hook_resolves_to_the_ledger_this_instruction_creates`.
    #[account(
        init,
        payer = payer,
        space = 8 + HolderCheckpoint::INIT_SPACE,
        seeds = [HOLDER_SEED, issue.key().as_ref(), owner.key().as_ref()],
        bump,
    )]
    pub holder: Account<'info, HolderCheckpoint>,

    #[account(mut)]
    pub payer: Signer<'info>,

    /// CHECK: власник обліку. Підпису не вимагає — відкриття дозвільне
    /// (`FR-038`), і чужий облік однаково веде програма, а не той, хто його
    /// оплатив. Обмежувати власника системним акаунтом теж не можна: бонд
    /// цілком може лежати на PDA чужої програми.
    pub owner: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
}

pub fn open_position(ctx: Context<OpenPosition>) -> Result<()> {
    let issue = ctx.accounts.issue.key();
    let owner = ctx.accounts.owner.key();
    // Свіжий облік починається з сьогоднішнього індексу, а не з нуля: різницю
    // рахує `FR-016`, і чекпоінт у нулі віддав би новому власникові всі
    // виплати, що накопичились до того, як він тут з'явився.
    let index_at_checkpoint = ctx.accounts.issue.payout_index;
    let bump = ctx.bumps.holder;

    let holder = &mut ctx.accounts.holder;
    holder.issue = issue;
    holder.owner = owner;
    holder.index_at_checkpoint = index_at_checkpoint;
    holder.accrued = 0;
    holder.claimed_total = 0;
    holder.bump = bump;

    Ok(())
}

#[derive(Accounts)]
pub struct Subscribe<'info> {
    /// `has_one` прибиває і мінт, і сховище до самого випуску: підставити чуже
    /// сховище або чужий мінт неможливо, а перевіряти це в тілі не треба.
    #[account(mut, has_one = bond_mint, has_one = subscription_vault)]
    pub issue: Account<'info, Issue>,

    /// `FR-038`: облік мусить бути відкритий **до** того, як з'являться
    /// бонд-токени. Інвестор із бондом і без обліку не зміг би ані забрати
    /// виплату, ані передати бонд далі — гук не знайшов би його чекпоінта.
    ///
    /// Seeds містять і випуск, і власника, тому чужий облік у цей набір не
    /// сходиться, а неоткритий — не існує.
    #[account(
        seeds = [HOLDER_SEED, issue.key().as_ref(), investor.key().as_ref()],
        bump = holder.bump,
    )]
    pub holder: Account<'info, HolderCheckpoint>,

    pub investor: Signer<'info>,

    #[account(mut, token::mint = usdc_mint, token::authority = investor)]
    pub investor_usdc: InterfaceAccount<'info, TokenAccount>,

    /// `FR-008`: внески лежать тут і емітенту не доступні. Валюта звіряється з
    /// мінтом, яким рахується переказ.
    #[account(mut, token::mint = usdc_mint)]
    pub subscription_vault: InterfaceAccount<'info, TokenAccount>,

    /// `FR-013`: пропозиція росте лише тут і рівно на суму внесків. Authority
    /// мінта — PDA випуску, тому друкує тільки програма.
    #[account(mut)]
    pub bond_mint: InterfaceAccount<'info, Mint>,

    #[account(mut, token::mint = bond_mint, token::authority = investor)]
    pub investor_bond: InterfaceAccount<'info, TokenAccount>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    pub token_program: Program<'info, Token2022>,
}

/// Внесок у випуск (`FR-008`, `FR-009`, `FR-010`, `FR-013`).
///
/// `amount` — скільки інвестор **пропонує**, а не скільки з нього спишуть.
/// Мінімальний лот міряється саме пропозицією, а приймається `min(amount,
/// залишок)`: інакше хвіст номіналу, менший за лот, не добрав би ніхто й
/// ніколи, і випуск не зміг би дійти до рівності `raised == face`, якої вимагає
/// `FR-010`.
pub fn subscribe(ctx: Context<Subscribe>, amount: u64) -> Result<()> {
    let issue = &ctx.accounts.issue;

    // `FR-008`: підписка живе рівно у своєму стані й у своєму вікні. Вікно
    // відкрите **до** `subscription_end_ts`, не включно: у цю саму секунду вже
    // можна вимагати повернення (`FR-011`), і дві протилежні дії не мають
    // ділити одну мить.
    require!(
        issue.state == IssueState::Subscribing,
        ClubError::IssueNotSubscribing
    );
    require!(
        Clock::get()?.unix_timestamp < issue.subscription_end_ts,
        ClubError::SubscriptionWindowClosed
    );

    let face = u128::from(issue.face);
    let raised = u128::from(issue.raised);
    let remaining = face.checked_sub(raised).ok_or(ClubError::MathOverflow)?;

    // Другий замок на `FR-009`: надрукувати бонд понад номінал не можна навіть
    // тоді, коли стан випуску розійшовся зі зібраним. Через саму інструкцію в
    // такий стан не потрапити — повний збір одразу переводить випуск у `Funded`.
    require!(remaining > 0, ClubError::IssueFullySubscribed);
    require!(amount >= issue.min_lot, ClubError::BelowMinimumLot);

    // `FR-009`: частковий прийом. Решта не списується — вона просто лишається в
    // інвестора, і жодного повернення надлишку не існує.
    let accepted = u128::from(amount).min(remaining);
    let accepted = u64::try_from(accepted).map_err(|_| error!(ClubError::MathOverflow))?;

    let source = issue.source;
    let seq = issue.seq.to_le_bytes();
    let issue_signer: &[&[&[u8]]] = &[&[ISSUE_SEED, source.as_ref(), &seq, &[issue.bump]]];

    // `FR-008`: кошти йдуть у сховище випуску, а не емітенту.
    transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.investor_usdc.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: ctx.accounts.subscription_vault.to_account_info(),
                authority: ctx.accounts.investor.to_account_info(),
            },
        ),
        accepted,
        ctx.accounts.usdc_mint.decimals,
    )?;

    // `FR-009`: бонд видається в тій самій транзакції, що й внесок. Друк гука
    // не кличе — Token-2022 запускає його лише на переказі, тому чекпоінт
    // інвестора тут не зрушується. Це безпечно, поки випуск у `Subscribing`:
    // індекс виплати рухає лише перехоплення, а воно працює в `Repaying`.
    mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            MintTo {
                mint: ctx.accounts.bond_mint.to_account_info(),
                to: ctx.accounts.investor_bond.to_account_info(),
                authority: ctx.accounts.issue.to_account_info(),
            },
            issue_signer,
        ),
        accepted,
    )?;

    let raised = raised
        .checked_add(u128::from(accepted))
        .ok_or(ClubError::MathOverflow)?;
    let raised = u64::try_from(raised).map_err(|_| error!(ClubError::MathOverflow))?;

    let issue = &mut ctx.accounts.issue;
    issue.raised = raised;
    // `FR-010`: успішним випуск стає рівно на повному номіналі. Часткове
    // фінансування не допускається, тому проміжного «майже зібрано» немає.
    if issue.raised == issue.face {
        issue.state = IssueState::Funded;
    }

    Ok(())
}

#[derive(Accounts)]
pub struct Refund<'info> {
    /// Той самий `has_one` на обидва боки обміну, що й у `subscribe`: мінт, з
    /// якого палиться бонд, і сховище, з якого повертаються гроші, прибиті до
    /// самого випуску.
    ///
    /// Ескроу погашення в цей набір не входить узагалі. У випуску два сховища
    /// на одній валюті й одній authority, і `has_one` рятує лише того, хто
    /// назвав правильне поле, — тому поле назване, а зайве сховище просто
    /// відсутнє. Для недозібраного випуску воно до того ж і порожнє: `FR-011`
    /// каже, що зобов'язання не виникає, а перехоплення не вмикається.
    #[account(mut, has_one = bond_mint, has_one = subscription_vault)]
    pub issue: Account<'info, Issue>,

    pub investor: Signer<'info>,

    /// `FR-011`: внесок повертається тому, хто його зробив. Підпис дає право
    /// забрати свої гроші, а не право відправити їх кому завгодно.
    #[account(mut, token::mint = usdc_mint, token::authority = investor)]
    pub investor_usdc: InterfaceAccount<'info, TokenAccount>,

    /// `FR-008`: гроші весь цей час лежали тут і емітенту не діставались. До
    /// сховища погашення вони не доходили ніколи — туди їх кладе лише
    /// перехоплення, яке в недозібраного випуску не вмикається.
    #[account(mut, token::mint = usdc_mint)]
    pub subscription_vault: InterfaceAccount<'info, TokenAccount>,

    /// Пропозиція меншає рівно на спалене (`FR-013`): бонд, за яким гроші вже
    /// повернуто, не має лишатись на ланцюгу — інакше він торгувався б як
    /// вимога до випуску, якої більше немає.
    #[account(mut)]
    pub bond_mint: InterfaceAccount<'info, Mint>,

    #[account(mut, token::mint = bond_mint, token::authority = investor)]
    pub investor_bond: InterfaceAccount<'info, TokenAccount>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    pub token_program: Program<'info, Token2022>,
}

/// Повернення внеску з недозібраного випуску (`FR-011`).
///
/// Інструкція несе дві речі: сам обмін бондів на гроші й **лінивий** перехід
/// випуску в `Failed`. Окремої інструкції «закрити підписку» немає навмисно:
/// стан на ланцюгу однаково лишається старим, доки хтось не надішле
/// транзакцію, тому дозвільне закриття не дало б нічого, чого не дає перше ж
/// повернення. Перший виклик пише `Failed`, наступні застають його вже
/// написаним і працюють так само.
///
/// **Наслідок для читачів (`FR-037`, `FR-031`):** доки ніхто не забрав грошей,
/// недозібраний випуск на ланцюгу ще каже `Subscribing`. «Недозібрано» — це
/// `state == Subscribing && now >= subscription_end_ts && raised < face`, і
/// показувати це треба явно, а не нулями.
///
/// Скільки повертати, каже баланс бонду: одиниця токена дорівнює одиниці
/// номіналу (`FR-013`), а друкувався бонд рівно на прийняте (`FR-009`), тому
/// «рівно свій внесок» із `FR-011` — це і є те, що лежить на рахунку. Origination
/// fee не утримується: її бере `withdraw_proceeds`, якого тут не було й не буде.
///
/// Облік власника (`FR-038`) у наборі відсутній і не зрушується: `payout_index`
/// у випуску, який не дійшов до погашення, стоїть на нулі — рухає його лише
/// перехоплення, а воно працює в `Repaying`. Закривати чекпоінт теж нічого:
/// `FR-011` про це не просить, а порожній облік нікому не шкодить.
pub fn refund(ctx: Context<Refund>) -> Result<()> {
    let issue = &ctx.accounts.issue;

    // Перевірка стану — явна, а не виведена із сум. Випуск у погашенні під
    // умову `raised < face` не підпадає й сам собою, але покладатись на це
    // означало б лишити повернення без замка рівно там, де в сховищах лежать
    // чужі гроші. Дозволених станів два: `Subscribing` — до першого виклику,
    // `Failed` — після нього.
    require!(
        matches!(issue.state, IssueState::Subscribing | IssueState::Failed),
        ClubError::IssueNotFailed
    );

    // Контракт із `subscribe`: вікно закрите **з** `subscription_end_ts`, не
    // після. Дві протилежні дії не діляться однією секундою, а зазор між ними
    // був би вікном, у якому неможливо ані внести, ані повернути.
    require!(
        Clock::get()?.unix_timestamp >= issue.subscription_end_ts,
        ClubError::SubscriptionWindowStillOpen
    );

    // `FR-010`: недозібраний — це той, у кого номінал не вибрано. Зібраний
    // випуск повернень не дає навіть до кінця вікна: `subscribe` закриває його
    // в `Funded` тієї ж миті, і гроші вже належать не інвесторам.
    require!(issue.raised < issue.face, ClubError::IssueNotFailed);

    let amount = ctx.accounts.investor_bond.amount;
    require!(amount > 0, ClubError::NothingToRefund);

    let source = issue.source;
    let seq = issue.seq.to_le_bytes();
    let issue_signer: &[&[&[u8]]] = &[&[ISSUE_SEED, source.as_ref(), &seq, &[issue.bump]]];

    // `FR-011` називає порядок: інвестор **повертає** бонд-токени й **забирає**
    // внесок. Спалення першим і робить другий переказ безпечним — вимога до
    // випуску зникає до того, як по ній платять, а не після.
    burn_checked(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            BurnChecked {
                mint: ctx.accounts.bond_mint.to_account_info(),
                from: ctx.accounts.investor_bond.to_account_info(),
                authority: ctx.accounts.investor.to_account_info(),
            },
        ),
        amount,
        ctx.accounts.bond_mint.decimals,
    )?;

    transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.subscription_vault.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: ctx.accounts.investor_usdc.to_account_info(),
                authority: ctx.accounts.issue.to_account_info(),
            },
            issue_signer,
        ),
        amount,
        ctx.accounts.usdc_mint.decimals,
    )?;

    let raised = u128::from(issue.raised)
        .checked_sub(u128::from(amount))
        .ok_or(ClubError::MathOverflow)?;
    let raised = u64::try_from(raised).map_err(|_| error!(ClubError::MathOverflow))?;

    let issue = &mut ctx.accounts.issue;
    // Зібране меншає разом зі сховищем і пропозицією бонду: усі три величини
    // рівні між собою від першого внеску, і повернення не має права їх
    // розвести. Історії «скільки колись зібрали» тут не лишається — жодна
    // вимога її не просить, а розбіжність між `raised` і рештою в сховищі
    // коштувала б рівно стільки, скільки в тому сховищі лежить.
    issue.raised = raised;
    // Перший виклик і є тим, хто позначає випуск недозібраним (`FR-011`).
    // Наступні застають `Failed` і просто пишуть його вдруге.
    issue.state = IssueState::Failed;

    Ok(())
}

#[derive(Accounts)]
pub struct Claim<'info> {
    /// Індекс живе у випуску, і claim його **не рухає** — тому `mut` тут
    /// немає. Обидва `has_one` не про зручність: сховище погашення й мінт
    /// бонду — це рівно ті дві речі, підміна яких перетворила б виплату на
    /// крадіжку. Перше називає, звідки беруться гроші, друге — чим міряється
    /// частка.
    #[account(has_one = escrow_vault, has_one = bond_mint)]
    pub issue: Account<'info, Issue>,

    /// Той самий резолв, що й у `subscribe`: seeds містять випуск і власника,
    /// тому чужий облік у набір не сходиться, а невідкритий — не існує.
    #[account(
        mut,
        seeds = [HOLDER_SEED, issue.key().as_ref(), owner.key().as_ref()],
        bump = holder.bump,
    )]
    pub holder: Account<'info, HolderCheckpoint>,

    /// `FR-016`: забирає власник, і підпис дає право забрати **своє**, а не
    /// відправити чуже куди завгодно — рахунок призначення прибитий до нього ж.
    pub owner: Signer<'info>,

    #[account(mut, token::mint = usdc_mint, token::authority = owner)]
    pub owner_usdc: InterfaceAccount<'info, TokenAccount>,

    /// `FR-014`: сюди перехоплення складало частку, звідси вона й іде. Сховище
    /// підписки в наборі відсутнє: гроші інвесторів і гроші на виплати не мають
    /// ділити одну інструкцію.
    #[account(mut, token::mint = usdc_mint)]
    pub escrow_vault: InterfaceAccount<'info, TokenAccount>,

    /// Баланс тут — це друга половина `FR-016`. Він читається **зараз**, і це
    /// правильно рівно доти, доки кожна передача бонду лишає по собі чекпоінт
    /// (`FR-017`): між двома чекпоінтами баланс не змінюється, тому «баланс за
    /// період» і «баланс зараз» — одне й те саме число.
    #[account(token::mint = bond_mint, token::authority = owner)]
    pub owner_bond: InterfaceAccount<'info, TokenAccount>,

    pub bond_mint: InterfaceAccount<'info, Mint>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    pub token_program: Program<'info, Token2022>,
}

/// Виплата власникові (`FR-016`).
///
/// Кумулятивний індекс (`FR-015`) робить цю інструкцію дешевою і не залежною
/// від кількості власників: перехоплення зрушило одне число у випуску, а хто
/// саме й коли забере свою частку — його вже не обходить. Тому й черги немає:
/// власник приходить у будь-який момент, а сума — це різниця індексів,
/// помножена на його баланс.
///
/// **Стан випуску тут нічого не вирішує, і це навмисно.** `FR-016` каже «в
/// будь-який момент», а найчастіший момент — саме після `Repaid`: зобов'язання
/// закрите, гроші лежать у сховищі, і замок за станом не дав би їх забрати
/// нікому. Відмовляють лише права, набір акаунтів і відсутність претензії.
///
/// **Поточний баланс замість історичного** — це контракт із `FR-017`, а не
/// спрощення. Він тримається тим, що передати бонд повз облік неможливо: гук
/// вбудований у сам мінт, тому кожна передача або зсуває чекпоінти обох сторін,
/// або не відбувається взагалі. Тримає цю обіцянку `execute` (`hook.rs`): він
/// нараховує обом сторонам на баланси до передачі й переносить чекпоінти на
/// сьогоднішній індекс, тому різниця тут рахується вже від нього. Хто чіпатиме
/// гук, мусить контракт зберегти: клієнт, який зрушив баланс без чекпоінта,
/// забере тут чужі гроші — атаку показує `tests/hook.rs`.
///
/// `accrued` — те, що гук нарахував при передачах; воно додається до претензії
/// й обнуляється тут же. Тому власник, який продав увесь бонд, усе одно приходить
/// сюди за нарахованим до продажу, і нульовий баланс претензії не скасовує.
pub fn claim(ctx: Context<Claim>) -> Result<()> {
    let issue = &ctx.accounts.issue;
    let holder = &ctx.accounts.holder;

    // Чекпоінт із майбутнього — це зіпсований облік, а не нульова претензія, і
    // він має власне ім'я. `claimable` повернула б на ньому те саме `None`, що
    // й на переповненні, а це дві різні аварії: одна означає «індекс поїхав
    // назад», друга — «сума не влізла». Звести їх в один код означало б віддати
    // найважчу діагностику одному номеру на двох.
    require!(
        holder.index_at_checkpoint <= issue.payout_index,
        ClubError::CheckpointAheadOfIndex
    );

    // `FR-016`: різниця індексів × баланс, плюс нараховане гуком. Ділення вниз
    // лишає залишок у сховищі — він не зникає, а дістається наступним claim'ам.
    let amount = math::claimable(
        issue.payout_index,
        holder.index_at_checkpoint,
        u128::from(ctx.accounts.owner_bond.amount),
        u128::from(holder.accrued),
    )
    .ok_or(ClubError::MathOverflow)?;
    let amount = u64::try_from(amount).map_err(|_| error!(ClubError::MathOverflow))?;

    // Нульова виплата — це відмова, а не успішний переказ нуля: транзакція, яка
    // нічого не переказала, але зрушила чекпоінт, виглядала б як виплата.
    require!(amount > 0, ClubError::NothingToClaim);

    let source = issue.source;
    let seq = issue.seq.to_le_bytes();
    let issue_signer: &[&[&[u8]]] = &[&[ISSUE_SEED, source.as_ref(), &seq, &[issue.bump]]];

    transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.escrow_vault.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: ctx.accounts.owner_usdc.to_account_info(),
                authority: ctx.accounts.issue.to_account_info(),
            },
            issue_signer,
        ),
        amount,
        ctx.accounts.usdc_mint.decimals,
    )?;

    let claimed_total = u128::from(holder.claimed_total)
        .checked_add(u128::from(amount))
        .ok_or(ClubError::MathOverflow)?;
    let claimed_total =
        u64::try_from(claimed_total).map_err(|_| error!(ClubError::MathOverflow))?;

    let index_at_checkpoint = issue.payout_index;

    let holder = &mut ctx.accounts.holder;
    // Чекпоінт переїжджає на сьогоднішній індекс цілком, разом із відкинутим
    // при діленні залишком: `CLAUDE.md` називає ціну прямо — округлення завжди
    // вниз, а решта лишається у сховищі.
    holder.index_at_checkpoint = index_at_checkpoint;
    holder.accrued = 0;
    holder.claimed_total = claimed_total;

    Ok(())
}
