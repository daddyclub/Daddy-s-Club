//! `register_source` (`FR-004`, `FR-028`) і `intercept` (`FR-014`, `FR-019`,
//! `FR-020`) на справжньому байткоді.
//!
//! Реєстрація — це момент, з якого джерело починає накопичувати історію
//! (`FR-028`), і єдине місце, де записується, чий підпис потім прийматиме
//! перехоплення (`FR-004`). Тому перевіряється три речі: що записано, що
//! історія починається зараз, і що чужа namespace недосяжна.
//!
//! Друга половина міряє потік. Перехоплення вбудоване в чужу інструкцію, тому
//! половина тестів тут — про те, що воно **не** відмовляє: випуску немає, ще не
//! видано, вже погашено — усе це проходить успішно й лишає гроші емітенту.
//! Відмовляють лише права й неузгоджений набір акаунтів, і кожна така відмова
//! має тут свій тест.
//!
//! The last part is past due (`FR-022`). Here time matters for the first
//! time, so those tests set the chain clock themselves and stand on both sides
//! of the maturity second.

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

const SEQ: u64 = 0;

fn register_ix(issuer: Pubkey, seq: u64) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::RegisterSource { seq }.data(),
        vec![
            AccountMeta::new_readonly(config_pda().0, false),
            AccountMeta::new(source_pda(issuer, seq).0, false),
            AccountMeta::new(issuer, true),
            AccountMeta::new_readonly(issuer_authority().0, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(SOURCE_VAULT, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    )
}

fn register_accounts(issuer: Pubkey, seq: u64) -> Vec<(Pubkey, Account)> {
    vec![
        (config_pda().0, anchor_account(&stored_config())),
        (source_pda(issuer, seq).0, uninitialized()),
        (issuer, wallet()),
        (issuer_authority().0, wallet()),
        (USDC_MINT, usdc_mint(0)),
        (SOURCE_VAULT, usdc_account(issuer_authority().0, 0)),
        system_program(),
    ]
}

fn anchor_err(code: anchor_lang::error::ErrorCode) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(code)))
}

#[test]
fn register_source_records_who_may_deliver_revenue_and_where_it_lands() {
    let mollusk = setup();

    let result = mollusk.process_and_validate_instruction(
        &register_ix(ISSUER, SEQ),
        &register_accounts(ISSUER, SEQ),
        &[Check::success()],
    );

    let source: RevenueSource = decode(&result, &source_pda(ISSUER, SEQ).0);

    assert_eq!(source.issuer, anchor_key(ISSUER));
    assert_eq!(source.authority, anchor_key(issuer_authority().0));
    assert_eq!(source.vault, anchor_key(SOURCE_VAULT));
    assert_eq!(source.seq, SEQ);
    assert_eq!(source.bump, source_pda(ISSUER, SEQ).1);
}

/// `FR-028`: історія починається з підключення перехоплення, а не зі створення
/// випуску. Якби відлік стартував із випуску, поріг допуску (`FR-007`) не
/// відрізняв би джерело з доходом від джерела, зареєстрованого хвилину тому.
#[test]
fn the_history_of_a_fresh_source_starts_now_and_is_empty() {
    let mollusk = setup();

    let result = mollusk.process_and_validate_instruction(
        &register_ix(ISSUER, SEQ),
        &register_accounts(ISSUER, SEQ),
        &[Check::success()],
    );

    let source: RevenueSource = decode(&result, &source_pda(ISSUER, SEQ).0);

    assert_eq!(source.first_seen_ts, NOW);
    assert_eq!(source.total_observed, 0);
    assert_eq!(source.observed_before_issue, 0);
    // `FR-006`: свіже джерело нічим не зайняте, і саме цей `None` дозволить
    // створити під нього перший випуск.
    assert_eq!(source.active_issue, None);
}

