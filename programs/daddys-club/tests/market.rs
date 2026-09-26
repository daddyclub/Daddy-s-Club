//! Вторинний ринок (`FR-024`…`FR-027`, `FR-035`, `FR-038`) — виставлення,
//! викуп і скасування на справжньому байткоді всіх трьох програм.
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
const SELLER_USDC: Pubkey = Pubkey::new_from_array([72u8; 32]);
const BUYER_USDC: Pubkey = Pubkey::new_from_array([73u8; 32]);
const BUYER_BOND: Pubkey = Pubkey::new_from_array([74u8; 32]);

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

// ---- Викуп (`FR-026`, `FR-035`, `FR-038`) ----------------------------------

/// З чим покупець приходить на ринок.
const BUYER_CASH: u64 = 10_000_000_000;

/// Комісія за ставкою **протоколу**, а не за літералом: інакше тест лишився б
/// зеленим після зміни ставки в конфізі й перестав би про неї говорити.
fn fee_of(price: u64) -> u64 {
    price * u64::from(stored_config().trading_fee_bps) / 10_000
}

fn to_seller(price: u64) -> u64 {
    price - fee_of(price)
}

fn proceeds_key() -> Pubkey {
    offer_proceeds_pda(offer_key()).0
}

/// Викуп у тому вигляді, в якому його подає покупець. Порядок — оголошення
/// `BuyOffer`.
fn buy_offer_ix() -> Instruction {
    Instruction::new_with_bytes(
        market_id(),
        &daddys_market::instruction::BuyOffer {}.data(),
        vec![
            AccountMeta::new_readonly(config_pda().0, false),
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(offer_key(), false),
            AccountMeta::new(escrow_key(), false),
            AccountMeta::new(proceeds_key(), false),
            AccountMeta::new(INVESTOR, false),
            AccountMeta::new(SELLER_USDC, false),
            AccountMeta::new(BUYER, true),
            AccountMeta::new(BUYER_USDC, false),
            AccountMeta::new(BUYER_BOND, false),
            AccountMeta::new(holder_pda(demo_issue(), BUYER).0, false),
            AccountMeta::new(holder_pda(demo_issue(), offer_key()).0, false),
            AccountMeta::new(ESCROW_VAULT, false),
            AccountMeta::new(FEE_VAULT, false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(extra_metas_pda(BOND_MINT).0, false),
            AccountMeta::new_readonly(club_id(), false),
            AccountMeta::new_readonly(token_program().0, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    )
}

/// Світ виставлення плюс усе, що потрібно для викупу: протокол, покупець із
/// грошима, сховище погашення й скарбниця комісій.
fn market_world() -> Vec<(Pubkey, Account)> {
    let issue = repaying(PAID_IN, INDEX);

    let mut accounts = world();
    accounts.extend([
        (config_pda().0, anchor_account(&stored_config())),
        (SELLER_USDC, usdc_account(INVESTOR, 0)),
        (BUYER, wallet()),
        (BUYER_USDC, usdc_account(BUYER, BUYER_CASH)),
        (BUYER_BOND, bond_account(BUYER, 0)),
        (holder_pda(demo_issue(), BUYER).0, uninitialized()),
        (proceeds_key(), uninitialized()),
        (ESCROW_VAULT, usdc_account(demo_issue(), issue.repaid_total)),
        (FEE_VAULT, usdc_account(ADMIN, 0)),
        (USDC_MINT, usdc_mint(1_000_000_000_000_000)),
    ]);

    accounts
}

/// Світ після виставлення: та сама оферта, але вже на ланцюгу. `moved_index` —
/// це те, що сталося, поки оферта стояла: у випуск прийшло ще перехоплення, і
/// індекс поїхав уперед.
fn after_listing(moved_index: Option<(u64, u128)>) -> Vec<(Pubkey, Account)> {
    let listed = setup().process_and_validate_instruction(
        &create_offer_ix(LOT, PRICE),
        &market_world(),
        &[Check::success()],
    );

    let mut accounts: Vec<(Pubkey, Account)> = market_world()
        .into_iter()
        .map(|(key, account)| match listed.get_account(&key) {
            Some(updated) => (key, updated.clone()),
            None => (key, account),
        })
        .collect();

    if let Some((repaid, index)) = moved_index {
        accounts = replacing(
            &accounts,
            demo_issue(),
            anchor_account(&repaying(repaid, index)),
        );
        accounts = replacing(&accounts, ESCROW_VAULT, usdc_account(demo_issue(), repaid));
    }

    accounts
}

fn lamports_of(result: &InstructionResult, key: &Pubkey) -> u64 {
    result
        .get_account(key)
        .map(|account| account.lamports)
        .unwrap_or_default()
}

/// `FR-026`: купівля атомарна — бонд у покупця, гроші в продавця, і жодного
/// стану посередині. `FR-035`: комісія утримується з того, що отримує
/// продавець, тому покупець платить рівно стільки, скільки написано в оферті.
#[test]
fn a_purchase_hands_over_the_bond_and_the_money_at_once() {
    let result = setup().process_and_validate_instruction(
        &buy_offer_ix(),
        &after_listing(None),
        &[Check::success()],
    );

    assert_eq!(token_balance(&result, &BUYER_BOND), LOT, "лот не доїхав");
    assert_eq!(
        token_balance(&result, &BUYER_USDC),
        BUYER_CASH - PRICE,
        "покупець заплатив не ціну оферти"
    );
    assert_eq!(
        token_balance(&result, &SELLER_USDC),
        to_seller(PRICE),
        "продавцеві дісталось не те, що лишається після комісії"
    );
    assert_eq!(
        token_balance(&result, &FEE_VAULT),
        fee_of(PRICE),
        "комісія протоколу порахована не за його ж ставкою"
    );
}

/// Викуплена оферта не має статусу — її просто більше немає: обидва її рахунки
/// закриті, і оренда, яку вносив продавець, повернулась йому. Без цього в
/// ланцюгу лишався б акаунт, який виглядає як чинна оферта з порожнім сховищем.
#[test]
fn an_offer_bought_is_an_offer_gone() {
    let before = after_listing(None);
    let seller_before = before
        .iter()
        .find(|(key, _)| *key == INVESTOR)
        .map(|(_, account)| account.lamports)
        .expect("продавець є у світі");

    let result =
        setup().process_and_validate_instruction(&buy_offer_ix(), &before, &[Check::success()]);

    assert_eq!(lamports_of(&result, &offer_key()), 0, "оферта лишилась");
    assert_eq!(lamports_of(&result, &escrow_key()), 0, "сховище лишилось");
    assert_eq!(
        lamports_of(&result, &proceeds_key()),
        0,
        "тимчасовий USDC-рахунок лишився"
    );
    assert!(
        lamports_of(&result, &INVESTOR) > seller_before,
        "оренда не повернулась продавцеві"
    );
}

/// `FR-038`: покупцеві облік відкривається в тій самій транзакції. Без нього
/// гук відмовив би, і купівля впала б цілком — тому це не зручність, а умова
/// того, що вторинка взагалі працює.
#[test]
fn the_buyer_gets_a_ledger_opened_in_the_same_transaction() {
    let result = setup().process_and_validate_instruction(
        &buy_offer_ix(),
        &after_listing(None),
        &[Check::success()],
    );

    let buyer = holder(&result, BUYER);
    assert_eq!(buyer.owner, anchor_key(BUYER));
    assert_eq!(
        buyer.index_at_checkpoint, INDEX,
        "свіжий облік мусить починатись від сьогоднішнього індексу"
    );
    assert_eq!(
        buyer.accrued, 0,
        "покупцеві нараховано те, чого він не тримав"
    );
}

/// Найтонше місце вторинки. Поки оферта стоїть, бонд лежить у сховищі — і
/// виплати за нього набігають на облік **сховища**, а не продавця. Належать
/// вони продавцеві: доки оферту не викупили, він міг її скасувати й забрати
/// бонд назад. Тому викуп віддає йому і ціну, і те, що набігло.
///
/// Без цього кроку 240 USDC лишились би на обліку PDA, який після викупу
/// перестає існувати, — тобто зникли б для всіх.
#[test]
fn what_accrued_while_the_offer_stood_goes_to_the_seller() {
    // Поки оферта стояла, прийшло ще стільки ж: індекс подвоївся.
    let moved = (PAID_IN * 2, INDEX * 2);
    // Частка лота за цей проміжок: (96e9 − 48e9) × 5e9 / 1e12.
    let accrued = 240_000_000;

    let result = setup().process_and_validate_instruction(
        &buy_offer_ix(),
        &after_listing(Some(moved)),
        &[Check::success()],
    );

    assert_eq!(
        token_balance(&result, &SELLER_USDC),
        to_seller(PRICE) + accrued,
        "накопичене за час оферти не дійшло до продавця"
    );
    assert_eq!(
        token_balance(&result, &ESCROW_VAULT),
        moved.0 - accrued,
        "зі сховища погашення пішла не та сума"
    );

    // Покупець платить ту саму ціну: накопичене — не його справа.
    assert_eq!(token_balance(&result, &BUYER_USDC), BUYER_CASH - PRICE);

    // І забирає він бонд із чистим обліком: усе, що було до нього, уже
    // виплачене, а його власний відлік починається з цього індексу.
    let buyer = holder(&result, BUYER);
    assert_eq!(buyer.accrued, 0);
    assert_eq!(buyer.index_at_checkpoint, moved.1);
}

/// Гроші йдуть тому, кого назвала оферта, і туди, куди показує протокол.
/// Підмінити продавця — це забрати чужий лот за свої гроші; підмінити
/// скарбницю — це забрати комісію протоколу собі.
#[test]
fn neither_the_seller_nor_the_fee_vault_can_be_swapped() {
    let outsider_usdc = Pubkey::new_from_array([75u8; 32]);

    for (what, metas) in [
        ("продавця", (5usize, OUTSIDER)),
        ("скарбницю комісій", (13usize, outsider_usdc)),
    ] {
        println!("підміна: {what}");

        let mut instruction = buy_offer_ix();
        instruction.accounts[metas.0].pubkey = metas.1;

        let world = {
            let mut accounts = after_listing(None);
            accounts.push((OUTSIDER, wallet()));
            accounts.push((outsider_usdc, usdc_account(OUTSIDER, 0)));
            accounts
        };

        let result = setup().process_instruction(&instruction, &world);

        assert!(
            !result.program_result.is_ok(),
            "підміна пройшла: {:?}",
            result.program_result
        );
    }
}

// ---- Скасування (`FR-027`) -------------------------------------------------

/// Чужі рахунки для спроби скасувати не свою оферту. Вони потрібні саме тому,
/// що без них тест упав би на `token::authority` і нічого не сказав би про
/// власника оферти.
const OUTSIDER_BOND: Pubkey = Pubkey::new_from_array([76u8; 32]);
const OUTSIDER_USDC: Pubkey = Pubkey::new_from_array([77u8; 32]);

fn anchor_err(code: anchor_lang::error::ErrorCode) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(code)))
}

