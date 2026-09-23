//! Оферти вторинного ринку (`FR-024`, `FR-025`).
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
//! спрацьовує гук: продавцеві закривається чекпоінт на тому балансі, який він
//! тримав до виставлення, і все накопичене до цієї миті лишається за ним
//! (`FR-017`). Набір акаунтів гука не переписується тут руками, а резолвиться з
//! того самого списку в мінті, яким користується Token-2022:
//! `add_extra_accounts_for_execute_cpi` читає його з ланцюга. Переписаний
//! список одного дня розійшовся б із тим, що записав `create_issue`.
//!
//! **Сховищу потрібен власний облік** (`FR-038`): без чекпоінта на PDA оферти
//! гук відмовив би, а разом із ним — і весь переказ. Облік відкривається тут же,
//! через `open_position` ядра, який дозвільний і власника-PDA допускає навмисно.
//! Виплати, що накопичаться на ньому, поки оферта стоїть, належать продавцеві —
//! їх забирають `buy_offer` і `cancel_offer` (`T035`, `T036`).

use {
    crate::{
        errors::MarketError,
        state::{Offer, ESCROW_SEED, OFFER_SEED},
    },
    anchor_lang::{prelude::*, solana_program::program::invoke},
    anchor_spl::{
        token_2022::{spl_token_2022, Token2022},
        token_interface::{Mint, TokenAccount},
    },
    daddys_club::{program::DaddysClub, state::Issue},
    spl_transfer_hook_interface::onchain::add_extra_accounts_for_execute_cpi,
};

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
    pub offer_escrow: Box<InterfaceAccount<'info, TokenAccount>>,

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

/// Переказує бонд у сховище оферти так само, як це зробив би клієнт: звичайним
/// `transfer_checked`, з акаунтами гука, які резолвить сам список у мінті.
///
/// Саме тут видно, чому вторинка не в ядрі: цей `invoke` веде в Token-2022, а
/// той — у гук. Був би гук цією ж програмою, стек замкнувся б на собі й
/// транзакція впала б на `ReentrancyNotAllowed`, не дійшовши до обліку.
fn move_bond_into_escrow(accounts: &CreateOffer, amount: u64) -> Result<()> {
    let mut instruction = spl_token_2022::instruction::transfer_checked(
        &accounts.token_program.key(),
        &accounts.seller_bond.key(),
        &accounts.bond_mint.key(),
        &accounts.offer_escrow.key(),
        &accounts.seller.key(),
        &[],
        amount,
        accounts.bond_mint.decimals,
    )?;

    let mut infos = vec![
        accounts.seller_bond.to_account_info(),
        accounts.bond_mint.to_account_info(),
        accounts.offer_escrow.to_account_info(),
        accounts.seller.to_account_info(),
    ];

    add_extra_accounts_for_execute_cpi(
        &mut instruction,
        &mut infos,
        &accounts.club_program.key(),
        accounts.seller_bond.to_account_info(),
        accounts.bond_mint.to_account_info(),
        accounts.offer_escrow.to_account_info(),
        accounts.seller.to_account_info(),
        amount,
        &[
            accounts.extra_account_meta_list.to_account_info(),
            accounts.issue.to_account_info(),
            accounts.holder_seller.to_account_info(),
            accounts.holder_escrow.to_account_info(),
            accounts.club_program.to_account_info(),
        ],
    )?;

    invoke(&instruction, &infos)?;

    Ok(())
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

    move_bond_into_escrow(ctx.accounts, amount)?;

    let offer = &mut ctx.accounts.offer;
    offer.seller = ctx.accounts.seller.key();
    offer.issue = ctx.accounts.issue.key();
    offer.amount = amount;
    offer.price = price;
    offer.token_escrow = ctx.accounts.offer_escrow.key();
    offer.nonce = nonce;
    offer.bump = ctx.bumps.offer;

    Ok(())
}