#[test]
fn register_source_refuses_to_register_the_same_source_twice() {
    let mollusk = setup();

    let taken = replacing(
        &register_accounts(ISSUER, SEQ),
        source_pda(ISSUER, SEQ).0,
        anchor_account(&RevenueSource {
            issuer: anchor_key(ISSUER),
            authority: anchor_key(issuer_authority().0),
            vault: anchor_key(SOURCE_VAULT),
            first_seen_ts: NOW - 30 * DAY,
            total_observed: 500_000,
            observed_before_issue: 0,
            active_issue: None,
            seq: SEQ,
            bump: source_pda(ISSUER, SEQ).1,
        }),
    );

    // Повторна реєстрація обнулила б `first_seen_ts` і `total_observed` — тобто
    // стерла б історію, під яку джерело вже допускається до випуску (`FR-007`).
    let result = mollusk.process_instruction(&register_ix(ISSUER, SEQ), &taken);

    assert!(
        !result.program_result.is_ok(),
        "джерело зареєструвалось удруге: {:?}",
        result.program_result
    );
}

#[test]
fn one_issuer_may_register_more_than_one_source() {
    let mollusk = setup();

    let first = source_pda(ISSUER, 0).0;
    let second = source_pda(ISSUER, 1).0;
    assert_ne!(first, second);

    let result = mollusk.process_and_validate_instruction(
        &register_ix(ISSUER, 1),
        &register_accounts(ISSUER, 1),
        &[Check::success()],
    );

    let source: RevenueSource = decode(&result, &second);
    assert_eq!(source.seq, 1);
}

/// Джерело дерівається від підписанта, тому чужа namespace недосяжна: набір
/// акаунтів на `ISSUER`, підпис `OUTSIDER` — і seeds не сходяться.
#[test]
fn register_source_refuses_to_claim_another_issuers_namespace() {
    let mollusk = setup();

    let mut instruction = register_ix(ISSUER, SEQ);
    instruction.accounts[2] = AccountMeta::new(OUTSIDER, true);

    let mut accounts = register_accounts(ISSUER, SEQ);
    accounts[2] = (OUTSIDER, wallet());

    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintSeeds)],
    );
}

#[test]
fn register_source_refuses_a_vault_in_another_currency() {
    let mollusk = setup();
    let other_mint = Pubkey::new_from_array([99u8; 32]);

    let mut foreign = usdc_account(issuer_authority().0, 0);
    foreign.data[..32].copy_from_slice(other_mint.as_ref());

    mollusk.process_and_validate_instruction(
        &register_ix(ISSUER, SEQ),
        &replacing(&register_accounts(ISSUER, SEQ), SOURCE_VAULT, foreign),
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintTokenMint)],
    );
}

/// Валюта береться з конфігу, а не з переданого мінта: інакше пара «чужий мінт
/// + рахунок у ньому» пройшла б обидві перевірки й завела джерело не в тій
/// валюті, в якій протокол уміє платити.
#[test]
fn register_source_refuses_a_mint_that_is_not_the_protocol_currency() {
    let mollusk = setup();
    let other_mint = Pubkey::new_from_array([99u8; 32]);

    let mut instruction = register_ix(ISSUER, SEQ);
    instruction.accounts[4] = AccountMeta::new_readonly(other_mint, false);

    let mut foreign_vault = usdc_account(issuer_authority().0, 0);
    foreign_vault.data[..32].copy_from_slice(other_mint.as_ref());

    let mut accounts = replacing(&register_accounts(ISSUER, SEQ), SOURCE_VAULT, foreign_vault);
    accounts[4] = (other_mint, usdc_mint(0));

    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintAddress)],
    );
}

#[test]
fn register_source_refuses_to_run_before_the_protocol_exists() {
    let mollusk = setup();

    mollusk.process_and_validate_instruction(
        &register_ix(ISSUER, SEQ),
        &replacing(&register_accounts(ISSUER, SEQ), config_pda().0, uninitialized()),
        &[anchor_err(anchor_lang::error::ErrorCode::AccountNotInitialized)],
    );
}

// ---- intercept (`FR-014`, `FR-019`, `FR-020`) -------------------------------

/// Надходження середнього розміру. Число не випадкове: на ньому стоїть фікстура
/// `pledged_share(1_070_000_000, 1_200) = 128_400_000` у `math.rs`, тож частку
/// в тестах нижче не треба перераховувати вдруге.
const INFLOW: u64 = 1_070_000_000;
const SHARE: u64 = 128_400_000;

