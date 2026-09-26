//! Оферти вторинного ринку (`FR-024`…`FR-027`, `FR-035`).
//!
//! **Ціну ставить продавець.** Протокол її не рахує, не котирує і другою
//! стороною не стає: оферта — це «стільки бондів за стільки USDC», і єдине, що
//! перевіряє програма, — що обидва числа додатні й що бонд справді є.
//!
//! **Бонд лежить у сховищі, а не в обіцянці.** `FR-025` вимагає, щоб
//! виставлене не можна було продати вдруге, і тримається це не прапорцем у
//! стані, а тим, що токенів у продавця вже немає: вони переїхали на рахунок,
//! чия authority — PDA самої оферти. Забрати їх звідти вміє лише ця програма і
//! лише через `buy_offer` або `cancel_offer`.
//!
//! **Переказ іде звичайним `transfer_checked`** — саме тому, що він звичайний,
//! спрацьовує гук: стороні, яка віддає бонд, закривається чекпоінт на тому
//! балансі, який вона тримала до переказу, і все накопичене до цієї миті
//! лишається за нею (`FR-017`). Набір акаунтів гука не переписується тут
//! руками, а резолвиться з того самого списку в мінті, яким користується
//! Token-2022: `add_extra_accounts_for_execute_cpi` читає його з ланцюга.
//! Переписаний список одного дня розійшовся б із тим, що записав `create_issue`.
//!
//! **Сховищу потрібен власний облік** (`FR-038`): без чекпоінта на PDA оферти
//! гук відмовив би, а разом із ним — і весь переказ. Облік відкривається при
//! виставленні, через `open_position` ядра, який дозвільний і власника-PDA
//! допускає навмисно. Виплати, що набігли на ньому, поки оферта стояла,
//! належать продавцеві — і `buy_offer`, і `cancel_offer` віддають їх йому.

use {
    crate::{
        errors::MarketError,
        state::{Offer, ESCROW_SEED, OFFER_SEED, PROCEEDS_SEED},
    },
    anchor_lang::{
        prelude::*,
        solana_program::program::{invoke, invoke_signed},
    },
    anchor_spl::{
        token_2022::{
            close_account, spl_token_2022, transfer_checked, CloseAccount, Token2022,
            TransferChecked,
        },
        token_interface::{Mint, TokenAccount},
    },
    daddys_club::{
        math,
        program::DaddysClub,
        state::{HolderCheckpoint, Issue, ProtocolConfig},
    },
    spl_transfer_hook_interface::onchain::add_extra_accounts_for_execute_cpi,
};

/// Акаунти, якими Token-2022 дійде до гука: список у мінті, випуск і два
/// чекпоінти. Резолвер вибирає з них за ключем, тому порядок тут не важить —
/// важить, що кожен потрібний є.
struct HookRoute<'a, 'info> {
    bond_mint: &'a InterfaceAccount<'info, Mint>,
    issue: AccountInfo<'info>,
    holder_source: AccountInfo<'info>,
    holder_destination: AccountInfo<'info>,
    extra_account_meta_list: AccountInfo<'info>,
    club_program: AccountInfo<'info>,
    token_program: AccountInfo<'info>,
}

/// Переказує бонд так само, як це зробив би клієнт: звичайним
/// `transfer_checked` із акаунтами гука, які резолвить сам список у мінті.
///
/// Саме тут видно, чому вторинка не в ядрі: цей виклик веде в Token-2022, а той
/// — у гук. Був би гук цією ж програмою, стек замкнувся б на собі й транзакція
/// впала б на `ReentrancyNotAllowed`, не дійшовши до обліку.
fn move_bond<'info>(
    from: &InterfaceAccount<'info, TokenAccount>,
    to: &InterfaceAccount<'info, TokenAccount>,
    authority: AccountInfo<'info>,
    amount: u64,
    route: &HookRoute<'_, 'info>,
    signer: Option<&[&[&[u8]]]>,
) -> Result<()> {
    let mut instruction = spl_token_2022::instruction::transfer_checked(
        route.token_program.key,
        &from.key(),
        &route.bond_mint.key(),
        &to.key(),
        authority.key,
        &[],
        amount,
        route.bond_mint.decimals,
    )?;

    let mut infos = vec![
        from.to_account_info(),
        route.bond_mint.to_account_info(),
        to.to_account_info(),
        authority.clone(),
    ];

    add_extra_accounts_for_execute_cpi(
        &mut instruction,
        &mut infos,
        route.club_program.key,
        from.to_account_info(),
        route.bond_mint.to_account_info(),
        to.to_account_info(),
        authority.clone(),
        amount,
        &[
            route.extra_account_meta_list.clone(),
            route.issue.clone(),
            route.holder_source.clone(),
            route.holder_destination.clone(),
            route.club_program.clone(),
        ],
    )?;

    match signer {
        Some(seeds) => invoke_signed(&instruction, &infos, seeds),
        None => invoke(&instruction, &infos),
    }?;

    Ok(())
}

