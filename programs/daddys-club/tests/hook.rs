//! `execute` (`FR-017`, `FR-038`) — облік при передачі бонду на справжньому
//! байткоді Token-2022 і ядра.
//!
//! Гук тут ніхто не кличе напряму. Кожен тест — це `transfer_checked`
//! Token-2022, і саме він, переставивши баланси, знаходить наш список у мінті,
//! добирає за ним чекпоінти обох сторін і робить CPI в `execute`. Тобто
//! доводиться не «інструкція рахує правильно», а що передача бонду **як така**
//! лишає по собі облік — і що зроблена повз наш застосунок вона не проходить
//! інакше.
//!
//! Головне, що тут перевіряється, — контракт із `claim`. Виплата рахує
//! претензію на **поточному** балансі, і це законно рівно доти, доки кожна
//! передача лишає по собі чекпоінт: між двома чекпоінтами баланс не
//! змінюється. Баланс, зрушений без чекпоінта, дав би отримувачеві забрати
//! те, що накопичилось до нього, — чужі гроші. Тест `the_buyer_cannot_claim_…`
//! показує саме цю атаку й те, що вона не проходить.
//!
//! Світ складається руками з тієї ж причини, що й у виплаті: щоб побачити, як
//! передача ділить накопичене, потрібен випуск, у якому вже щось перехоплено.
//! Числа ті самі, що в `tests/invest.rs`: 12 000 USDC у сховищі, 4% номіналу
//! на руках, 480 USDC накопичено — і саме ці 480 USDC мусять лишитись за тим,
//! хто тримав бонд, коли вони надходили.

#[path = "harness.rs"]
mod harness;

use {
    anchor_lang::InstructionData,
    anchor_spl::token_2022::spl_token_2022::{
        extension::{
            transfer_hook::TransferHookAccount, BaseStateWithExtensions, StateWithExtensions,
        },
        instruction::TokenInstruction,
        state::Account as HookTokenAccount,
    },
    daddys_club::{
        errors::ClubError,
        math,
        state::{HolderCheckpoint, Issue},
    },
    harness::*,
    mollusk_svm::result::{Check, InstructionResult},
    solana_account::Account,
    solana_address::Address as Pubkey,
    solana_instruction::{AccountMeta, Instruction},
    solana_program_error::ProgramError,
};

const INVESTOR_USDC: Pubkey = Pubkey::new_from_array([61u8; 32]);
const INVESTOR_BOND: Pubkey = Pubkey::new_from_array([62u8; 32]);
const BUYER_USDC: Pubkey = Pubkey::new_from_array([63u8; 32]);
const BUYER_BOND: Pubkey = Pubkey::new_from_array([64u8; 32]);
/// Другий рахунок того самого гаманця в тому самому мінті: у нього один облік
/// на обидва.
const INVESTOR_SPARE_BOND: Pubkey = Pubkey::new_from_array([65u8; 32]);
/// Рахунок бонду стороннього, для якого облік ніхто не відкривав.
const OUTSIDER_BOND: Pubkey = Pubkey::new_from_array([66u8; 32]);

/// Скільки вже перехоплено у сховище погашення — 12 000 USDC.
const PAID_IN: u64 = 12_000_000_000;
/// Індекс, який лишило по собі це перехоплення: `PAID_IN * SCALE / face`.
const INDEX: u128 = 48_000_000_000;
/// Баланс інвестора — 10 000 одиниць номіналу з 250 000 — і те, що йому з
/// `PAID_IN` належить: 4%.
const BALANCE: u64 = 10_000_000_000;
const CLAIM: u64 = 480_000_000;
/// Половина позиції — звичайний продаж, після якого бонд є в обох.
const HALF: u64 = BALANCE / 2;

fn custom(error: ClubError) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(error)))
}

fn anchor_err(code: anchor_lang::error::ErrorCode) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(code)))
}

/// Облік у тому вигляді, в якому його лишають `open_position` і попередні
/// виплати: чекпоінт там, де його востаннє зрушили.
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

/// Сторона переказу: рахунок і його власник. Власник потрібен окремо, бо саме
/// з нього дерівується чекпоінт, який Token-2022 шукатиме в наборі.
#[derive(Clone, Copy)]
struct Side {
    token: Pubkey,
    owner: Pubkey,
}

const INVESTOR_SIDE: Side = Side {
    token: INVESTOR_BOND,
    owner: INVESTOR,
};
const BUYER_SIDE: Side = Side {
    token: BUYER_BOND,
    owner: BUYER,
};