/// Скільки лежить на рахунку джерела до розщеплення. З запасом: тести міряють,
/// скільки пішло, а не скільки могло піти.
const VAULT_BALANCE: u64 = 10_000_000_000;

/// Пропозиція бонду дорівнює номіналу, тому одиниця виплати рахується просто:
/// `SCALE / face = 1e12 / 250e9 = 4`. Індекс рухається на `сума × 4`.
const INDEX_PER_UNIT: u128 = 4;

/// Опис виклику. Негативні тести переписують одне поле й лишають решту
/// happy-path'ом — так у тесті видно рівно те, що відрізняється.
struct Call {
    authority: Pubkey,
    signs: bool,
    issue: Option<Pubkey>,
    vault: Pubkey,
    escrow: Pubkey,
    amount: u64,
}

fn call() -> Call {
    Call {
        authority: issuer_authority().0,
        signs: true,
        issue: Some(demo_issue()),
        vault: SOURCE_VAULT,
        escrow: ESCROW_VAULT,
        amount: INFLOW,
    }
}

fn intercept_ix(c: Call) -> Instruction {
    let (issue, escrow, mint) = match c.issue {
        Some(issue) => (issue, c.escrow, BOND_MINT),
        // Опційні акаунти подаються трійцею: випуску немає — немає ані його
        // сховища, ані його мінта.
        None => (
            omitted(club_id()).0,
            omitted(club_id()).0,
            omitted(club_id()).0,
        ),
    };

    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Intercept { amount: c.amount }.data(),
        vec![
            AccountMeta::new(source_pda(ISSUER, SOURCE_SEQ).0, false),
            AccountMeta::new_readonly(c.authority, c.signs),
            AccountMeta::new(c.vault, false),
            AccountMeta::new(issue, false),
            AccountMeta::new(escrow, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

/// Світ демо: джерело з історією, випуск у погашенні, гроші на рахунку джерела.
/// Сховище підписки лежить поруч навмисно — саме його найлегше подати замість
/// ескроу погашення.
fn intercept_accounts(source: RevenueSource, issue: Option<Issue>) -> Vec<(Pubkey, Account)> {
    let mut accounts = vec![
        (source_pda(ISSUER, SOURCE_SEQ).0, anchor_account(&source)),
        (issuer_authority().0, wallet()),
        (OUTSIDER, wallet()),
        (
            SOURCE_VAULT,
            usdc_account(issuer_authority().0, VAULT_BALANCE),
        ),
        (ESCROW_VAULT, usdc_account(demo_issue(), 0)),
        (SUBSCRIPTION_VAULT, usdc_account(demo_issue(), 0)),
        (USDC_MINT, usdc_mint(1_000_000_000_000_000)),
        token_program(),
        omitted(club_id()),
    ];

    match issue {
        Some(issue) => {
            accounts.push((demo_issue(), anchor_account(&issue)));
            accounts.push((BOND_MINT, bond_mint(demo_issue(), issue.raised)));
        }
        None => accounts.push((demo_issue(), uninitialized())),
    }

    accounts
}

fn intercept(source: RevenueSource, issue: Option<Issue>, c: Call) -> InstructionResult {
    setup().process_and_validate_instruction(
        &intercept_ix(c),
        &intercept_accounts(source, issue),
        &[Check::success()],
    )
}

/// Прогін на демо-світі: джерело зайняте випуском, випуск у погашенні.
fn intercept_into(issue: Issue, c: Call) -> InstructionResult {
    intercept(stored_source(Some(demo_issue()), 0), Some(issue), c)
}

fn custom(error: ClubError) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(error)))
}