/// Закриває порожній токен-рахунок оферти й повертає оренду тому, хто її вніс.
fn close_token_account<'info>(
    account: &InterfaceAccount<'info, TokenAccount>,
    destination: AccountInfo<'info>,
    authority: AccountInfo<'info>,
    token_program: &Program<'info, Token2022>,
    signer: &[&[&[u8]]],
) -> Result<()> {
    close_account(CpiContext::new_with_signer(
        token_program.to_account_info(),
        CloseAccount {
            account: account.to_account_info(),
            destination,
            authority,
        },
        signer,
    ))
}

/// Скільки накопичив облік до цієї миті. Читається з акаунта, а не з типу в
/// наборі: між `try_accounts` і цим рядком стоїть переказ, і саме він кладе
/// туди число, заради якого все й робиться.
fn accrued_on(holder: &UncheckedAccount) -> Result<u64> {
    let data = holder.try_borrow_data()?;

    Ok(HolderCheckpoint::try_deserialize(&mut &data[..])?.accrued)
}

// ---- Виставлення (`FR-024`, `FR-025`) --------------------------------------

#[derive(Accounts)]
#[instruction(nonce: u64)]
pub struct CreateOffer<'info> {
    /// Випуск, чий бонд продається. Читається, але не змінюється: оферта на
    /// облік випуску не впливає ніяк. `has_one` замикає мінт на випуск — бонд
    /// чужого випуску в цей набір не сходиться.
    #[account(has_one = bond_mint)]
    pub issue: Box<Account<'info, Issue>>,

    #[account(mut)]
    pub seller: Signer<'info>,

    /// Рахунок, з якого їде бонд. `token::authority` тут не зайвий попри
    /// підпис: продати можна лише своє, а не те, на що виписано делегата.
    #[account(mut, token::mint = bond_mint, token::authority = seller)]
    pub seller_bond: Box<InterfaceAccount<'info, TokenAccount>>,

    pub bond_mint: Box<InterfaceAccount<'info, Mint>>,

    /// `nonce` у seeds — щоб у продавця могло стояти кілька оферт на один
    /// випуск. Повторний `nonce` упирається в `init`.
    #[account(
        init,
        payer = seller,
        space = 8 + Offer::INIT_SPACE,
        seeds = [OFFER_SEED, issue.key().as_ref(), seller.key().as_ref(), &nonce.to_le_bytes()],
        bump,
    )]
    pub offer: Box<Account<'info, Offer>>,

    /// Сховище оферти. Authority — сам PDA оферти, тому рухати його вміє лише
    /// ця програма. Адреса дерівується, а не приноситься клієнтом: оферта живе
    /// довше за транзакцію, і знайти її сховище мусить будь-хто, хто бачить
    /// саму оферту.
    #[account(
        init,
        payer = seller,
        seeds = [ESCROW_SEED, offer.key().as_ref()],
        bump,
        token::mint = bond_mint,
        token::authority = offer,
        token::token_program = token_program,
    )]
    pub token_escrow: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: облік продавця. Його адресу резолвить список у мінті, і звіряє
    /// її Token-2022 — тут він лише кандидат, який передається далі. Свого
    /// `has_one` не треба: підставити чужий облік означає не знайтись у
    /// резолвері, а не пройти повз нього.
    #[account(mut)]
    pub holder_seller: UncheckedAccount<'info>,

    /// CHECK: облік сховища. На вході його ще немає — його створює `CPI
    /// open_position` нижче, — тому типом він бути не може. Якщо хтось відкрив
    /// його наперед (відкриття дозвільне), програма це побачить і не
    /// створюватиме вдруге.
    #[account(mut)]
    pub holder_escrow: UncheckedAccount<'info>,

    /// CHECK: TLV-список гука. Його розкладку задає Token-2022, тому акаунтом
    /// Anchor він не є; читає його резолвер.
    pub extra_account_meta_list: UncheckedAccount<'info>,

    /// Ядро: сюди йде `open_position`, і воно ж — програма гука, яку
    /// Token-2022 покличе зсередини переказу.
    pub club_program: Program<'info, DaddysClub>,

    pub token_program: Program<'info, Token2022>,

    pub system_program: Program<'info, System>,
}