/// Скасування в тому вигляді, в якому його подає продавець. Порядок —
/// оголошення `CancelOffer`.
fn cancel_offer_ix() -> Instruction {
    Instruction::new_with_bytes(
        market_id(),
        &daddys_market::instruction::CancelOffer {}.data(),
        vec![
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(offer_key(), false),
            AccountMeta::new(escrow_key(), false),
            AccountMeta::new(proceeds_key(), false),
            AccountMeta::new(INVESTOR, true),
            AccountMeta::new(SELLER_BOND, false),
            AccountMeta::new(SELLER_USDC, false),
            AccountMeta::new(holder_pda(demo_issue(), INVESTOR).0, false),
            AccountMeta::new(holder_pda(demo_issue(), offer_key()).0, false),
            AccountMeta::new(ESCROW_VAULT, false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(extra_metas_pda(BOND_MINT).0, false),
            AccountMeta::new_readonly(club_id(), false),
            AccountMeta::new_readonly(token_program().0, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    )
}

fn lamports_in(world: &[(Pubkey, Account)], key: Pubkey) -> u64 {
    world
        .iter()
        .find(|(existing, _)| *existing == key)
        .map(|(_, account)| account.lamports)
        .expect("акаунт є у світі")
}

/// `FR-027`: передумати — це не угода, тому лот повертається цілим. Протокол
/// заробляє на купівлі (`FR-035`) і тільки на ній: скарбниці комісій у наборі
/// акаунтів скасування немає взагалі — не «ставка нульова», а нікуди її взяти.
#[test]
fn cancelling_returns_the_whole_lot_and_charges_nothing() {
    assert!(
        !cancel_offer_ix()
            .accounts
            .iter()
            .any(|meta| meta.pubkey == FEE_VAULT),
        "скарбниця комісій потрапила в набір скасування"
    );

    let result = setup().process_and_validate_instruction(
        &cancel_offer_ix(),
        &after_listing(None),
        &[Check::success()],
    );

    assert_eq!(
        token_balance(&result, &SELLER_BOND),
        BALANCE,
        "продавцеві повернувся не весь лот"
    );
    assert_eq!(
        token_balance(&result, &SELLER_USDC),
        0,
        "за скасування з продавця щось узяли або йому щось доплатили"
    );
}

/// Те саме найтонше місце, що й у викупі, лише з іншого боку. Поки оферта
/// стоїть, виплати набігають на облік **сховища**; якби скасування їх не
/// забирало, «повертає повністю» було б неправдою — бонд повернувся б, а плата
/// за час, поки він стояв на продажу, лишилась би на обліку PDA, який тією ж
/// інструкцією перестає бути комусь потрібним.
#[test]
fn what_accrued_while_the_offer_stood_comes_back_with_the_lot() {
    // Поки оферта стояла, прийшло ще стільки ж: індекс подвоївся.
    let moved = (PAID_IN * 2, INDEX * 2);
    // Частка лота за цей проміжок: (96e9 − 48e9) × 5e9 / 1e12.
    let accrued = 240_000_000;

    let result = setup().process_and_validate_instruction(
        &cancel_offer_ix(),
        &after_listing(Some(moved)),
        &[Check::success()],
    );

    assert_eq!(token_balance(&result, &SELLER_BOND), BALANCE);
    assert_eq!(
        token_balance(&result, &SELLER_USDC),
        accrued,
        "накопичене за час оферти не дійшло до продавця"
    );
    assert_eq!(
        token_balance(&result, &ESCROW_VAULT),
        moved.0 - accrued,
        "зі сховища погашення пішла не та сума"
    );

    // Друга половина позиції весь цей час була на руках, і нараховане за неї
    // нікуди не зникло: воно лежить на обліку продавця й чекає свого `claim`.
    // Разом із 480 USDC, які туди поклало саме виставлення.
    let seller = holder(&result, INVESTOR);
    assert_eq!(
        seller.accrued,
        OWED + accrued,
        "скасування зачепило те, що продавець заробив поза офертою"
    );
    assert_eq!(seller.index_at_checkpoint, moved.1);
}

/// `has_one = seller` і є тим замком, через який чужу оферту не скасувати.
/// Чужинець приходить із власними рахунками — саме тому, що інакше відмова
/// прийшла б від `token::authority` і про власника оферти не сказала б нічого.
#[test]
fn a_stranger_cannot_cancel_an_offer_they_did_not_place() {
    let stranger_ledger = holder_pda(demo_issue(), OUTSIDER).0;

    let mut instruction = cancel_offer_ix();
    instruction.accounts[4].pubkey = OUTSIDER;
    instruction.accounts[5].pubkey = OUTSIDER_BOND;
    instruction.accounts[6].pubkey = OUTSIDER_USDC;
    instruction.accounts[7].pubkey = stranger_ledger;

    let mut world = after_listing(None);
    world.extend([
        (OUTSIDER, wallet()),
        (OUTSIDER_BOND, bond_account(OUTSIDER, 0)),
        (OUTSIDER_USDC, usdc_account(OUTSIDER, 0)),
        (stranger_ledger, anchor_account(&ledger(OUTSIDER, INDEX))),
    ]);

    let result = setup().process_and_validate_instruction(
        &instruction,
        &world,
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintHasOne)],
    );

    assert_eq!(
        token_balance(&result, &escrow_key()),
        LOT,
        "лот пішов зі сховища попри відмову"
    );
    assert_eq!(token_balance(&result, &OUTSIDER_BOND), 0);
}

/// Скасована оферта, як і викуплена, не має статусу — її немає. Обидва її
/// рахунки закриті, і оренда за них повернулась тому, хто її вносив: тут це
/// продавець, і за сховище бонду, і за тимчасовий USDC-рахунок.
#[test]
fn a_cancelled_offer_is_an_offer_gone() {
    let before = after_listing(None);
    let seller_before = lamports_in(&before, INVESTOR);
    // Тимчасовий USDC-рахунок у цю суму не входить навмисно: продавець і
    // вносить за нього оренду, і отримує її назад у тій самій інструкції.
    let rent_back = lamports_in(&before, offer_key()) + lamports_in(&before, escrow_key());
    assert!(rent_back > 0, "у світі до скасування оферти не було");

    let result =
        setup().process_and_validate_instruction(&cancel_offer_ix(), &before, &[Check::success()]);

    assert_eq!(lamports_of(&result, &offer_key()), 0, "оферта лишилась");
    assert_eq!(lamports_of(&result, &escrow_key()), 0, "сховище лишилось");
    assert_eq!(
        lamports_of(&result, &proceeds_key()),
        0,
        "тимчасовий USDC-рахунок лишився"
    );
    assert_eq!(
        lamports_of(&result, &INVESTOR),
        seller_before + rent_back,
        "оренда повернулась не вся або не продавцеві"
    );
}

/// Скасувати двічі нічого не вийде, і доводить це не прапорець, а те, що
/// оферти вже немає: другий виклик приходить у порожній акаунт. Саме тому
/// `OfferNotActive` у `MarketError` і не знадобилась.
#[test]
fn an_offer_cancelled_once_cannot_be_cancelled_again() {
    let before = after_listing(None);
    let mollusk = setup();
    let cancelled =
        mollusk.process_and_validate_instruction(&cancel_offer_ix(), &before, &[Check::success()]);

    let after = before
        .into_iter()
        .map(|(key, account)| match cancelled.get_account(&key) {
            Some(updated) => (key, updated.clone()),
            None => (key, account),
        })
        .collect::<Vec<_>>();

    mollusk.process_and_validate_instruction(
        &cancel_offer_ix(),
        &after,
        &[anchor_err(
            anchor_lang::error::ErrorCode::AccountNotInitialized,
        )],
    );
}

/// `nonce` — номер слота, а не лічильник. Облік сховища переживає свою оферту,
/// тому друге виставлення з тим самим `nonce` дешевше рівно на його оренду.
/// Рішення від 2026-09-24, docs/PLAN.md → «Модель даних» → `Offer`.
#[test]
fn the_ledger_of_a_gone_offer_is_reused_by_the_next_one() {
    let ledger_key = holder_pda(demo_issue(), offer_key()).0;
    let mollusk = setup();

    // --- перше виставлення
    let before1 = market_world();
    let listed1 = mollusk.process_and_validate_instruction(
        &create_offer_ix(LOT, PRICE),
        &before1,
        &[Check::success()],
    );
    let cost1 = lamports_in(&before1, INVESTOR) - lamports_of(&listed1, &INVESTOR);
    let ledger_rent = lamports_of(&listed1, &ledger_key);
    println!("оренда обліку сховища = {ledger_rent} лампортів");
    println!("перше виставлення коштувало {cost1}");

    let apply = |world: Vec<(Pubkey, Account)>, result: &InstructionResult| {
        world
            .into_iter()
            .map(|(key, account)| match result.get_account(&key) {
                Some(updated) => (key, updated.clone()),
                None => (key, account),
            })
            .collect::<Vec<_>>()
    };

    // --- скасування
    let after_listing1 = apply(market_world(), &listed1);
    let cancelled = mollusk.process_and_validate_instruction(
        &cancel_offer_ix(),
        &after_listing1,
        &[Check::success()],
    );
    let after_cancel = apply(after_listing1, &cancelled);

    println!(
        "після скасування: оферта = {}, сховище = {}, облік = {}",
        lamports_in(&after_cancel, offer_key()),
        lamports_in(&after_cancel, escrow_key()),
        lamports_in(&after_cancel, ledger_key)
    );

    // --- друге виставлення з тим самим nonce
    let listed2 = mollusk.process_and_validate_instruction(
        &create_offer_ix(LOT, PRICE),
        &after_cancel,
        &[Check::success()],
    );
    let cost2 = lamports_in(&after_cancel, INVESTOR) - lamports_of(&listed2, &INVESTOR);
    println!("друге виставлення коштувало {cost2}");
    println!("різниця = {}", cost1 - cost2);

    assert_eq!(
        cost1 - cost2,
        ledger_rent,
        "друге виставлення мало б зекономити рівно оренду обліку"
    );
    assert_eq!(token_balance(&listed2, &escrow_key()), LOT);

    // Облік перевикористаний — і він чистий: гук звів його на нулі балансу.
    let escrow = holder(&listed2, offer_key());
    assert_eq!(
        escrow.accrued, 0,
        "у перевикористаному обліку лишився хвіст"
    );
    assert_eq!(escrow.index_at_checkpoint, INDEX);
}