/// `FR-014`: розщеплення відбувається в тій самій транзакції, що й надходження.
/// Чотири величини перевіряються разом — те, що вони зійшлися поодинці, ще не
/// означає, що вони зійшлися між собою.
#[test]
fn intercept_splits_the_inflow_where_it_arises() {
    let result = intercept_into(repaying(0, 0), call());

    assert_eq!(token_balance(&result, &ESCROW_VAULT), SHARE);
    assert_eq!(
        token_balance(&result, &SOURCE_VAULT),
        VAULT_BALANCE - SHARE,
        "з рахунку джерела пішло не рівно стільки, скільки прийшло в ескроу"
    );

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.repaid_total, SHARE);
    assert_eq!(issue.payout_index, u128::from(SHARE) * INDEX_PER_UNIT);
    assert_eq!(
        issue.state,
        IssueState::Repaying,
        "випуск закрився, не виплативши зобов'язання"
    );
}

/// `FR-015`: надходження рухає індекс, а не робить переказ кожному власникові.
/// Тому в наборі акаунтів немає жодного обліку — і саме це робить вартість
/// обробки незалежною від кількості власників (`SC-005`).
#[test]
fn the_payout_index_moves_instead_of_paying_every_holder() {
    let already = 4_000_000_000;
    let result = intercept_into(repaying(0, already), call());

    let issue: Issue = decode(&result, &demo_issue());

    assert_eq!(
        issue.payout_index,
        already + u128::from(SHARE) * INDEX_PER_UNIT,
        "індекс не зрушився від того місця, де стояв"
    );
    assert!(
        result
            .get_account(&holder_pda(demo_issue(), INVESTOR).0)
            .is_none(),
        "у розщепленні бере участь облік власника"
    );
}

/// `FR-028`: історія накопичується незалежно від того, чи існує під джерелом
/// випуск. Саме нею потім міряється допуск (`FR-007`), тому перший внесок в
/// історію відбувається задовго до того, як з'явиться перший бонд.
#[test]
fn the_history_grows_even_when_no_issue_exists_yet() {
    let before = 3_000_000_000;
    let result = intercept(
        stored_source(None, before),
        None,
        Call {
            issue: None,
            ..call()
        },
    );

    let source: RevenueSource = decode(&result, &source_pda(ISSUER, SOURCE_SEQ).0);

    assert_eq!(source.total_observed, before + INFLOW);
    assert_eq!(
        token_balance(&result, &SOURCE_VAULT),
        VAULT_BALANCE,
        "без випуску з рахунку джерела не має піти нічого"
    );
}

/// Історія росте і тоді, коли випуск є: спостереження — окрема від
/// розщеплення дія, і `FR-030` порівнює саме темпи, а не суми в ескроу.
#[test]
fn the_history_counts_the_whole_inflow_not_the_intercepted_share() {
    let result = intercept(
        stored_source(Some(demo_issue()), 0),
        Some(repaying(0, 0)),
        call(),
    );

    let source: RevenueSource = decode(&result, &source_pda(ISSUER, SOURCE_SEQ).0);

    assert_eq!(
        source.total_observed, INFLOW,
        "в історію потрапила частка, а не надходження"
    );
}

/// Перехоплення — фільтр, а не ворота. Випуск, зобов'язання по якому ще не
/// виникло, пропускає потік цілим: `FR-012` провів межу видачею, а відмова тут
/// завалила б чужий своп, тобто не пропустила б комісію ані нам, ані емітенту.
#[test]
fn an_issue_that_is_not_in_repayment_lets_the_whole_flow_through() {
    for state in [
        IssueState::Subscribing,
        IssueState::Funded,
        IssueState::Failed,
    ] {
        let issue = Issue {
            raised: stored_issue(state, 0).face,
            ..stored_issue(state, 0)
        };

        let result = intercept_into(issue, call());

        assert_eq!(
            token_balance(&result, &SOURCE_VAULT),
            VAULT_BALANCE,
            "{state:?} розщепив потік"
        );
        assert_eq!(token_balance(&result, &ESCROW_VAULT), 0, "{state:?}");

        let issue: Issue = decode(&result, &demo_issue());
        assert_eq!(issue.repaid_total, 0, "{state:?}");
        assert_eq!(issue.payout_index, 0, "{state:?}");

        let source: RevenueSource = decode(&result, &source_pda(ISSUER, SOURCE_SEQ).0);
        assert_eq!(source.total_observed, INFLOW, "{state:?} не побачив доходу");
    }
}

