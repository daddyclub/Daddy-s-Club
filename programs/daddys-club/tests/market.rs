//! `create_offer` (`FR-024`, `FR-025`) — виставлення бонду на продаж на
//! справжньому байткоді всіх трьох програм.
//!
//! Тести лежать у цьому крейті, а не в крейті ринку, з тієї ж причини, що й
//! `tests/swap.rs`: харнес один, і він піднімає всі три програми разом із
//! Token-2022. Розвести їх означало б мати два описи одного світу.
//!
//! Головне, що тут доводиться, — **виставлення не губить накопиченого**. Бонд
//! їде у сховище звичайним `transfer_checked`, тому спрацьовує гук: продавцеві
//! закривається чекпоінт на тому балансі, який він тримав до виставлення, і
//! 480 USDC, що набігли, поки бонд був у нього, лишаються за ним, а не
//! переїжджають разом із токенами. Числа ті самі, що в `tests/hook.rs` і
//! `tests/invest.rs`: 12 000 USDC у сховищі погашення, 4% номіналу на руках.
//!
//! Другий бік — те, чого без окремої програми не було б узагалі: сховище
//! належить PDA оферти, і бонд у нього заходить із ядра-гука. Що цей самий
//! переказ із **ядра** неможливий (`ReentrancyNotAllowed`), показала проба
//! перед `T033a`; тут видно, що з окремої програми він проходить.

#[path = "harness.rs"]
mod harness;

use {
    anchor_lang::InstructionData,
    daddys_club::state::{HolderCheckpoint, Issue},
    daddys_market::{errors::MarketError, state::Offer},
    harness::*,
    mollusk_svm::result::{Check, InstructionResult},
    solana_account::Account,
    solana_address::Address as Pubkey,
    solana_instruction::{AccountMeta, Instruction},
    solana_program_error::ProgramError,
};

const SELLER_BOND: Pubkey = Pubkey::new_from_array([71u8; 32]);

/// Скільки вже перехоплено у сховище погашення — 12 000 USDC.
const PAID_IN: u64 = 12_000_000_000;
/// Індекс, який лишило по собі це перехоплення: `PAID_IN * SCALE / face`.
const INDEX: u128 = 48_000_000_000;
/// Баланс продавця — 10 000 одиниць номіналу з 250 000.
const BALANCE: u64 = 10_000_000_000;
/// Що йому з `PAID_IN` належить: 4% — 480 USDC.
const OWED: u64 = 480_000_000;
/// Половина позиції — звичайна оферта, після якої бонд є і в продавця, і в
/// сховищі.
const LOT: u64 = BALANCE / 2;
/// Ціна лота. Її ставить продавець, і протокол про неї нічого не знає.
const PRICE: u64 = 4_900_000_000;
const NONCE: u64 = 0;

fn market_err(error: MarketError) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(error)))
}

fn offer_key() -> Pubkey {
    offer_pda(demo_issue(), INVESTOR, NONCE).0
}

fn escrow_key() -> Pubkey {
    offer_escrow_pda(offer_key()).0
}

/// Облік у тому вигляді, в якому його лишають `open_position` і попередні
/// виплати.
fn ledger(owner: Pubkey, index_at_checkpoint: u128) -> HolderCheckpoint {
    HolderCheckpoint {
        issue: anchor_key(demo_issue()),
        owner: anchor_key(owner),
        index_at_checkpoint,
        accrued: 0,
        claimed_total: 0,
        bump: holder_pda(demo_issue(), owner).1,
    }
}