/// `transfer_checked` Token-2022 з тим набором, який зібрав би клієнт за
/// списком у мінті: чотири обов'язкові акаунти, далі список, випуск, два
/// чекпоінти і сама програма гука. Дані пакує крейт токен-програми — переписані
/// байти одного дня розійшлися б із тим, що читає Token-2022.
///
/// Порядок додаткових акаунтів Token-2022 не важить: він шукає їх за ключем.
/// Важить, що чекпоінти тут дерівуються **нашими** seeds, а Token-2022 звіряє
/// їх із тим, що резолвить список у мінті. Розійдуться — передача впаде на
/// `IncorrectAccount` ще до гука, і саме це доводить, що `open_position`
/// створює той акаунт, який гук шукає.
fn transfer_ix(from: Side, to: Side, amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        token_program().0,
        &TokenInstruction::TransferChecked {
            amount,
            decimals: BOND_DECIMALS,
        }
        .pack(),
        vec![
            AccountMeta::new(from.token, false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new(to.token, false),
            AccountMeta::new_readonly(from.owner, true),
            AccountMeta::new_readonly(extra_metas_pda(BOND_MINT).0, false),
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(holder_pda(demo_issue(), from.owner).0, false),
            AccountMeta::new(holder_pda(demo_issue(), to.owner).0, false),
            AccountMeta::new_readonly(club_id(), false),
        ],
    )
}