/// `FR-020`: надходження понад залишок зараховується рівно в розмірі залишку, а
/// надлишок лишається емітенту. `FR-019`: у ту саму мить перехоплення
/// припиняється — без окремої дії з боку емітента.
#[test]
fn the_last_inflow_takes_only_what_is_left_and_closes_the_obligation() {
    let obligation = repaying(0, 0).obligation_total;
    let tail = 50_000_000;

    assert!(tail < SHARE, "хвіст мусить бути меншим за частку");

    let result = intercept_into(repaying(obligation - tail, 0), call());

    assert_eq!(token_balance(&result, &ESCROW_VAULT), tail);
    assert_eq!(
        token_balance(&result, &SOURCE_VAULT),
        VAULT_BALANCE - tail,
        "надлишок понад залишок зобов'язання все одно списався"
    );

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.repaid_total, obligation);
    assert_eq!(issue.state, IssueState::Repaid);
    assert_eq!(issue.payout_index, u128::from(tail) * INDEX_PER_UNIT);
}

/// `FR-019`: наступні комісії йдуть емітенту повністю. Перехоплення не
/// відмовляє — воно просто більше нічого не бере, і своп емітента працює далі
/// так, наче протоколу тут ніколи й не було.
#[test]
fn a_repaid_issue_lets_every_later_fee_through_untouched() {
    let obligation = repaying(0, 0).obligation_total;
    let closed = Issue {
        repaid_total: obligation,
        state: IssueState::Repaid,
        ..repaying(obligation, 7_000_000_000)
    };

    let result = intercept_into(closed, call());

    assert_eq!(token_balance(&result, &SOURCE_VAULT), VAULT_BALANCE);
    assert_eq!(token_balance(&result, &ESCROW_VAULT), 0);

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.repaid_total, obligation);
    assert_eq!(issue.payout_index, 7_000_000_000, "індекс зрушився після");
    assert_eq!(issue.state, IssueState::Repaid);
}

/// Округлення частки — вниз, і відкинутий залишок лишається емітенту, а не
/// створюється з повітря. Дрібне надходження проходить успішно й нульовим
/// переказом: окремої гілки на нуль немає навмисно.
#[test]
fn a_dust_inflow_rounds_the_share_down_to_nothing() {
    // 7 × 12% = 0.84 → 0. Та сама фікстура, що в `math.rs`.
    let result = intercept_into(
        repaying(0, 0),
        Call {
            amount: 7,
            ..call()
        },
    );

    assert_eq!(token_balance(&result, &SOURCE_VAULT), VAULT_BALANCE);
    assert_eq!(token_balance(&result, &ESCROW_VAULT), 0);

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.repaid_total, 0);
    assert_eq!(issue.payout_index, 0);

    let source: RevenueSource = decode(&result, &source_pda(ISSUER, SOURCE_SEQ).0);
    assert_eq!(source.total_observed, 7, "дрібниця теж є доходом");
}

/// `FR-004`: дохід приймається лише від того, хто **підписав** записаний ключ.
/// Перевіряються обидві половини: чужий підпис і правильний ключ без підпису.
#[test]
fn revenue_is_accepted_only_from_the_key_that_signed() {
    setup().process_and_validate_instruction(
        &intercept_ix(Call {
            authority: OUTSIDER,
            ..call()
        }),
        &intercept_accounts(stored_source(Some(demo_issue()), 0), Some(repaying(0, 0))),
        &[custom(ClubError::SourceAuthorityMismatch)],
    );

    setup().process_and_validate_instruction(
        &intercept_ix(Call {
            signs: false,
            ..call()
        }),
        &intercept_accounts(stored_source(Some(demo_issue()), 0), Some(repaying(0, 0))),
        &[anchor_err(anchor_lang::error::ErrorCode::AccountNotSigner)],
    );
}