/// Виставлення в тому вигляді, в якому його подає клієнт. Порядок акаунтів —
/// оголошення `CreateOffer`; чекпоінти й список гука подаються кандидатами,
/// з яких резолвер вибере те, що записано в мінті.
fn create_offer_ix(amount: u64, price: u64) -> Instruction {
    Instruction::new_with_bytes(
        market_id(),
        &daddys_market::instruction::CreateOffer {
            nonce: NONCE,
            amount,
            price,
        }
        .data(),
        vec![
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(INVESTOR, true),
            AccountMeta::new(SELLER_BOND, false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new(offer_key(), false),
            AccountMeta::new(escrow_key(), false),
            AccountMeta::new(holder_pda(demo_issue(), INVESTOR).0, false),
            AccountMeta::new(holder_pda(demo_issue(), offer_key()).0, false),
            AccountMeta::new_readonly(extra_metas_pda(BOND_MINT).0, false),
            AccountMeta::new_readonly(club_id(), false),
            AccountMeta::new_readonly(token_program().0, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    )
}

/// Світ у погашенні: у продавця є бонд і відкритий облік із чекпоінтом у нулі,
/// у випуску вже перехоплено `PAID_IN`. Оферти й сховища ще немає — їх створює
/// сама інструкція, тому в наборі вони порожні.
fn world() -> Vec<(Pubkey, Account)> {
    let issue = repaying(PAID_IN, INDEX);

    vec![
        (demo_issue(), anchor_account(&issue)),
        (
            holder_pda(demo_issue(), INVESTOR).0,
            anchor_account(&ledger(INVESTOR, 0)),
        ),
        (holder_pda(demo_issue(), offer_key()).0, uninitialized()),
        (INVESTOR, wallet()),
        (SELLER_BOND, bond_account(INVESTOR, BALANCE)),
        (offer_key(), uninitialized()),
        (escrow_key(), uninitialized()),
        (BOND_MINT, bond_mint(demo_issue(), issue.raised)),
        (extra_metas_pda(BOND_MINT).0, extra_metas_account()),
        token_program(),
        system_program(),
        program_account(club_id()),
    ]
}

fn stored_offer(result: &InstructionResult) -> Offer {
    decode(result, &offer_key())
}

fn holder(result: &InstructionResult, owner: Pubkey) -> HolderCheckpoint {
    decode(result, &holder_pda(demo_issue(), owner).0)
}

/// `FR-024`: оферта — це кількість і ціна, і обидві задає продавець. `FR-025`:
/// токени вже не в нього — вони на рахунку, чия authority — PDA оферти.
#[test]
fn listing_moves_the_bond_into_the_escrow_and_writes_the_offer() {
    let result = setup().process_and_validate_instruction(
        &create_offer_ix(LOT, PRICE),
        &world(),
        &[Check::success()],
    );

    assert_eq!(token_balance(&result, &SELLER_BOND), BALANCE - LOT);
    assert_eq!(
        token_balance(&result, &escrow_key()),
        LOT,
        "бонд не доїхав до сховища"
    );

    let offer = stored_offer(&result);
    assert_eq!(offer.seller, anchor_key(INVESTOR));
    assert_eq!(offer.issue, anchor_key(demo_issue()));
    assert_eq!(offer.amount, LOT);
    assert_eq!(offer.price, PRICE, "ціну протокол не перераховує");
    assert_eq!(
        offer.token_escrow,
        anchor_key(escrow_key()),
        "оферта не веде до власного сховища"
    );
    assert_eq!(offer.nonce, NONCE);
    assert_eq!(offer.bump, offer_pda(demo_issue(), INVESTOR, NONCE).1);
}

/// Головне в цій задачі. Виставлення — це передача, тому спрацьовує гук: усе,
/// що накопичилось, поки бонд був у продавця, переїжджає йому в `accrued`, а
/// чекпоінт стає на сьогоднішній індекс. Без цього 480 USDC поїхали б у
/// сховище разом із токенами й дістались би покупцеві — тому саме тут видно,
/// що вторинка не ламає `FR-017`.
#[test]
fn listing_settles_the_seller_at_the_moment_before_it() {
    let result = setup().process_and_validate_instruction(
        &create_offer_ix(LOT, PRICE),
        &world(),
        &[Check::success()],
    );

    let seller = holder(&result, INVESTOR);
    assert_eq!(
        seller.accrued, OWED,
        "продавцеві нараховано не на баланс до виставлення"
    );
    assert_eq!(seller.index_at_checkpoint, INDEX);

    // Індекс рухає перехоплення, а не оферта: випуск лишається як був.
    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.payout_index, INDEX);
    assert_eq!(issue.repaid_total, PAID_IN);
}

/// `FR-038`: бонд можна передати лише тому, для кого відкрито облік, — а
/// сховище оферти теж «хтось». Облік відкривається в тій самій транзакції, і
/// відкривається він **на PDA оферти**, а не на продавця: інакше гук поклав би
/// нарахування сховища туди ж, де лежить нарахування продавця.
#[test]
fn the_escrow_gets_its_own_ledger_in_the_same_transaction() {
    let result = setup().process_and_validate_instruction(
        &create_offer_ix(LOT, PRICE),
        &world(),
        &[Check::success()],
    );

    let escrow = holder(&result, offer_key());
    assert_eq!(escrow.owner, anchor_key(offer_key()));
    assert_eq!(escrow.issue, anchor_key(demo_issue()));
    assert_eq!(
        escrow.index_at_checkpoint, INDEX,
        "свіжий облік мусить починатись від сьогоднішнього індексу"
    );
    assert_eq!(
        escrow.accrued, 0,
        "сховищу нараховано те, чого воно не тримало"
    );
}

/// Відкриття обліку дозвільне, тому облік сховища міг відкрити хтось наперед —
/// наприклад, щоб зайняти адресу й зламати оферту ще до того, як її виставили.
/// Другий `init` на тому ж акаунті впав би, тому програма дивиться, чи він уже
/// є. Оферта від цього не змінюється ні на байт.
#[test]
fn a_ledger_opened_in_advance_does_not_block_the_listing() {
    let ahead = replacing(
        &world(),
        holder_pda(demo_issue(), offer_key()).0,
        anchor_account(&ledger(offer_key(), INDEX)),
    );

    let result = setup().process_and_validate_instruction(
        &create_offer_ix(LOT, PRICE),
        &ahead,
        &[Check::success()],
    );

    assert_eq!(token_balance(&result, &escrow_key()), LOT);
    assert_eq!(holder(&result, offer_key()).index_at_checkpoint, INDEX);
}

/// `FR-024`: ціну і кількість задає продавець — але нульова оферта це не
/// оферта. Нуль у кількості дав би покупцеві заплатити за порожнечу, нуль у
/// ціні — забрати бонд задарма.
#[test]
fn an_offer_of_nothing_or_for_nothing_is_refused() {
    for (what, amount, price) in [("нуль бондів", 0, PRICE), ("нуль USDC", LOT, 0)] {
        println!("оферта на {what}");

        setup().process_and_validate_instruction(
            &create_offer_ix(amount, price),
            &world(),
            &[market_err(MarketError::OfferTermsInvalid)],
        );
    }
}

/// Продати можна лише те, що є на руках. Відмова прийшла б і від токен-програми
/// — але вже зсередини CPI і її власним кодом, який у логах не відрізнити від
/// відмови гука.
#[test]
fn selling_more_than_is_held_is_refused_by_name() {
    setup().process_and_validate_instruction(
        &create_offer_ix(BALANCE + 1, PRICE),
        &world(),
        &[market_err(MarketError::InsufficientBondBalance)],
    );
}

/// Дві оферти з одним `nonce` — це одна адреса, тобто спроба переписати
/// виставлене. Бонд першої оферти лежить у її сховищі, і переписаний запис
/// відрізав би від нього і продавця, і покупця.
#[test]
fn the_same_nonce_does_not_list_twice() {
    let mollusk = setup();
    let listed = mollusk.process_and_validate_instruction(
        &create_offer_ix(LOT, PRICE),
        &world(),
        &[Check::success()],
    );

    let existing = world()
        .into_iter()
        .map(|(key, account)| match listed.get_account(&key) {
            Some(updated) => (key, updated.clone()),
            None => (key, account),
        })
        .collect::<Vec<_>>();

    let result = mollusk.process_instruction(&create_offer_ix(LOT, PRICE), &existing);

    assert!(
        !result.program_result.is_ok(),
        "оферту переписали: {:?}",
        result.program_result
    );
    assert_eq!(
        token_balance(&result, &escrow_key()),
        LOT,
        "у сховищі опинилось не те, що виставляли"
    );
}