/// Той самий `claim`, що й у `tests/invest.rs`: після передачі кожна сторона
/// приходить по своє, і саме тут видно, кому що дісталось.
fn claim_ix(owner: Pubkey, usdc: Pubkey, bond: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Claim {}.data(),
        vec![
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(holder_pda(demo_issue(), owner).0, false),
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new(usdc, false),
            AccountMeta::new(ESCROW_VAULT, false),
            AccountMeta::new_readonly(bond, false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

/// Світ у погашенні: інвестор тримає бонд від самого початку (чекпоінт у
/// нулі), покупець відкрив облік тоді ж, але бонду ще не має. Усе перехоплене
/// лежить у сховищі, і жоден із них ще нічого не забирав.
///
/// Покупець із чекпоінтом у нулі — не випадковість, а умова атаки: якби
/// передача не рухала його чекпоінт, різниця індексу від нуля помножилась би
/// на щойно отриманий бонд.
fn world(investor: HolderCheckpoint, buyer: HolderCheckpoint) -> Vec<(Pubkey, Account)> {
    let issue = repaying(PAID_IN, INDEX);

    vec![
        (demo_issue(), anchor_account(&issue)),
        (
            holder_pda(demo_issue(), INVESTOR).0,
            anchor_account(&investor),
        ),
        (holder_pda(demo_issue(), BUYER).0, anchor_account(&buyer)),
        (holder_pda(demo_issue(), OUTSIDER).0, uninitialized()),
        (INVESTOR, wallet()),
        (BUYER, wallet()),
        (OUTSIDER, wallet()),
        (INVESTOR_USDC, usdc_account(INVESTOR, 0)),
        (INVESTOR_BOND, bond_account(INVESTOR, BALANCE)),
        (INVESTOR_SPARE_BOND, bond_account(INVESTOR, 0)),
        (BUYER_USDC, usdc_account(BUYER, 0)),
        (BUYER_BOND, bond_account(BUYER, 0)),
        (OUTSIDER_BOND, bond_account(OUTSIDER, 0)),
        (ESCROW_VAULT, usdc_account(demo_issue(), PAID_IN)),
        (BOND_MINT, bond_mint(demo_issue(), issue.raised)),
        (USDC_MINT, usdc_mint(1_000_000_000_000_000)),
        (extra_metas_pda(BOND_MINT).0, extra_metas_account()),
        token_program(),
        program_account(club_id()),
    ]
}

fn fresh_world() -> Vec<(Pubkey, Account)> {
    world(ledger(INVESTOR, 0), ledger(BUYER, 0))
}

fn holder(result: &InstructionResult, owner: Pubkey) -> HolderCheckpoint {
    decode(result, &holder_pda(demo_issue(), owner).0)
}

fn transferring(result: &InstructionResult, key: &Pubkey) -> bool {
    let stored = result.get_account(key).expect("рахунок є в результаті");
    let unpacked = StateWithExtensions::<HookTokenAccount>::unpack(&stored.data)
        .expect("рахунок розпаковується");
    let flag: &TransferHookAccount = unpacked.get_extension().expect("розширення на місці");

    bool::from(flag.transferring)
}

/// `FR-017` дослівно: обом сторонам нараховується станом на момент перед
/// передачею. Гук фізично спізнюється — баланси в ньому вже нові, — тому
/// «перед» відновлюється з `amount`: інвесторові нараховано на повний баланс,
/// покупцеві — на нуль, і чекпоінти обох стоять на сьогоднішньому індексі.
#[test]
fn a_transfer_settles_both_sides_as_of_the_moment_before_it() {
    let result = setup().process_and_validate_instruction(
        &transfer_ix(INVESTOR_SIDE, BUYER_SIDE, HALF),
        &fresh_world(),
        &[Check::success()],
    );

    assert_eq!(token_balance(&result, &INVESTOR_BOND), BALANCE - HALF);
    assert_eq!(token_balance(&result, &BUYER_BOND), HALF);

    let seller = holder(&result, INVESTOR);
    assert_eq!(
        seller.accrued, CLAIM,
        "продавцеві нараховано не на баланс до передачі"
    );
    assert_eq!(seller.index_at_checkpoint, INDEX);

    let buyer = holder(&result, BUYER);
    assert_eq!(
        buyer.accrued, 0,
        "покупцеві нараховано те, чого він не тримав"
    );
    assert_eq!(buyer.index_at_checkpoint, INDEX);

    // Індекс рухає перехоплення, а не передача: випуск лишається як був.
    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.payout_index, INDEX);
    assert_eq!(issue.repaid_total, PAID_IN);
}

/// Контракт із `claim`, половина продавця. Продавши **весь** бонд, він усе
/// одно забирає те, що накопичилось, поки бонд був у нього: нульовий баланс
/// претензії не скасовує, бо гук уже переніс її в `accrued`.
#[test]
fn what_accrued_before_the_sale_stays_with_the_seller() {
    let transfer = transfer_ix(INVESTOR_SIDE, BUYER_SIDE, BALANCE);
    let claim = claim_ix(INVESTOR, INVESTOR_USDC, INVESTOR_BOND);

    let result = setup().process_and_validate_instruction_chain(
        &[
            (&transfer, &[Check::success()]),
            (&claim, &[Check::success()]),
        ],
        &fresh_world(),
    );

    assert_eq!(token_balance(&result, &INVESTOR_BOND), 0);
    assert_eq!(token_balance(&result, &INVESTOR_USDC), CLAIM);
    assert_eq!(token_balance(&result, &ESCROW_VAULT), PAID_IN - CLAIM);

    let seller = holder(&result, INVESTOR);
    assert_eq!(seller.accrued, 0, "нараховане лишилось і забереться вдруге");
    assert_eq!(seller.claimed_total, CLAIM);
}

/// Контракт із `claim`, половина покупця — і сама атака. Його облік відкритий
/// із чекпоінтом у нулі, і після передачі на руках у нього весь бонд. Якби
/// передача не зрушила чекпоінт, виплата порахувала б різницю індексу від нуля
/// на цьому балансі — і віддала б йому рівно те, що заробив продавець.
#[test]
fn the_buyer_cannot_claim_what_accrued_before_the_bonds_were_theirs() {
    // Скільки взяв би покупець без чекпоінта: усе, що належить продавцеві.
    let without_checkpoint =
        math::claimable(INDEX, 0, u128::from(BALANCE), 0).expect("претензія рахується");
    assert_eq!(
        without_checkpoint,
        u128::from(CLAIM),
        "атака мусить бути на реальну суму"
    );

    let transfer = transfer_ix(INVESTOR_SIDE, BUYER_SIDE, BALANCE);
    let claim = claim_ix(BUYER, BUYER_USDC, BUYER_BOND);

    let result = setup().process_and_validate_instruction_chain(
        &[
            (&transfer, &[Check::success()]),
            (&claim, &[custom(ClubError::NothingToClaim)]),
        ],
        &fresh_world(),
    );

    assert_eq!(
        token_balance(&result, &BUYER_BOND),
        BALANCE,
        "бонд не дійшов"
    );
    assert_eq!(token_balance(&result, &BUYER_USDC), 0);
    assert_eq!(
        token_balance(&result, &ESCROW_VAULT),
        PAID_IN,
        "сховище схудло без виплати"
    );
}

/// Дві сторони разом ніколи не беруть більше, ніж обіцяв індекс: що
/// нараховано продавцеві плюс що нараховано покупцеві дорівнює тому, що
/// належало одному власникові до передачі. Це `SC-003` через зміну власника в
/// найменшому вигляді — на одній передачі; повний набір лишається `T039`.
#[test]
fn the_two_sides_together_are_owed_exactly_what_one_holder_was() {
    let result = setup().process_and_validate_instruction(
        &transfer_ix(INVESTOR_SIDE, BUYER_SIDE, HALF),
        &fresh_world(),
        &[Check::success()],
    );

    let seller = holder(&result, INVESTOR);
    let buyer = holder(&result, BUYER);
    let owed_now = |ledger: &HolderCheckpoint, balance: u64| {
        math::claimable(
            INDEX,
            ledger.index_at_checkpoint,
            u128::from(balance),
            u128::from(ledger.accrued),
        )
        .expect("претензія рахується")
    };

    assert_eq!(
        owed_now(&seller, BALANCE - HALF) + owed_now(&buyer, HALF),
        u128::from(CLAIM),
        "передача створила або загубила гроші"
    );
}

/// `FR-038`: передача на гаманець без відкритого обліку відхиляється цілком.
/// Список у мінті резолвить адресу чекпоінта, якого не існує, і гук упирається
/// в порожній акаунт — а разом із гуком падає й переказ: бонд лишається у
/// відправника, а не проходить повз облік.
#[test]
fn bonds_do_not_land_on_a_wallet_with_no_open_ledger() {
    let outsider = Side {
        token: OUTSIDER_BOND,
        owner: OUTSIDER,
    };

    let result = setup().process_and_validate_instruction(
        &transfer_ix(INVESTOR_SIDE, outsider, HALF),
        &fresh_world(),
        &[anchor_err(
            anchor_lang::error::ErrorCode::AccountNotInitialized,
        )],
    );

    assert_eq!(
        token_balance(&result, &INVESTOR_BOND),
        BALANCE,
        "бонд пішов повз облік"
    );
    assert_eq!(token_balance(&result, &OUTSIDER_BOND), 0);
}

/// Два рахунки одного гаманця в одному мінті мають **один** облік, і Token-2022
/// подає його гукові двічі — як чекпоінт відправника і як чекпоінт отримувача.
/// Нараховується один раз, на все, що гаманець тримав на обох рахунках; другий
/// прохід по тому самому акаунту не подвоює й не затирає перший.
#[test]
fn moving_bonds_between_two_accounts_of_one_wallet_settles_its_ledger_once() {
    let spare = Side {
        token: INVESTOR_SPARE_BOND,
        owner: INVESTOR,
    };

    let result = setup().process_and_validate_instruction(
        &transfer_ix(INVESTOR_SIDE, spare, HALF),
        &fresh_world(),
        &[Check::success()],
    );

    assert_eq!(token_balance(&result, &INVESTOR_BOND), BALANCE - HALF);
    assert_eq!(token_balance(&result, &INVESTOR_SPARE_BOND), HALF);

    let ledger = holder(&result, INVESTOR);
    assert_eq!(
        ledger.accrued, CLAIM,
        "один облік — одне нарахування на весь баланс"
    );
    assert_eq!(ledger.index_at_checkpoint, INDEX);
}

/// Чекпоінт із майбутнього — зіпсований облік, і гук відмовляє йому тим самим
/// ім'ям, що й виплата. Відмова гука валить переказ цілком: бонд не рухається.
#[test]
fn a_ledger_ahead_of_the_index_fails_the_whole_transfer() {
    let result = setup().process_and_validate_instruction(
        &transfer_ix(INVESTOR_SIDE, BUYER_SIDE, HALF),
        &world(ledger(INVESTOR, INDEX + 1), ledger(BUYER, 0)),
        &[custom(ClubError::CheckpointAheadOfIndex)],
    );

    assert_eq!(token_balance(&result, &INVESTOR_BOND), BALANCE);
}

/// Прапорець `transferring` живе рівно стільки, скільки переказ: Token-2022
/// піднімає його перед гуком і опускає одразу після. Після передачі обидва
/// рахунки знову «поза переказом» — інакше прямий виклик гука після першої ж
/// передачі виглядав би законним.
#[test]
fn the_transferring_flag_is_down_again_once_the_transfer_is_over() {
    let result = setup().process_and_validate_instruction(
        &transfer_ix(INVESTOR_SIDE, BUYER_SIDE, HALF),
        &fresh_world(),
        &[Check::success()],
    );

    assert!(!transferring(&result, &INVESTOR_BOND));
    assert!(!transferring(&result, &BUYER_BOND));
}
