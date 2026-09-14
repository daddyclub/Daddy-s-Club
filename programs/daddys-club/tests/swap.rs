//! `demo_issuer.swap` — референсна інтеграція `FR-004` на справжньому байткоді
//! обох програм.
//!
//! Тут доводиться те, чого не міг довести `tests/source.rs`: там перехоплення
//! кликали напряму, і `AUTHORITY` підписував себе сам, бо mollusk шанує
//! `is_signer` у метаданих. Підпис PDA чужої програми так не підробити — його
//! може поставити лише сама програма, зсередини CPI. Саме це тут і
//! відбувається.
//!
//! Друге, що доводиться, — `SC-001`: нуль додаткових транзакцій і нуль
//! зовнішніх виконавців. Кожен тест нижче — **одна** інструкція з **одним**
//! підписом трейдера, і всередині неї комісія виникає, ділиться й доходить до
//! ескроу.
//!
//! Рахунок джерела в усіх наборах починається **порожнім**. Це не економія:
//! якби своп кликав перехоплення до того, як покласти туди комісію, ядру не
//! було б чого списувати, і happy-path упав би на першому ж переказі.

#[path = "harness.rs"]
mod harness;

use {
    anchor_lang::InstructionData,
    daddys_club::{
        errors::ClubError,
        state::{Issue, IssueState, RevenueSource},
    },
    harness::*,
    mollusk_svm::result::{Check, InstructionResult},
    solana_account::Account,
    solana_address::Address as Pubkey,
    solana_instruction::{AccountMeta, Instruction},
    solana_program_error::ProgramError,
};

/// Той, хто платить комісію і ставить єдиний підпис у всьому шляху погашення.
const TRADER: Pubkey = Pubkey::new_from_array([41u8; 32]);

/// Другий бік свопу. Протоколу він не цікавий, але без нього це був би не
/// своп, а збір комісії.
const BASE_MINT: Pubkey = Pubkey::new_from_array([42u8; 32]);
const BASE_DECIMALS: u8 = 6;

const TRADER_USDC: Pubkey = Pubkey::new_from_array([43u8; 32]);
const TRADER_BASE: Pubkey = Pubkey::new_from_array([44u8; 32]);
const POOL_USDC: Pubkey = Pubkey::new_from_array([45u8; 32]);
const POOL_BASE: Pubkey = Pubkey::new_from_array([46u8; 32]);

/// Своп на 1 000 USDC. Числа підібрані так, щоб їх не довелось перераховувати:
/// 0.3% комісії = 3 USDC, 12% перехоплення від неї = 0.36 USDC.
const SWAP_IN: u64 = 1_000_000_000;
const FEE: u64 = 3_000_000;
const SHARE: u64 = 360_000;
const AMOUNT_OUT: u64 = SWAP_IN - FEE;

/// Пропозиція бонду дорівнює номіналу: `SCALE / face = 1e12 / 250e9 = 4`.
const INDEX_PER_UNIT: u128 = 4;

const TRADER_FUNDS: u64 = 5_000_000_000;
const RESERVE: u64 = 5_000_000_000;

/// Опис виклику. Негативні тести переписують одне поле й лишають решту
/// happy-path'ом — так у тесті видно рівно те, що відрізняється.
struct Call {
    trader: Pubkey,
    signs: bool,
    issue: Option<Pubkey>,
    amount_in: u64,
}

fn call() -> Call {
    Call {
        trader: TRADER,
        signs: true,
        issue: Some(demo_issue()),
        amount_in: SWAP_IN,
    }
}