/// Виставляє бонд на продаж (`FR-024`, `FR-025`).
///
/// `amount` — скільки одиниць номіналу продається, `price` — скільки USDC за
/// весь лот. Часткового виконання немає: `FR-024` каже «будь-хто викуповує її
/// цілком», і ділити лот означало б рахувати ціну за одиницю з округленням —
/// тобто створювати або губити копійки там, де їх ніхто не просив.
pub fn create_offer(ctx: Context<CreateOffer>, nonce: u64, amount: u64, price: u64) -> Result<()> {
    require!(amount > 0 && price > 0, MarketError::OfferTermsInvalid);
    // Відмова дійшла б і від токен-програми, але вже зсередини CPI і чужим
    // кодом: `InsufficientFunds` Token-2022 у логах не відрізнити від відмови
    // самого гука. Назване ім'я тут коштує одне порівняння.
    require!(
        ctx.accounts.seller_bond.amount >= amount,
        MarketError::InsufficientBondBalance
    );

    // `FR-038`: без обліку на PDA оферти гук відмовить, а з ним упаде й переказ.
    // Відкриття дозвільне, тому облік міг відкрити хтось наперед — тоді він уже
    // на місці, і другий `init` лише зламав би законну оферту.
    if ctx.accounts.holder_escrow.data_is_empty() {
        daddys_club::cpi::open_position(CpiContext::new(
            ctx.accounts.club_program.to_account_info(),
            daddys_club::cpi::accounts::OpenPosition {
                issue: ctx.accounts.issue.to_account_info(),
                holder: ctx.accounts.holder_escrow.to_account_info(),
                payer: ctx.accounts.seller.to_account_info(),
                owner: ctx.accounts.offer.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
            },
        ))?;
    }

    move_bond(
        &ctx.accounts.seller_bond,
        &ctx.accounts.token_escrow,
        ctx.accounts.seller.to_account_info(),
        amount,
        &HookRoute {
            bond_mint: &ctx.accounts.bond_mint,
            issue: ctx.accounts.issue.to_account_info(),
            holder_source: ctx.accounts.holder_seller.to_account_info(),
            holder_destination: ctx.accounts.holder_escrow.to_account_info(),
            extra_account_meta_list: ctx.accounts.extra_account_meta_list.to_account_info(),
            club_program: ctx.accounts.club_program.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
        },
        None,
    )?;

    let offer = &mut ctx.accounts.offer;
    offer.seller = ctx.accounts.seller.key();
    offer.issue = ctx.accounts.issue.key();
    offer.amount = amount;
    offer.price = price;
    offer.token_escrow = ctx.accounts.token_escrow.key();
    offer.nonce = nonce;
    offer.bump = ctx.bumps.offer;

    Ok(())
}

// ---- Викуп (`FR-026`, `FR-035`, `FR-038`) ----------------------------------

/// Торгова комісія вторинки (`FR-035`), утримана з ціни лота.
///
/// Округлення вниз, як і скрізь у протоколі: відкинутий залишок дістається
/// продавцеві, а протокол ніколи не бере більше за оголошену ставку. Дзеркало —
/// `tradingFee` у `packages/sdk/src/market.ts`: форма виставлення показує
/// продавцеві «You receive» до підпису, і тест SDK звіряє формулу з цим файлом.
fn trading_fee(price: u128, fee_bps: u16) -> Result<u128> {
    price
        .checked_mul(u128::from(fee_bps))
        .and_then(|scaled| scaled.checked_div(math::BPS_DENOM))
        .ok_or_else(|| error!(MarketError::MathOverflow))
}

