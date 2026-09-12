//! Джерело revenue: реєстрація (`FR-004`, `FR-028`) і перехоплення доходу
//! (`FR-014`, `FR-019`, `FR-020`).
//!
//! Реєстрація — це підключення перехоплення, а не заявка: з цієї миті джерело
//! накопичує історію (`FR-028`), і саме її тривалість потім вирішує, чи
//! допускається джерело до випуску (`FR-007`). Тому `first_seen_ts` береться з
//! годинника ланцюга, а не приходить аргументом.
//!
//! Тут же записується `authority` — ключ, чий підпис `intercept` прийматиме за
//! доказ, що дохід приніс сам емітент (`FR-004`). Ключ береться таким, яким
//! його дав емітент, і не перевіряється: PDA чужої програми не підписати, тому
//! записати можна будь-що, а скористатись — лише своїм.
//!
//! Перехоплення — друга половина файлу і єдиний вхід доходу в протокол. Воно
//! живе всередині чужої інструкції, що збирає комісії, і тому влаштоване як
//! фільтр, а не як ворота: завалити чужий своп через стан нашого випуску
//! означало б не пропустити комісію ані нам, ані емітенту.

use {
    crate::{
        errors::ClubError,
        math,
        state::{Issue, IssueState, ProtocolConfig, RevenueSource, CONFIG_SEED, SOURCE_SEED},
    },
    anchor_lang::prelude::*,
    anchor_spl::{
        token_2022::{transfer_checked, Token2022, TransferChecked},
        token_interface::{Mint, TokenAccount},
    },
};

#[derive(Accounts)]
#[instruction(seq: u64)]
pub struct RegisterSource<'info> {
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, ProtocolConfig>,

    /// Seeds містять підписанта, тому джерело в чужій namespace не створити.
    #[account(
        init,
        payer = issuer,
        space = 8 + RevenueSource::INIT_SPACE,
        seeds = [SOURCE_SEED, issuer.key().as_ref(), &seq.to_le_bytes()],
        bump,
    )]
    pub source: Account<'info, RevenueSource>,

    #[account(mut)]
    pub issuer: Signer<'info>,

    /// CHECK: сюди подається PDA програми-емітента, яка викликатиме
    /// перехоплення. Довести, що ключ справді належить її програмі, на цьому
    /// боці нічим — доказом є підпис, і його вимагає `intercept` (`FR-004`).
    pub authority: UncheckedAccount<'info>,

    #[account(address = config.usdc_mint)]
    pub usdc_mint: InterfaceAccount<'info, Mint>,

    /// Валюта звіряється при реєстрації, а не при першому розщепленні: рахунок
    /// у чужому мінті виявився б рівно тоді, коли комісія вже мусила пройти.
    #[account(token::mint = usdc_mint)]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
}

pub fn register_source(ctx: Context<RegisterSource>, seq: u64) -> Result<()> {
    let source = &mut ctx.accounts.source;

    source.issuer = ctx.accounts.issuer.key();
    source.authority = ctx.accounts.authority.key();
    source.vault = ctx.accounts.vault.key();
    source.first_seen_ts = Clock::get()?.unix_timestamp;
    source.total_observed = 0;
    source.observed_before_issue = 0;
    source.active_issue = None;
    source.seq = seq;
    source.bump = ctx.bumps.source;

    Ok(())
}