fn swap_ix(c: Call) -> Instruction {
    // Порожній слот несе program id **демо-емітента**: опційні акаунти читає
    // та програма, у чий список вони подані, а список тут його. Ядро побачить
    // у своєму — власний id, і поставить його туди Anchor при CPI.
    let (issue, escrow, mint) = match c.issue {
        Some(issue) => (
            AccountMeta::new(issue, false),
            AccountMeta::new(ESCROW_VAULT, false),
            AccountMeta::new_readonly(BOND_MINT, false),
        ),
        None => {
            let empty = omitted(issuer_program_id()).0;
            (
                AccountMeta::new_readonly(empty, false),
                AccountMeta::new_readonly(empty, false),
                AccountMeta::new_readonly(empty, false),
            )
        }
    };

    Instruction::new_with_bytes(
        issuer_program_id(),
        &demo_issuer::instruction::Swap {
            amount_in: c.amount_in,
        }
        .data(),
        vec![
            AccountMeta::new_readonly(issuer_authority().0, false),
            AccountMeta::new_readonly(c.trader, c.signs),
            AccountMeta::new(TRADER_USDC, false),
            AccountMeta::new(TRADER_BASE, false),
            AccountMeta::new(POOL_USDC, false),
            AccountMeta::new(POOL_BASE, false),
            AccountMeta::new(SOURCE_VAULT, false),
            AccountMeta::new(source_pda(ISSUER, SOURCE_SEQ).0, false),
            issue,
            escrow,
            mint,
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(BASE_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
            AccountMeta::new_readonly(club_id(), false),
        ],
    )
}

/// Демо-світ: пул з резервами, трейдер із грошима, джерело з **порожнім**
/// рахунком і випуск, який це джерело забезпечує.
fn swap_accounts(source: RevenueSource, issue: Option<Issue>) -> Vec<(Pubkey, Account)> {
    let pool = issuer_authority().0;

    let mut accounts = vec![
        // PDA пулу даних не має: його роль — підпис, а не стан.
        (pool, uninitialized()),
        (TRADER, wallet()),
        (TRADER_USDC, token_account(USDC_MINT, TRADER, TRADER_FUNDS)),
        (TRADER_BASE, token_account(BASE_MINT, TRADER, 0)),
        (POOL_USDC, token_account(USDC_MINT, pool, 0)),
        (POOL_BASE, token_account(BASE_MINT, pool, RESERVE)),
        (SOURCE_VAULT, token_account(USDC_MINT, pool, 0)),
        (source_pda(ISSUER, SOURCE_SEQ).0, anchor_account(&source)),
        (ESCROW_VAULT, token_account(USDC_MINT, demo_issue(), 0)),
        (USDC_MINT, usdc_mint(1_000_000_000_000_000)),
        (BASE_MINT, plain_mint(BASE_DECIMALS, 1_000_000_000_000_000)),
        token_program(),
        // Обидві програми: ядро — ціль CPI, демо-емітент — порожній слот
        // опційної трійці.
        program_account(club_id()),
        program_account(issuer_program_id()),
    ];

    if let Some(issue) = issue {
        accounts.push((demo_issue(), anchor_account(&issue)));
        accounts.push((BOND_MINT, bond_mint(demo_issue(), issue.raised)));
    }

    accounts
}

fn swap(source: RevenueSource, issue: Option<Issue>, c: Call) -> InstructionResult {
    setup().process_and_validate_instruction(
        &swap_ix(c),
        &swap_accounts(source, issue),
        &[Check::success()],
    )
}

/// Прогін на демо-світі: джерело зайняте випуском, випуск у погашенні.
fn swap_into(issue: Issue, c: Call) -> InstructionResult {
    swap(stored_source(Some(demo_issue()), 0), Some(issue), c)
}

fn custom(error: ClubError) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(error)))
}

/// `FR-004` і `SC-001` разом: комісія виникає у свопі емітента й ділиться в тій
/// самій транзакції. Жодного другого виклику, жодного кранка — і жодного
/// підпису, крім трейдерового.
///
/// Тут же перевіряється порядок: рахунок джерела починався порожнім, отже все,
/// що дійшло до ескроу, поклав туди цей самий своп до виклику перехоплення.
#[test]
fn a_swap_pays_the_escrow_out_of_the_fee_it_has_just_taken() {
    let result = swap_into(repaying(0, 0), call());

    // Гроші трейдера: вхід пішов, вихід прийшов.
    assert_eq!(token_balance(&result, &TRADER_USDC), TRADER_FUNDS - SWAP_IN);
    assert_eq!(token_balance(&result, &TRADER_BASE), AMOUNT_OUT);
    assert_eq!(token_balance(&result, &POOL_USDC), AMOUNT_OUT);
    assert_eq!(token_balance(&result, &POOL_BASE), RESERVE - AMOUNT_OUT);

    // Комісія: частка — в ескроу, решта лишилась емітенту на рахунку джерела.
    assert_eq!(token_balance(&result, &ESCROW_VAULT), SHARE);
    assert_eq!(
        token_balance(&result, &SOURCE_VAULT),
        FEE - SHARE,
        "з комісії пішло не рівно стільки, скільки прийшло в ескроу"
    );

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.repaid_total, SHARE);
    assert_eq!(issue.payout_index, u128::from(SHARE) * INDEX_PER_UNIT);
    assert_eq!(issue.state, IssueState::Repaying);
}

/// `SC-001` літерою: у шляху погашення один підпис і нуль зовнішніх
/// виконавців. Перевіряється сам список акаунтів — те, що транзакція проходить,
/// ще не означає, що її не мусив би підписати хтось другий.
#[test]
fn the_whole_repayment_path_carries_one_signature_and_nobody_else() {
    let ix = swap_ix(call());

    let signers: Vec<Pubkey> = ix
        .accounts
        .iter()
        .filter(|meta| meta.is_signer)
        .map(|meta| meta.pubkey)
        .collect();

    assert_eq!(signers, vec![TRADER], "у шляху погашення зайвий підпис");
    assert!(
        !ix.accounts
            .iter()
            .any(|meta| meta.pubkey == issuer_authority().0 && meta.is_signer),
        "PDA емітента підписаний ззовні: тоді це доводило б не CPI, а метадані"
    );
}

/// Розщеплюється **комісія**, а не обсяг свопу. Найдорожча помилка інтегратора
/// — передати в перехоплення `amount_in`: гроші трейдера, які лише проходять
/// крізь пул, пішли б у погашення чужого боргу.
#[test]
fn the_split_is_measured_from_the_fee_not_from_the_trade() {
    let result = swap_into(repaying(0, 0), call());

    let source: RevenueSource = decode(&result, &source_pda(ISSUER, SOURCE_SEQ).0);

    assert_eq!(
        source.total_observed, FEE,
        "в історію джерела потрапив обсяг свопу, а не дохід емітента"
    );
    assert!(
        token_balance(&result, &ESCROW_VAULT) < FEE,
        "в ескроу пішло більше за всю комісію"
    );
}