#[derive(Accounts)]
pub struct BuyOffer<'info> {
    /// Ставка комісії й скарбниця — з протоколу, а не з оферти: `FR-035` робить
    /// ставку параметром, тому ставка угоди — та, що в конфізі зараз. `has_one`
    /// прибиває і скарбницю, і валюту: обидві підміни ведуть чужі гроші не туди.
    #[account(has_one = fee_vault, has_one = usdc_mint)]
    pub config: Box<Account<'info, ProtocolConfig>>,

    /// Сховище погашення й мінт бонду беруться звідси й ніяк інакше: перше —
    /// звідки `claim` візьме накопичене за час оферти, другий — чим міряється
    /// сам лот.
    #[account(has_one = bond_mint, has_one = escrow_vault)]
    pub issue: Box<Account<'info, Issue>>,

    /// Викуплена оферта — це не оферта зі статусом, а оферта, якої більше
    /// немає: акаунт закривається, оренда повертається продавцеві. `has_one`
    /// замикає її на продавця, випуск і власне сховище.
    #[account(
        mut,
        close = seller,
        has_one = seller,
        has_one = issue,
        has_one = token_escrow,
    )]
    pub offer: Box<Account<'info, Offer>>,

    /// Сховище оферти. Після переказу воно порожнє й закривається — оренда теж
    /// іде продавцеві, бо вносив її він.
    #[account(mut, token::mint = bond_mint, token::authority = offer)]
    pub token_escrow: Box<InterfaceAccount<'info, TokenAccount>>,

    /// USDC-рахунок оферти, і живе він рівно одну інструкцію. `claim` віддає
    /// виплату лише на рахунок **власника** обліку, а власник тут — PDA оферти;
    /// тому накопичене за час оферти проходить через цей рахунок і тією ж
    /// інструкцією їде продавцеві. Дешевше було б навчити ядро переносити
    /// `accrued` між обліками, але це нова інструкція в ядрі заради ринку — нова
    /// поверхня атаки там, де лежать чужі гроші.
    #[account(
        init,
        payer = buyer,
        seeds = [PROCEEDS_SEED, offer.key().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = offer,
        token::token_program = token_program,
    )]
    pub offer_proceeds: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: продавець. Підпису не дає й не може — у цьому й сенс оферти. До
    /// оферти прибитий `has_one`, і саме сюди повертається оренда.
    #[account(mut)]
    pub seller: UncheckedAccount<'info>,

    #[account(mut, token::mint = usdc_mint, token::authority = seller)]
    pub seller_usdc: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut)]
    pub buyer: Signer<'info>,

    #[account(mut, token::mint = usdc_mint, token::authority = buyer)]
    pub buyer_usdc: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Куди їде лот. Рахунок бонду покупець заводить сам — це не наша оренда;
    /// облік (`FR-038`) — інша річ, його відкриває ця ж інструкція.
    #[account(mut, token::mint = bond_mint, token::authority = buyer)]
    pub buyer_bond: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: облік покупця. `FR-038` каже відкривати його в тій самій
    /// транзакції, що й купівля; у покупця, який уже тримає цей бонд, він є.
    #[account(mut)]
    pub holder_buyer: UncheckedAccount<'info>,

    /// CHECK: облік сховища. Його адресу резолвить список у мінті; читається
    /// він **після** переказу — саме переказ і кладе туди накопичене.
    #[account(mut)]
    pub holder_escrow: UncheckedAccount<'info>,

    #[account(mut, token::mint = usdc_mint)]
    pub escrow_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, token::mint = usdc_mint)]
    pub fee_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub bond_mint: Box<InterfaceAccount<'info, Mint>>,

    pub usdc_mint: Box<InterfaceAccount<'info, Mint>>,

    /// CHECK: TLV-список гука; читає його резолвер.
    pub extra_account_meta_list: UncheckedAccount<'info>,

    pub club_program: Program<'info, DaddysClub>,

    pub token_program: Program<'info, Token2022>,

    pub system_program: Program<'info, System>,
}

