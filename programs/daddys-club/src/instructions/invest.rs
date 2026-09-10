//! Позиція інвестора: відкриття обліку (`FR-038`) і підписка
//! (`FR-008`…`FR-010`, `FR-013`).
//!
//! `HolderCheckpoint` — це не запис «про всяк випадок», а перепустка. Список
//! акаунтів гука, покладений у мінт при створенні випуску, резолвить чекпоінти
//! обох сторін переказу; акаунта немає — резолв не сходиться, і Token-2022
//! відхиляє передачу цілком, замість того щоб пропустити її повз облік. Тому
//! `open_position` мусить створювати рівно той PDA, який гук шукає, і жодного
//! іншого: адреса зафіксована в `hook_account_metas` і переписати її нічим.
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
//! доки випуск не зібрано повністю (`FR-008`, `FR-012`) — видачу пише T021.

use {
    crate::{
        errors::ClubError,
        state::{HolderCheckpoint, Issue, IssueState, HOLDER_SEED, ISSUE_SEED},
    },
    anchor_lang::prelude::*,
    anchor_spl::{
        token_2022::{mint_to, transfer_checked, MintTo, Token2022, TransferChecked},
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