/// `FR-028`: емітент підключає перехоплення до того, як з'явиться бонд, і
/// свопи з цієї миті вже пишуть історію. Саме нею потім міряється допуск
/// (`FR-007`), тому інтеграція мусить працювати без випуску.
#[test]
fn a_swap_without_an_issue_still_feeds_the_history() {
    let result = swap(
        stored_source(None, 0),
        None,
        Call {
            issue: None,
            ..call()
        },
    );

    let source: RevenueSource = decode(&result, &source_pda(ISSUER, SOURCE_SEQ).0);

    assert_eq!(source.total_observed, FEE);
    assert_eq!(
        token_balance(&result, &SOURCE_VAULT),
        FEE,
        "без випуску комісія має лишитись емітенту цілком"
    );
    assert_eq!(token_balance(&result, &TRADER_BASE), AMOUNT_OUT);
}

/// Округлення вниз доходить і сюди: комісія є, а частка від неї — ні. Своп
/// проходить, історія росте, у сховище не йде нічого. Гілки «якщо є що ділити»
/// у свопі немає навмисно — вона зробила б історію залежною від розміру угоди.
#[test]
fn a_fee_too_small_to_split_still_settles_the_swap() {
    // 1 000 × 0.3% = 3, з них 12% = 0.36 → 0.
    let result = swap_into(
        repaying(0, 0),
        Call {
            amount_in: 1_000,
            ..call()
        },
    );

    assert_eq!(token_balance(&result, &ESCROW_VAULT), 0);
    assert_eq!(token_balance(&result, &SOURCE_VAULT), 3);
    assert_eq!(token_balance(&result, &TRADER_BASE), 997);

    let source: RevenueSource = decode(&result, &source_pda(ISSUER, SOURCE_SEQ).0);
    assert_eq!(source.total_observed, 3, "дрібниця теж є доходом");

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.repaid_total, 0);
    assert_eq!(issue.payout_index, 0);
}

/// `FR-019` наскрізь усього шляху: своп, який закриває зобов'язання, і
/// наступний за ним. Другий проходить так, наче протоколу тут ніколи й не
/// було, — без відмови, без дії емітента й без окремої транзакції.
#[test]
fn the_swap_that_closes_the_obligation_lets_the_next_one_through() {
    let obligation = repaying(0, 0).obligation_total;
    let tail = 100_000;

    assert!(tail < SHARE, "хвіст мусить бути меншим за частку");

    let ix = swap_ix(call());
    let result = setup().process_and_validate_instruction_chain(
        &[(&ix, &[Check::success()]), (&ix, &[Check::success()])],
        &swap_accounts(
            stored_source(Some(demo_issue()), 0),
            Some(repaying(obligation - tail, 0)),
        ),
    );

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.repaid_total, obligation);
    assert_eq!(issue.state, IssueState::Repaid);
    assert_eq!(issue.payout_index, u128::from(tail) * INDEX_PER_UNIT);

    assert_eq!(
        token_balance(&result, &ESCROW_VAULT),
        tail,
        "другий своп теж щось відщепив"
    );
    assert_eq!(
        token_balance(&result, &SOURCE_VAULT),
        2 * FEE - tail,
        "після погашення комісія має лишатись емітенту цілком"
    );

    let source: RevenueSource = decode(&result, &source_pda(ISSUER, SOURCE_SEQ).0);
    assert_eq!(
        source.total_observed,
        2 * FEE,
        "історія бачить обидва свопи"
    );
}

/// `FR-004`: автентифікує підпис PDA, а не той факт, що виклик прийшов із
/// програми. Джерело, записане на чужий ключ, відмовляє — і відмова ядра валить
/// увесь своп, замість пустити комісію повз зобов'язання.
#[test]
fn a_source_that_answers_to_another_key_fails_the_whole_swap() {
    let foreign = RevenueSource {
        authority: anchor_key(OUTSIDER),
        ..stored_source(Some(demo_issue()), 0)
    };

    let result = setup().process_and_validate_instruction(
        &swap_ix(call()),
        &swap_accounts(foreign, Some(repaying(0, 0))),
        &[custom(ClubError::SourceAuthorityMismatch)],
    );

    assert_eq!(
        token_balance(&result, &TRADER_USDC),
        TRADER_FUNDS,
        "своп упав, а гроші трейдера все одно рухались"
    );
}

/// Емітент не може підсунути ядру чужий випуск: кільце «джерело ↔ випуск»
/// замкнене з обох боків, і своп проти незабезпеченого випуску не проходить
/// цілком.
#[test]
fn a_swap_against_an_issue_this_source_does_not_back_is_refused() {
    setup().process_and_validate_instruction(
        &swap_ix(call()),
        &swap_accounts(stored_source(None, 0), Some(repaying(0, 0))),
        &[custom(ClubError::SourceNotPledged)],
    );
}