impl<'info> BuyOffer<'info> {
    fn usdc_transfer(
        &self,
        from: &InterfaceAccount<'info, TokenAccount>,
        to: &InterfaceAccount<'info, TokenAccount>,
        authority: AccountInfo<'info>,
        amount: u64,
        signer: Option<&[&[&[u8]]]>,
    ) -> Result<()> {
        let context = CpiContext::new(
            self.token_program.to_account_info(),
            TransferChecked {
                from: from.to_account_info(),
                mint: self.usdc_mint.to_account_info(),
                to: to.to_account_info(),
                authority,
            },
        );
        let decimals = self.usdc_mint.decimals;

        match signer {
            Some(seeds) => transfer_checked(context.with_signer(seeds), amount, decimals),
            None => transfer_checked(context, amount, decimals),
        }
    }
}

/// Викуп оферти (`FR-026`, `FR-035`, `FR-038`).
///
/// **Атомарність — це те, що лишається після відмови.** Лот, оплата, комісія,
/// облік покупця і накопичене за час оферти — в одній інструкції, тому стану
/// «бонд поїхав, а гроші ні» не існує.
///
/// **Чиє те, що набігло, поки оферта стояла.** Продавця: доки її не викупили,
/// він міг скасувати оферту й забрати бонд назад — ризик увесь час був його.
/// Гук кладе це на облік сховища в мить переказу лота, а `claim` віддає звідти
/// рівно ту суму, бо баланс сховища на той момент уже нульовий.
///
/// **Покупець платить рівно `price`.** Комісія утримується з того, що отримає
/// продавець (`FR-035`), тому оферта коштує стільки, скільки в ній написано.
pub fn buy_offer(ctx: Context<BuyOffer>) -> Result<()> {
    let amount = ctx.accounts.offer.amount;
    let price = ctx.accounts.offer.price;

    let fee = trading_fee(u128::from(price), ctx.accounts.config.trading_fee_bps)?;
    let to_seller = u128::from(price)
        .checked_sub(fee)
        .ok_or(MarketError::MathOverflow)?;
    let fee = u64::try_from(fee).map_err(|_| error!(MarketError::MathOverflow))?;
    let to_seller = u64::try_from(to_seller).map_err(|_| error!(MarketError::MathOverflow))?;

    // `FR-038`: без обліку покупця гук відмовить, а з ним упаде вся покупка.
    if ctx.accounts.holder_buyer.data_is_empty() {
        daddys_club::cpi::open_position(CpiContext::new(
            ctx.accounts.club_program.to_account_info(),
            daddys_club::cpi::accounts::OpenPosition {
                issue: ctx.accounts.issue.to_account_info(),
                holder: ctx.accounts.holder_buyer.to_account_info(),
                payer: ctx.accounts.buyer.to_account_info(),
                owner: ctx.accounts.buyer.to_account_info(),
                system_program: ctx.accounts.system_program.to_account_info(),
            },
        ))?;
    }

    let issue_key = ctx.accounts.issue.key();
    let seller_key = ctx.accounts.seller.key();
    let nonce = ctx.accounts.offer.nonce.to_le_bytes();
    let bump = [ctx.accounts.offer.bump];
    let signer: &[&[&[u8]]] = &[&[
        OFFER_SEED,
        issue_key.as_ref(),
        seller_key.as_ref(),
        &nonce,
        &bump,
    ]];

    // Лот покупцеві. Переказ звичайний, тому гук закриває чекпоінт сховища на
    // балансі до переказу — саме там і опиняється накопичене за час оферти.
    move_bond(
        &ctx.accounts.token_escrow,
        &ctx.accounts.buyer_bond,
        ctx.accounts.offer.to_account_info(),
        amount,
        &HookRoute {
            bond_mint: &ctx.accounts.bond_mint,
            issue: ctx.accounts.issue.to_account_info(),
            holder_source: ctx.accounts.holder_escrow.to_account_info(),
            holder_destination: ctx.accounts.holder_buyer.to_account_info(),
            extra_account_meta_list: ctx.accounts.extra_account_meta_list.to_account_info(),
            club_program: ctx.accounts.club_program.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
        },
        Some(signer),
    )?;

    if accrued_on(&ctx.accounts.holder_escrow)? > 0 {
        daddys_club::cpi::claim(CpiContext::new_with_signer(
            ctx.accounts.club_program.to_account_info(),
            daddys_club::cpi::accounts::Claim {
                issue: ctx.accounts.issue.to_account_info(),
                holder: ctx.accounts.holder_escrow.to_account_info(),
                owner: ctx.accounts.offer.to_account_info(),
                owner_usdc: ctx.accounts.offer_proceeds.to_account_info(),
                escrow_vault: ctx.accounts.escrow_vault.to_account_info(),
                owner_bond: ctx.accounts.token_escrow.to_account_info(),
                bond_mint: ctx.accounts.bond_mint.to_account_info(),
                usdc_mint: ctx.accounts.usdc_mint.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            },
            signer,
        ))?;

        ctx.accounts.offer_proceeds.reload()?;
        let claimed = ctx.accounts.offer_proceeds.amount;

        ctx.accounts.usdc_transfer(
            &ctx.accounts.offer_proceeds,
            &ctx.accounts.seller_usdc,
            ctx.accounts.offer.to_account_info(),
            claimed,
            Some(signer),
        )?;
    }

    ctx.accounts.usdc_transfer(
        &ctx.accounts.buyer_usdc,
        &ctx.accounts.seller_usdc,
        ctx.accounts.buyer.to_account_info(),
        to_seller,
        None,
    )?;

    // Нульова комісія — законний стан протоколу (`FR-035` ставки не обмежує
    // знизу), а переказ на нуль лише спалив би CU.
    if fee > 0 {
        ctx.accounts.usdc_transfer(
            &ctx.accounts.buyer_usdc,
            &ctx.accounts.fee_vault,
            ctx.accounts.buyer.to_account_info(),
            fee,
            None,
        )?;
    }

    // Обидва рахунки оферти порожні. Оренда повертається тим, хто її вносив:
    // сховище бонду — продавцеві, тимчасовий USDC-рахунок — покупцеві.
    close_token_account(
        &ctx.accounts.token_escrow,
        ctx.accounts.seller.to_account_info(),
        ctx.accounts.offer.to_account_info(),
        &ctx.accounts.token_program,
        signer,
    )?;
    close_token_account(
        &ctx.accounts.offer_proceeds,
        ctx.accounts.buyer.to_account_info(),
        ctx.accounts.offer.to_account_info(),
        &ctx.accounts.token_program,
        signer,
    )?;

    Ok(())
}