#[derive(Accounts)]
pub struct Intercept<'info> {
    /// Права стереже `has_one = authority`, а не порівняння в тілі: дохід
    /// приймається тільки від того, хто **підписав** ключ, записаний при
    /// реєстрації (`FR-004`). Переданий program id доказом не є й тому в наборі
    /// відсутній.
    #[account(
        mut,
        has_one = authority @ ClubError::SourceAuthorityMismatch,
        has_one = vault,
    )]
    pub source: Account<'info, RevenueSource>,

    /// PDA програми-емітента. Підписати його може лише вона сама — це і є весь
    /// доказ походження доходу.
    pub authority: Signer<'info>,

    /// Рахунок джерела: сюди комісія вже надійшла, звідси йде частка. Решта
    /// нікуди не рухається — вона й так лежить на рахунку, який контролює
    /// емітент, і це та сама «решта — емітенту» з `FR-014`.
    ///
    /// `token::authority = authority` нічого не додає до прав (рахунок уже
    /// прибитий до джерела через `has_one`), але робить видимим інваріант, на
    /// якому все стоїть: програма рухає лише ті гроші, якими розпоряджається
    /// сам викликач. Без нього невідповідність упиралась би в безіменну відмову
    /// токен-програми при першому ж розщепленні.
    #[account(mut, token::mint = usdc_mint, token::authority = authority)]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    /// Випуску може не бути — і це не виняток, а нормальний стан.
    /// `FR-028` вимагає, щоб джерело накопичувало історію **незалежно від
    /// того, чи існує під ним випуск**: саме цією історією потім міряється
    /// допуск (`FR-007`). Тому акаунт опційний, і `None` подається program id
    /// нашої ж програми.
    ///
    /// Коли випуск поданий, він мусить бути тим, який джерело зараз забезпечує:
    /// `has_one = source` веде від випуску до джерела, `active_issue` — назад,
    /// і разом вони замикають кільце, яке `create_issue` зав'язав з обох боків.
    #[account(
        mut,
        has_one = source,
        has_one = escrow_vault,
        has_one = bond_mint,
        constraint = source.active_issue == Some(issue.key()) @ ClubError::SourceNotPledged,
    )]
    pub issue: Option<Account<'info, Issue>>,

    /// `FR-014`: сюди йде узгоджена частка. Сховище підписки в набір не подане
    /// взагалі — гроші інвесторів і гроші на виплати не мають ділити одну
    /// інструкцію.
    #[account(mut, token::mint = usdc_mint)]
    pub escrow_vault: Option<InterfaceAccount<'info, TokenAccount>>,

    /// `FR-015`: пропозиція бонду — знаменник індексу. Читається з мінта, а не
    /// з `issue.raised`: індекс мусить ділитись на те, що справді існує в обігу.
    pub bond_mint: Option<InterfaceAccount<'info, Mint>>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    pub token_program: Program<'info, Token2022>,
}