/// Випуск, якого джерело не забезпечує, у набір не сходиться: `active_issue`
/// веде від джерела до випуску, і подати чужий або вже закритий випуск не
/// вийде. Без цього замка дохід одного протоколу гасив би борг іншого.
#[test]
fn an_issue_this_source_does_not_back_is_refused() {
    for active in [None, Some(issue_pda(demo_source(), ISSUE_SEQ + 1).0)] {
        setup().process_and_validate_instruction(
            &intercept_ix(call()),
            &intercept_accounts(stored_source(active, 0), Some(repaying(0, 0))),
            &[custom(ClubError::SourceNotPledged)],
        );
    }
}

/// У випуску два сховища на одній валюті й одній authority. Частка йде рівно в
/// те, яке названо у випуску, — сховище підписки тут виглядає цілком «своїм», і
/// саме на ньому помилитись найлегше.
#[test]
fn the_share_goes_to_the_escrow_this_issue_names() {
    setup().process_and_validate_instruction(
        &intercept_ix(Call {
            escrow: SUBSCRIPTION_VAULT,
            ..call()
        }),
        &intercept_accounts(stored_source(Some(demo_issue()), 0), Some(repaying(0, 0))),
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintHasOne)],
    );
}

/// Частка списується з того рахунку, який джерело зареєструвало, — інакше
/// емітент розщеплював би чужі гроші, а свої лишав собі.
#[test]
fn the_share_is_taken_from_the_vault_the_source_registered() {
    let stray = SUBSCRIPTION_VAULT;

    setup().process_and_validate_instruction(
        &intercept_ix(Call {
            vault: stray,
            ..call()
        }),
        &intercept_accounts(stored_source(Some(demo_issue()), 0), Some(repaying(0, 0))),
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintHasOne)],
    );
}

/// Програма рухає лише ті гроші, якими розпоряджається сам викликач. Джерело,
/// чий рахунок належить комусь іншому, впирається в іменоване обмеження, а не в
/// безіменну відмову токен-програми на першому ж розщепленні.
#[test]
fn the_vault_must_answer_to_the_key_that_signed() {
    let accounts = replacing(
        &intercept_accounts(stored_source(Some(demo_issue()), 0), Some(repaying(0, 0))),
        SOURCE_VAULT,
        usdc_account(OUTSIDER, VAULT_BALANCE),
    );

    setup().process_and_validate_instruction(
        &intercept_ix(call()),
        &accounts,
        &[anchor_err(
            anchor_lang::error::ErrorCode::ConstraintTokenOwner,
        )],
    );
}

// ---- Past due (`FR-022`) ---------------------------------------------------

/// `INFLOW` at the demo protocol cap of 30%: `1_070_000_000 × 3_000 / 10_000`.
const SHARE_AT_CAP: u64 = 321_000_000;

fn intercept_at(now: i64, issue: Issue) -> InstructionResult {
    let mut mollusk = setup();
    mollusk.sysvars.clock.unix_timestamp = now;

    mollusk.process_and_validate_instruction(
        &intercept_ix(call()),
        &intercept_accounts(stored_source(Some(demo_issue()), 0), Some(issue)),
        &[Check::success()],
    )
}

/// The share the issue was created with stays the cap the protocol had then.
#[test]
fn the_demo_issue_carries_the_cap_it_was_created_under() {
    assert_eq!(
        repaying(0, 0).past_due_pledge_bps,
        stored_config().max_pledge_bps
    );
    assert_eq!(
        u128::from(SHARE_AT_CAP),
        daddys_club::math::pledged_share(u128::from(INFLOW), stored_config().max_pledge_bps)
            .expect("the share fits")
    );
}

/// One second before the date the debt is still on time: the agreed share,
/// and the state stays.
#[test]
fn an_inflow_a_second_before_maturity_is_split_at_the_agreed_share() {
    let issue = repaying(0, 0);
    let result = intercept_at(issue.maturity_ts - 1, issue);

    assert_eq!(token_balance(&result, &ESCROW_VAULT), SHARE);

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.state, IssueState::Repaying);
}