// ---- Скасування (`FR-027`) -------------------------------------------------

#[derive(Accounts)]
pub struct CancelOffer<'info> {
    #[account(has_one = bond_mint, has_one = escrow_vault)]
    pub issue: Box<Account<'info, Issue>>,

    /// Скасована оферта зникає так само, як викуплена: закривається сама й
    /// закриває обидва свої рахунки. `has_one = seller` і є тим замком, через
    /// який чужу оферту не скасувати, — підпис нижче мусить збігтися з тим, кого
    /// вона назвала.
    #[account(
        mut,
        close = seller,
        has_one = seller,
        has_one = issue,
        has_one = token_escrow,
    )]
    pub offer: Box<Account<'info, Offer>>,

    #[account(mut, token::mint = bond_mint, token::authority = offer)]
    pub token_escrow: Box<InterfaceAccount<'info, TokenAccount>>,

    /// Той самий тимчасовий рахунок, що й у викупі, і з тієї ж причини: `claim`
    /// платить лише власникові обліку, а власник тут — PDA оферти.
    #[account(
        init,
        payer = seller,
        seeds = [PROCEEDS_SEED, offer.key().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = offer,
        token::token_program = token_program,
    )]
    pub offer_proceeds: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut)]
    pub seller: Signer<'info>,

    /// Куди повертається лот. Рахунок може бути й не той, з якого бонд пішов, —
    /// важливо, що він продавців: `FR-027` каже «повертає токени власнику».
    #[account(mut, token::mint = bond_mint, token::authority = seller)]
    pub seller_bond: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(mut, token::mint = usdc_mint, token::authority = seller)]
    pub seller_usdc: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: облік продавця; його резолвить список у мінті.
    #[account(mut)]
    pub holder_seller: UncheckedAccount<'info>,

    /// CHECK: облік сховища; читається після переказу.
    #[account(mut)]
    pub holder_escrow: UncheckedAccount<'info>,

    #[account(mut, token::mint = usdc_mint)]
    pub escrow_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub bond_mint: Box<InterfaceAccount<'info, Mint>>,

    pub usdc_mint: Box<InterfaceAccount<'info, Mint>>,

    /// CHECK: TLV-список гука; читає його резолвер.
    pub extra_account_meta_list: UncheckedAccount<'info>,

    pub club_program: Program<'info, DaddysClub>,

    pub token_program: Program<'info, Token2022>,

    pub system_program: Program<'info, System>,
}