/// Перехоплення доходу (`FR-014`, `FR-019`, `FR-020`) — єдиний вхід доходу в
/// протокол (`FR-004`).
///
/// Це **фільтр, а не ворота**. Інструкція вбудована в чужу інструкцію, що
/// збирає комісії, і жодна причина, яка стосується стану нашого випуску, не
/// має права завалити чужий своп: відмова тут означала б, що комісія не дійшла
/// до емітента взагалі. Тому відмовляють лише права й неузгоджений набір
/// акаунтів, а стан випуску вирішує рівно одне — розщеплювати цей потік чи
/// пропустити його повз.
///
/// Звідси три різні поведінки на одному вході:
/// - випуску немає — дохід тільки спостерігається (`FR-028`);
/// - випуск є, але зобов'язання ще не виникло (`Subscribing`, `Funded`) або вже
///   закрите (`Repaid`) — так само лише спостерігається. Друге і є `FR-019`:
///   перехоплення припиняється саме, без окремої дії емітента;
/// - випуск у погашенні — потік розщеплюється тут же, у тій самій транзакції.
///
/// `amount` приходить від емітента, і перевірити його ланцюг не може: скільки
/// комісії виникло, знає лише та інструкція, всередині якої це сталося.
/// Заниження — це те саме «пустив комісії повз перехоплення», якого `FR-030`
/// свідомо не береться доводити; завищення обертається проти самого емітента,
/// бо частка списується з його ж рахунку.
pub fn intercept(ctx: Context<Intercept>, amount: u64) -> Result<()> {
    // `FR-028`: спостереження безумовне. Воно стоїть першим саме тому, що не
    // залежить ні від випуску, ні від його стану — історія джерела належить
    // джерелу, а не випуску.
    let observed = u128::from(ctx.accounts.source.total_observed)
        .checked_add(u128::from(amount))
        .ok_or(ClubError::MathOverflow)?;
    let observed = u64::try_from(observed).map_err(|_| error!(ClubError::MathOverflow))?;
    ctx.accounts.source.total_observed = observed;

    let (Some(issue), Some(escrow_vault), Some(bond_mint)) = (
        ctx.accounts.issue.as_ref(),
        ctx.accounts.escrow_vault.as_ref(),
        ctx.accounts.bond_mint.as_ref(),
    ) else {
        return Ok(());
    };

    // `FR-012` провів межу: зобов'язання виникає з видачею, і до неї потік
    // емітента наш. `PastDue` тут навмисно **не** проходить: `FR-022` вимагає
    // на ньому іншої ставки — стелі протоколу, — і пропустити його на
    // `pledge_bps` означало б розщепити правильно виглядаючою, але хибною
    // часткою. Стан додасть T042 разом зі ставкою.
    if issue.state != IssueState::Repaying {
        return Ok(());
    }

    // `FR-020`: у сховище йде частка, але не більше за залишок зобов'язання.
    // Надлишок нікуди не переказується — він просто не залишає рахунку
    // емітента, і це та сама «та сама транзакція», якої вимагає вимога.
    let remaining = u128::from(issue.obligation_total)
        .checked_sub(u128::from(issue.repaid_total))
        .ok_or(ClubError::MathOverflow)?;
    let split = math::split_intercept(u128::from(amount), issue.pledge_bps, remaining)
        .ok_or(ClubError::MathOverflow)?;

    // `FR-015`: одиниць бонду мусить бути ненульова кількість — інакше «виплата
    // на одиницю» не визначена. У погашенні це виконано завжди (`raised == face`
    // і пропозиція дорівнює зібраному), тому замок названий окремо: сплутати
    // «нема на що ділити» з переповненням означало б віддати найважчу
    // діагностику одному коду на двох.
    require!(bond_mint.supply > 0, ClubError::ZeroBondSupply);
    let payout_index = math::advance_index(
        issue.payout_index,
        split.to_escrow,
        u128::from(bond_mint.supply),
    )
    .ok_or(ClubError::MathOverflow)?;

    let repaid_total = u128::from(issue.repaid_total)
        .checked_add(split.to_escrow)
        .ok_or(ClubError::MathOverflow)?;
    let repaid_total = u64::try_from(repaid_total).map_err(|_| error!(ClubError::MathOverflow))?;
    let closes = repaid_total == issue.obligation_total;

    let to_escrow = u64::try_from(split.to_escrow).map_err(|_| error!(ClubError::MathOverflow))?;

    // Нульового переказу окремою гілкою не обходимо: він законний, а гілка
    // коштувала б розгалуження, якого ніхто не ганяє. Нуль тут настає на дрібних
    // надходженнях, де частка округлилась униз, — і залишок лишається емітенту.
    transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.vault.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: escrow_vault.to_account_info(),
                authority: ctx.accounts.authority.to_account_info(),
            },
        ),
        to_escrow,
        ctx.accounts.usdc_mint.decimals,
    )?;

    if let Some(issue) = ctx.accounts.issue.as_mut() {
        issue.repaid_total = repaid_total;
        // `FR-015`: рухається індекс, а не N переказів. Саме тому вартість
        // обробки надходження не залежить від кількості власників (`SC-005`).
        issue.payout_index = payout_index;
        // `FR-019`: перехоплення припиняється в ту саму мить, коли виплачено
        // повне зобов'язання, — і припиняє його цей рядок, а не окрема дія
        // емітента. Наступні надходження знайдуть випуск у `Repaid` і пройдуть
        // повз розщеплення цілими.
        if closes {
            issue.state = IssueState::Repaid;
        }
    }

    Ok(())
}