/// On the date itself an open obligation is already overdue. Nobody marks it:
/// the first fee after the date writes the state and is split at the cap in
/// the same call, so the higher share starts with the first money there is.
#[test]
fn the_first_inflow_on_the_maturity_date_makes_the_issue_past_due() {
    let issue = repaying(0, 0);
    let result = intercept_at(issue.maturity_ts, issue);

    assert_eq!(token_balance(&result, &ESCROW_VAULT), SHARE_AT_CAP);
    assert_eq!(
        token_balance(&result, &SOURCE_VAULT),
        VAULT_BALANCE - SHARE_AT_CAP
    );

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.state, IssueState::PastDue);
    assert_eq!(issue.repaid_total, SHARE_AT_CAP);
    assert_eq!(
        issue.payout_index,
        u128::from(SHARE_AT_CAP) * INDEX_PER_UNIT
    );
}

/// Past due speeds repayment up and does not make the debt dearer: the
/// obligation is the one fixed at creation, to the unit.
#[test]
fn going_past_due_does_not_grow_the_obligation() {
    let issue = repaying(0, 0);
    let owed = issue.obligation_total;
    let result = intercept_at(issue.maturity_ts, issue);

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(
        issue.state,
        IssueState::PastDue,
        "the issue never went past due"
    );
    assert_eq!(issue.obligation_total, owed);
}

/// The share is the one frozen into the issue, not whatever the protocol cap
/// is today (`FR-036`): the config is not even in the set of accounts. An
/// issue frozen at 20% is split at 20%.
#[test]
fn a_past_due_issue_is_split_at_its_own_frozen_cap() {
    let frozen = 2_000;
    let issue = Issue {
        state: IssueState::PastDue,
        past_due_pledge_bps: frozen,
        ..repaying(0, 0)
    };
    let result = intercept_at(issue.maturity_ts + 30 * DAY, issue);

    assert_eq!(
        u128::from(token_balance(&result, &ESCROW_VAULT)),
        daddys_club::math::pledged_share(u128::from(INFLOW), frozen).expect("the share fits")
    );

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.state, IssueState::PastDue);
}

/// There is no way back to the agreed share: a past-due issue keeps the cap
/// for every later fee until it is repaid.
#[test]
fn a_past_due_issue_keeps_the_cap_for_every_later_fee() {
    let issue = Issue {
        state: IssueState::PastDue,
        ..repaying(SHARE_AT_CAP, u128::from(SHARE_AT_CAP) * INDEX_PER_UNIT)
    };
    let result = intercept_at(issue.maturity_ts + DAY, issue);

    assert_eq!(token_balance(&result, &ESCROW_VAULT), SHARE_AT_CAP);

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.state, IssueState::PastDue);
    assert_eq!(issue.repaid_total, 2 * SHARE_AT_CAP);
}

/// At the cap the last inflow still takes only what is left (`FR-020`) and
/// closes the obligation (`FR-019`): past due ends the only way it can.
#[test]
fn a_past_due_issue_is_closed_by_the_inflow_that_covers_the_rest() {
    let obligation = repaying(0, 0).obligation_total;
    let tail = 50_000_000;

    let issue = Issue {
        state: IssueState::PastDue,
        ..repaying(obligation - tail, 0)
    };
    let result = intercept_at(issue.maturity_ts + DAY, issue);

    assert_eq!(token_balance(&result, &ESCROW_VAULT), tail);

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.repaid_total, obligation);
    assert_eq!(issue.state, IssueState::Repaid);
}

/// Only an obligation can be overdue. Before the payout there is none
/// (`FR-012`), after full repayment there is none left, and an
/// undersubscribed issue never gets one — the date changes none of these,
/// and the flow goes through whole.
#[test]
fn an_issue_without_an_open_obligation_is_never_past_due() {
    for state in [
        IssueState::Subscribing,
        IssueState::Funded,
        IssueState::Repaid,
        IssueState::Failed,
    ] {
        let issue = Issue {
            raised: stored_issue(state, 0).face,
            ..stored_issue(state, 0)
        };
        let result = intercept_at(issue.maturity_ts + DAY, issue);

        assert_eq!(
            token_balance(&result, &SOURCE_VAULT),
            VAULT_BALANCE,
            "{state:?} split the flow after the date"
        );

        let issue: Issue = decode(&result, &demo_issue());
        assert_eq!(issue.state, state, "{state:?} changed its state");
    }
}