/// Скасування оферти (`FR-027`).
///
/// **Повністю і без комісії.** Скарбниці протоколу в цьому наборі акаунтів
/// немає взагалі — не «комісія нульова», а нікуди її взяти: протокол заробляє
/// на угоді (`FR-035`), а передумати — не угода.
///
/// Накопичене за час оферти повертається тією ж дорогою, що й у викупі: воно
/// лежить на обліку сховища, і `claim` віддає його продавцеві. Без цього
/// кроку скасування коштувало б власникові всіх виплат за період, поки бонд
/// стояв на продажу, — тобто «повертає повністю» було б неправдою.
pub fn cancel_offer(ctx: Context<CancelOffer>) -> Result<()> {
    let amount = ctx.accounts.offer.amount;

    let issue_key = ctx.accounts.issue.key();
    let seller_key = ctx.accounts.seller.key();
    let nonce = ctx.accounts.offer.nonce.to_le_bytes();
    let bump = [ctx.accounts.offer.bump];
    let signer: &[&[&[u8]]] = &[&[
        OFFER_SEED,
        issue_key.as_ref(),
        seller_key.as_ref(),
        &nonce,
        &bump,
    ]];

    move_bond(
        &ctx.accounts.token_escrow,
        &ctx.accounts.seller_bond,
        ctx.accounts.offer.to_account_info(),
        amount,
        &HookRoute {
            bond_mint: &ctx.accounts.bond_mint,
            issue: ctx.accounts.issue.to_account_info(),
            holder_source: ctx.accounts.holder_escrow.to_account_info(),
            holder_destination: ctx.accounts.holder_seller.to_account_info(),
            extra_account_meta_list: ctx.accounts.extra_account_meta_list.to_account_info(),
            club_program: ctx.accounts.club_program.to_account_info(),
            token_program: ctx.accounts.token_program.to_account_info(),
        },
        Some(signer),
    )?;

    if accrued_on(&ctx.accounts.holder_escrow)? > 0 {
        daddys_club::cpi::claim(CpiContext::new_with_signer(
            ctx.accounts.club_program.to_account_info(),
            daddys_club::cpi::accounts::Claim {
                issue: ctx.accounts.issue.to_account_info(),
                holder: ctx.accounts.holder_escrow.to_account_info(),
                owner: ctx.accounts.offer.to_account_info(),
                owner_usdc: ctx.accounts.offer_proceeds.to_account_info(),
                escrow_vault: ctx.accounts.escrow_vault.to_account_info(),
                owner_bond: ctx.accounts.token_escrow.to_account_info(),
                bond_mint: ctx.accounts.bond_mint.to_account_info(),
                usdc_mint: ctx.accounts.usdc_mint.to_account_info(),
                token_program: ctx.accounts.token_program.to_account_info(),
            },
            signer,
        ))?;

        ctx.accounts.offer_proceeds.reload()?;
        let claimed = ctx.accounts.offer_proceeds.amount;

        transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.offer_proceeds.to_account_info(),
                    mint: ctx.accounts.usdc_mint.to_account_info(),
                    to: ctx.accounts.seller_usdc.to_account_info(),
                    authority: ctx.accounts.offer.to_account_info(),
                },
                signer,
            ),
            claimed,
            ctx.accounts.usdc_mint.decimals,
        )?;
    }

    // Обидва рахунки порожні, і оренду за обидва вносив продавець.
    close_token_account(
        &ctx.accounts.token_escrow,
        ctx.accounts.seller.to_account_info(),
        ctx.accounts.offer.to_account_info(),
        &ctx.accounts.token_program,
        signer,
    )?;
    close_token_account(
        &ctx.accounts.offer_proceeds,
        ctx.accounts.seller.to_account_info(),
        ctx.accounts.offer.to_account_info(),
        &ctx.accounts.token_program,
        signer,
    )?;

    Ok(())
}
