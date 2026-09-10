//! `create_issue` (`FR-001`…`FR-006`, `FR-013`) на справжньому байткоді.
//!
//! Умови випуску фіксуються тут і більше не змінюються (`FR-002`), тому
//! перевіряється чотири речі: що записано у випуск, що зроблено з джерелом
//! (`FR-006`), яким вийшов інструмент бонду (`FR-013`) і що жодна умова поза
//! межами протоколу не проходить.
//!
//! Мінт, обидва сховища і список гука створюються тією ж інструкцією, тому в
//! наборі акаунтів вони приходять порожніми — рівно такими, якими їх бачить
//! ланцюг до виклику.
//!
//! Друга половина файлу — `withdraw_proceeds` (`FR-012`, `FR-034`). Там випуск
//! приходить уже створеним і вже зібраним: створення перевірене вище, і тягнути
//! його в кожен прогін означало б міряти дві інструкції одним тестом. Кожна
//! видача дивиться на чотири величини одразу — що лишилось у сховищі підписки,
//! скільки взяв протокол, скільки дійшло емітенту і в якому стані вийшов
//! випуск: зійшлися вони поодинці — ще не значить, що зійшлися між собою.

#[path = "harness.rs"]
mod harness;

use {
    anchor_lang::{solana_program::program_option::COption as HookCOption, InstructionData, Space},
    anchor_spl::token_2022::spl_token_2022::{
        extension::{transfer_hook::TransferHook, BaseStateWithExtensions, StateWithExtensions},
        state::{Account as HookTokenAccount, Mint as HookMint},
    },
    daddys_club::{
        errors::ClubError,
        instructions::issue::{hook_account_metas, IssueParams, EXTRA_ACCOUNT_METAS},
        state::{Issue, IssueState, ProtocolConfig, RevenueSource},
    },
    harness::*,
    mollusk_svm::result::Check,
    solana_account::Account,
    solana_address::Address as Pubkey,
    solana_instruction::{AccountMeta, Instruction},
    solana_program_error::ProgramError,
    spl_tlv_account_resolution::state::ExtraAccountMetaList,
    spl_transfer_hook_interface::instruction::ExecuteInstruction,
};

/// PDA програми-емітента з `register_source` — тут воно ні на що не впливає,
/// але джерело без нього не буває.
const AUTHORITY: Pubkey = Pubkey::new_from_array([31u8; 32]);
const SOURCE_VAULT: Pubkey = Pubkey::new_from_array([32u8; 32]);

/// Дохід, який джерело вже пропустило через себе до випуску. Ненульовий
/// навмисно: зріз під `FR-030` інакше не відрізнити від нуля за замовчуванням.
const OBSERVED: u64 = 4_200_000_000;

fn source_key(issuer: Pubkey) -> Pubkey {
    source_pda(issuer, SOURCE_SEQ).0
}

fn issue_key(issuer: Pubkey, seq: u64) -> Pubkey {
    issue_pda(source_key(issuer), seq).0
}

fn stored_source(issuer: Pubkey, active_issue: Option<Pubkey>) -> RevenueSource {
    RevenueSource {
        issuer: anchor_key(issuer),
        authority: anchor_key(AUTHORITY),
        vault: anchor_key(SOURCE_VAULT),
        first_seen_ts: NOW - 30 * DAY,
        total_observed: OBSERVED,
        observed_before_issue: 0,
        active_issue: active_issue.map(anchor_key),
        seq: SOURCE_SEQ,
        bump: source_pda(issuer, SOURCE_SEQ).1,
    }
}

/// Умови з картки випуску на M0: 250 000 USDC під 9.5% на 90 днів, 12%
/// перехоплення, лот 1 USDC.
fn terms() -> IssueParams {
    IssueParams {
        face: 250_000_000_000,
        coupon_bps: 950,
        pledge_bps: 1_200,
        maturity_ts: NOW + 90 * DAY,
        subscription_end_ts: NOW + 7 * DAY,
        min_lot: 1_000_000,
    }
}

fn create_ix(issuer: Pubkey, seq: u64, params: IssueParams) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::CreateIssue { seq, params }.data(),
        vec![
            AccountMeta::new_readonly(config_pda().0, false),
            AccountMeta::new(source_key(issuer), false),
            AccountMeta::new(issue_key(issuer, seq), false),
            AccountMeta::new(issuer, true),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new(BOND_MINT, true),
            AccountMeta::new(SUBSCRIPTION_VAULT, true),
            AccountMeta::new(ESCROW_VAULT, true),
            AccountMeta::new(extra_metas_pda(BOND_MINT).0, false),
            AccountMeta::new_readonly(token_program().0, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    )
}

fn create_accounts(issuer: Pubkey, seq: u64) -> Vec<(Pubkey, Account)> {
    vec![
        (config_pda().0, anchor_account(&stored_config())),
        (
            source_key(issuer),
            anchor_account(&stored_source(issuer, None)),
        ),
        (issue_key(issuer, seq), uninitialized()),
        (issuer, wallet()),
        (USDC_MINT, usdc_mint(0)),
        (BOND_MINT, uninitialized()),
        (SUBSCRIPTION_VAULT, uninitialized()),
        (ESCROW_VAULT, uninitialized()),
        (extra_metas_pda(BOND_MINT).0, uninitialized()),
        token_program(),
        system_program(),
    ]
}

fn custom(error: ClubError) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(error)))
}

fn anchor_err(code: anchor_lang::error::ErrorCode) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(code)))
}

/// Прогін на робочих умовах. Ним починається більшість перевірок нижче, бо
/// створення — одна транзакція, і дивитись на її наслідки треба з одного місця.
fn create(params: IssueParams) -> mollusk_svm::result::InstructionResult {
    setup().process_and_validate_instruction(
        &create_ix(ISSUER, ISSUE_SEQ, params),
        &create_accounts(ISSUER, ISSUE_SEQ),
        &[Check::success()],
    )
}

/// Умови, відхилені набором акаунтів або перевірками: жодного випуску, жодного
/// мінта, джерело лишається вільним.
fn refuse(params: IssueParams, expected: Check<'_>) {
    setup().process_and_validate_instruction(
        &create_ix(ISSUER, ISSUE_SEQ, params),
        &create_accounts(ISSUER, ISSUE_SEQ),
        &[expected],
    );
}

#[test]
fn create_issue_freezes_the_terms_the_issuer_asked_for() {
    let params = terms();
    let result = create(params);

    let issue: Issue = decode(&result, &issue_key(ISSUER, ISSUE_SEQ));

    assert_eq!(issue.source, anchor_key(source_key(ISSUER)));
    assert_eq!(issue.bond_mint, anchor_key(BOND_MINT));
    assert_eq!(issue.subscription_vault, anchor_key(SUBSCRIPTION_VAULT));
    assert_eq!(issue.escrow_vault, anchor_key(ESCROW_VAULT));
    assert_eq!(issue.face, params.face);
    assert_eq!(issue.coupon_bps, params.coupon_bps);
    assert_eq!(issue.pledge_bps, params.pledge_bps);
    assert_eq!(issue.maturity_ts, params.maturity_ts);
    assert_eq!(issue.subscription_end_ts, params.subscription_end_ts);
    assert_eq!(issue.min_lot, params.min_lot);
    assert_eq!(issue.seq, ISSUE_SEQ);
    assert_eq!(issue.bump, issue_pda(source_key(ISSUER), ISSUE_SEQ).1);
}

/// `FR-018`: зобов'язання рахується один раз, тут, і далі не залежить від того,
/// як швидко надходить revenue.
#[test]
fn a_fresh_issue_owes_face_plus_coupon_and_has_repaid_nothing() {
    let result = create(terms());

    let issue: Issue = decode(&result, &issue_key(ISSUER, ISSUE_SEQ));

    assert_eq!(issue.obligation_total, 273_750_000_000);
    assert_eq!(issue.raised, 0);
    assert_eq!(issue.repaid_total, 0);
    assert_eq!(issue.payout_index, 0);
    assert_eq!(issue.state, IssueState::Subscribing);
}

/// `FR-006`: джерело зайняте випуском, і саме цей `Some` закриє дорогу другому.
/// Заразом знімається зріз доходу під `FR-030` — потім відрізнити «до» від
/// «після» вже нічим.
#[test]
fn creating_an_issue_takes_the_source_and_marks_the_revenue_seen_so_far() {
    let result = create(terms());

    let source: RevenueSource = decode(&result, &source_key(ISSUER));

    assert_eq!(
        source.active_issue,
        Some(anchor_key(issue_key(ISSUER, ISSUE_SEQ)))
    );
    assert_eq!(source.observed_before_issue, OBSERVED);
    // Історія при цьому не переписується: вона й далі рахується від реєстрації.
    assert_eq!(source.first_seen_ts, NOW - 30 * DAY);
    assert_eq!(source.total_observed, OBSERVED);
}

#[test]
fn a_source_that_already_backs_an_issue_refuses_a_second_one() {
    let mollusk = setup();

    let taken = replacing(
        &create_accounts(ISSUER, 1),
        source_key(ISSUER),
        anchor_account(&stored_source(ISSUER, Some(issue_key(ISSUER, ISSUE_SEQ)))),
    );

    mollusk.process_and_validate_instruction(
        &create_ix(ISSUER, 1, terms()),
        &taken,
        &[custom(ClubError::SourceAlreadyPledged)],
    );
}

/// `FR-013`: мінт свій на кожен випуск, друкувати вміє лише програма, а гук на
/// ньому незмінний — саме тому `FR-017` діє й повз наш застосунок.
#[test]
fn the_bond_mint_is_born_empty_with_a_hook_nobody_can_move() {
    let result = create(terms());

    let stored = result.get_account(&BOND_MINT).expect("мінт є в результаті");
    let unpacked = StateWithExtensions::<HookMint>::unpack(&stored.data).expect("мінт");

    assert_eq!(stored.owner, token_program_id());
    assert_eq!(unpacked.base.supply, 0);
    assert_eq!(unpacked.base.decimals, 0);
    assert_eq!(
        unpacked.base.mint_authority,
        HookCOption::Some(anchor_key(issue_key(ISSUER, ISSUE_SEQ)))
    );
    // Заморожений бонд не передається, а `FR-017` обіцяє передачу з обліком, а
    // не заборону.
    assert_eq!(unpacked.base.freeze_authority, HookCOption::None);

    let hook: &TransferHook = unpacked.get_extension().expect("гук на місці");
    let program_id: Option<anchor_lang::prelude::Pubkey> = hook.program_id.into();
    let authority: Option<anchor_lang::prelude::Pubkey> = hook.authority.into();

    assert_eq!(program_id, Some(anchor_key(club_id())));
    // Без authority гука переставити його не може ніхто й ніколи — ані емітент,
    // ані адміністратор протоколу.
    assert_eq!(authority, None);
}

#[test]
fn both_vaults_are_empty_and_answer_only_to_the_issue() {
    let result = create(terms());

    for vault in [SUBSCRIPTION_VAULT, ESCROW_VAULT] {
        let stored = result.get_account(&vault).expect("сховище є в результаті");
        let unpacked = StateWithExtensions::<HookTokenAccount>::unpack(&stored.data)
            .expect("сховище розпаковується");

        assert_eq!(unpacked.base.amount, 0);
        assert_eq!(unpacked.base.mint, anchor_key(USDC_MINT));
        assert_eq!(
            unpacked.base.owner,
            anchor_key(issue_key(ISSUER, ISSUE_SEQ)),
            "підписати переказ зі сховища має вміти лише програма"
        );
    }
}

/// Список читає Token-2022, а не ми, тому доводиться саме те, що лежить у
/// байтах: збіг із тим, що будує програма, і власник, за яким гук узагалі
/// шукатиме цей акаунт.
#[test]
fn the_hook_list_on_chain_is_the_one_the_program_builds() {
    let result = create(terms());

    let stored = result
        .get_account(&extra_metas_pda(BOND_MINT).0)
        .expect("список є в результаті");

    let metas =
        hook_account_metas(&anchor_key(issue_key(ISSUER, ISSUE_SEQ))).expect("меты будуються");
    let mut expected = vec![0u8; ExtraAccountMetaList::size_of(EXTRA_ACCOUNT_METAS).unwrap()];
    ExtraAccountMetaList::init::<ExecuteInstruction>(&mut expected, &metas)
        .expect("список пишеться");

    assert_eq!(stored.owner, club_id());
    assert_eq!(stored.data, expected);
}

/// `FR-002`: єдина інструкція, що пише умови, — ця. Другий виклик на той самий
/// випуск упирається в уже створений акаунт, тому переписати умови нічим.
#[test]
fn create_issue_refuses_to_overwrite_an_issue_that_already_exists() {
    let mollusk = setup();
    let created = create(terms());

    let existing = create_accounts(ISSUER, ISSUE_SEQ);
    let existing = replacing(
        &existing,
        issue_key(ISSUER, ISSUE_SEQ),
        created
            .get_account(&issue_key(ISSUER, ISSUE_SEQ))
            .expect("випуск є в результаті")
            .clone(),
    );

    let result = mollusk.process_instruction(&create_ix(ISSUER, ISSUE_SEQ, terms()), &existing);

    assert!(
        !result.program_result.is_ok(),
        "умови випуску переписались: {:?}",
        result.program_result
    );
}

/// `FR-003`: діапазон строків — властивість продукту. Обидва краї включно.
#[test]
fn create_issue_refuses_a_term_outside_the_protocol_range() {
    let config = stored_config();

    for tenor in [config.min_tenor_secs - 1, config.max_tenor_secs + 1] {
        refuse(
            IssueParams {
                maturity_ts: NOW + tenor,
                ..terms()
            },
            custom(ClubError::TermOutOfRange),
        );
    }

    for tenor in [config.min_tenor_secs, config.max_tenor_secs] {
        setup().process_and_validate_instruction(
            &create_ix(
                ISSUER,
                ISSUE_SEQ,
                IssueParams {
                    maturity_ts: NOW + tenor,
                    subscription_end_ts: NOW + DAY,
                    ..terms()
                },
            ),
            &create_accounts(ISSUER, ISSUE_SEQ),
            &[Check::success()],
        );
    }
}

/// `FR-005`: стеля існує, щоб перехоплення не позбавляло емітента обігових
/// коштів. Рівно стеля ще проходить.
#[test]
fn create_issue_refuses_a_share_above_the_protocol_cap() {
    let cap = stored_config().max_pledge_bps;

    refuse(
        IssueParams {
            pledge_bps: cap + 1,
            ..terms()
        },
        custom(ClubError::PledgeAboveCap),
    );

    setup().process_and_validate_instruction(
        &create_ix(
            ISSUER,
            ISSUE_SEQ,
            IssueParams {
                pledge_bps: cap,
                ..terms()
            },
        ),
        &create_accounts(ISSUER, ISSUE_SEQ),
        &[Check::success()],
    );
}

/// `FR-001` разом із `FR-010`: номінал, який не розкладається на цілі лоти,
/// лишає хвіст, менший за лот, і рівність `raised == face` стає недосяжною.
#[test]
fn create_issue_refuses_a_face_that_does_not_split_into_whole_lots() {
    refuse(
        IssueParams {
            face: 250_000_000_001,
            ..terms()
        },
        custom(ClubError::FaceAmountInvalid),
    );

    refuse(
        IssueParams {
            min_lot: 0,
            ..terms()
        },
        custom(ClubError::LotSizeInvalid),
    );
}

#[test]
fn create_issue_refuses_a_window_that_closes_outside_the_life_of_the_issue() {
    // Вікно, що закінчилось до створення, не дає підписатись нікому; вікно після
    // погашення дало б підписатись у прострочений випуск.
    for subscription_end_ts in [NOW, NOW + 90 * DAY] {
        refuse(
            IssueParams {
                subscription_end_ts,
                ..terms()
            },
            custom(ClubError::SubscriptionWindowInvalid),
        );
    }
}

/// Джерело дерівається від підписанта, тому під чуже джерело випуск не створити:
/// набір не сходиться ще до тіла інструкції.
#[test]
fn create_issue_refuses_to_build_on_another_issuers_source() {
    let mollusk = setup();

    let mut instruction = create_ix(ISSUER, ISSUE_SEQ, terms());
    instruction.accounts[3] = AccountMeta::new(OUTSIDER, true);

    let mut accounts = create_accounts(ISSUER, ISSUE_SEQ);
    accounts[3] = (OUTSIDER, wallet());

    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintSeeds)],
    );
}

/// Валюта сховищ береться з конфігу, а не з переданого мінта: інакше випуск
/// зібрав би кошти в тому, чим протокол не вміє платити.
#[test]
fn create_issue_refuses_a_mint_that_is_not_the_protocol_currency() {
    let mollusk = setup();
    let other_mint = Pubkey::new_from_array([99u8; 32]);

    let mut instruction = create_ix(ISSUER, ISSUE_SEQ, terms());
    instruction.accounts[4] = AccountMeta::new_readonly(other_mint, false);

    let mut accounts = create_accounts(ISSUER, ISSUE_SEQ);
    accounts[4] = (other_mint, usdc_mint(0));

    mollusk.process_and_validate_instruction(
        &instruction,
        &accounts,
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintAddress)],
    );
}

#[test]
fn create_issue_refuses_to_run_before_the_protocol_exists() {
    let mollusk = setup();

    mollusk.process_and_validate_instruction(
        &create_ix(ISSUER, ISSUE_SEQ, terms()),
        &replacing(
            &create_accounts(ISSUER, ISSUE_SEQ),
            config_pda().0,
            uninitialized(),
        ),
        &[anchor_err(
            anchor_lang::error::ErrorCode::AccountNotInitialized,
        )],
    );
}

/// Випуск виділяється рівно під `INIT_SPACE`: інша довжина ламає декодери SDK,
/// які цю рівність і перевіряють.
#[test]
fn the_issue_account_is_exactly_the_size_the_clients_expect() {
    let result = create(terms());

    let stored = result
        .get_account(&issue_key(ISSUER, ISSUE_SEQ))
        .expect("випуск є в результаті");

    assert_eq!(stored.data.len(), 8 + Issue::INIT_SPACE);
    assert_eq!(stored.owner, club_id());
}

// ---- withdraw_proceeds (`FR-012`, `FR-034`) ---------------------------------

/// USDC-рахунок емітента і чужа скарбниця. Ключі довільні: дерівацією вони не
/// задані — це звичайні токен-акаунти.
const ISSUER_USDC: Pubkey = Pubkey::new_from_array([43u8; 32]);
const OUTSIDE_VAULT: Pubkey = Pubkey::new_from_array([44u8; 32]);

/// Комісія на номіналі картки M0: 250 000 USDC × 1.5% демо-протоколу.
const FEE: u64 = 3_750_000_000;

/// Зібраний випуск: рівність `raised == face`, якою `subscribe` і ставить
/// `Funded` (`FR-010`). Сховище підписки в наборі акаунтів наповнюється з
/// `raised`, тому світ лишається узгодженим сам із собою.
fn funded() -> Issue {
    let issue = stored_issue(IssueState::Funded, 0);

    Issue {
        raised: issue.face,
        ..issue
    }
}

fn withdraw_ix() -> Instruction {
    withdraw_ix_with(ISSUER, SUBSCRIPTION_VAULT, FEE_VAULT, ISSUER_USDC)
}

/// Той самий виклик із підміненим підписантом, сховищем, скарбницею або
/// рахунком емітента: у цих випадках змінюється не вміст акаунта, а те, який
/// акаунт подали, — і `replacing` тут не допомагає.
fn withdraw_ix_with(
    issuer: Pubkey,
    vault: Pubkey,
    fee_vault: Pubkey,
    issuer_usdc: Pubkey,
) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::WithdrawProceeds {}.data(),
        vec![
            AccountMeta::new_readonly(config_pda().0, false),
            AccountMeta::new(demo_issue(), false),
            // Джерело прибите до випуску через `has_one`, тому воно те саме
            // навіть тоді, коли підписує хтось інший.
            AccountMeta::new_readonly(source_key(ISSUER), false),
            AccountMeta::new_readonly(issuer, true),
            AccountMeta::new(issuer_usdc, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(fee_vault, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

/// Світ на момент видачі: у сховищі підписки лежить рівно зібране, рахунок
/// емітента порожній, скарбниця протоколу порожня. Порожні навмисно — так
/// видно, що прийшло саме звідси.
fn withdraw_accounts(issue: Issue) -> Vec<(Pubkey, Account)> {
    vec![
        (config_pda().0, anchor_account(&stored_config())),
        (demo_issue(), anchor_account(&issue)),
        (
            source_key(ISSUER),
            anchor_account(&stored_source(ISSUER, Some(demo_issue()))),
        ),
        (ISSUER, wallet()),
        (ISSUER_USDC, usdc_account(ISSUER, 0)),
        (SUBSCRIPTION_VAULT, usdc_account(demo_issue(), issue.raised)),
        (FEE_VAULT, usdc_account(ADMIN, 0)),
        (USDC_MINT, usdc_mint(1_000_000_000_000_000)),
        token_program(),
    ]
}

fn withdraw(issue: Issue) -> mollusk_svm::result::InstructionResult {
    setup().process_and_validate_instruction(
        &withdraw_ix(),
        &withdraw_accounts(issue),
        &[Check::success()],
    )
}

fn refuse_withdrawal(issue: Issue, expected: Check<'_>) {
    setup().process_and_validate_instruction(
        &withdraw_ix(),
        &withdraw_accounts(issue),
        &[expected],
    );
}

/// `FR-012` разом із `FR-034`: емітент отримує номінал за вирахуванням
/// комісії. Сховище підписки при цьому спорожняється повністю — усе зібране
/// пішло, і жодна одиниця не лишилась ні за ким.
#[test]
fn withdraw_proceeds_hands_the_issuer_the_face_less_the_fee() {
    let face = funded().face;
    let result = withdraw(funded());

    assert_eq!(token_balance(&result, &SUBSCRIPTION_VAULT), 0);
    assert_eq!(token_balance(&result, &FEE_VAULT), FEE);
    assert_eq!(token_balance(&result, &ISSUER_USDC), face - FEE);
    // Комісія і виплата разом — це рівно номінал: грошей не з'явилось і не
    // зникло, вони лише розійшлись на дві адреси.
    assert_eq!(
        token_balance(&result, &FEE_VAULT) + token_balance(&result, &ISSUER_USDC),
        face
    );
}

/// Видача — це мить, коли зобов'язання виникає, тому випуск виходить звідси в
/// `Repaying`: `FR-011` каже, що в недозібраного випуску перехоплення **не**
/// вмикається, а тут воно й вмикається (`FR-014`). Умови при цьому не
/// рухаються — їх не редагує жодна інструкція (`FR-002`).
#[test]
fn the_payout_leaves_the_issue_in_repayment_and_owing_the_same() {
    let before = funded();
    let result = withdraw(funded());

    let issue: Issue = decode(&result, &demo_issue());

    assert_eq!(issue.state, IssueState::Repaying);
    assert_eq!(issue.raised, before.raised);
    assert_eq!(issue.face, before.face);
    assert_eq!(issue.obligation_total, before.obligation_total);
    // Погашення ще не починалось: комісія протоколу — не виплата власникам.
    assert_eq!(issue.repaid_total, 0);
    assert_eq!(issue.payout_index, 0);
}

/// `FR-034`: ставка — параметр протоколу, а не умова випуску. Саме тому конфіг
/// є в наборі акаунтів: у випуску ставки немає й бути не може.
#[test]
fn the_fee_follows_the_rate_the_protocol_holds_at_the_moment_of_the_payout() {
    let face = funded().face;
    let raised_rate = ProtocolConfig {
        origination_fee_bps: 200,
        ..stored_config()
    };

    let result = setup().process_and_validate_instruction(
        &withdraw_ix(),
        &replacing(
            &withdraw_accounts(funded()),
            config_pda().0,
            anchor_account(&raised_rate),
        ),
        &[Check::success()],
    );

    assert_eq!(token_balance(&result, &FEE_VAULT), 5_000_000_000);
    assert_eq!(token_balance(&result, &ISSUER_USDC), face - 5_000_000_000);
}

/// Окремого прапорця «видано» у випуску немає, тому двері зачиняє сам стан:
/// випуск, який уже пішов у погашення, віддати номінал удруге не може.
#[test]
fn the_proceeds_can_be_taken_only_once() {
    let taken = withdraw(funded());

    let already = replacing(
        &withdraw_accounts(funded()),
        demo_issue(),
        taken.get_account(&demo_issue()).expect("випуск").clone(),
    );

    setup().process_and_validate_instruction(
        &withdraw_ix(),
        &already,
        &[custom(ClubError::ProceedsAlreadyWithdrawn)],
    );

    // Прострочення й повне погашення — теж «після видачі»: гроші зі сховища
    // підписки пішли ще раніше.
    for state in [IssueState::PastDue, IssueState::Repaid] {
        refuse_withdrawal(
            Issue { state, ..funded() },
            custom(ClubError::ProceedsAlreadyWithdrawn),
        );
    }
}

/// `FR-012`: до повного збору кошти лежать в ескроу і належать інвесторам.
/// Недозібраний випуск не дає емітенту нічого й після закриття вікна — звідти
/// шлях лише в повернення (`FR-011`).
#[test]
fn an_issue_that_was_not_funded_pays_the_issuer_nothing() {
    for state in [IssueState::Subscribing, IssueState::Failed] {
        let issue = stored_issue(state, 0);

        refuse_withdrawal(
            Issue {
                raised: issue.face,
                ..issue
            },
            custom(ClubError::IssueNotFunded),
        );
    }
}

/// Другий замок на `FR-010`: `Funded` ставить `subscribe` рівно на рівності
/// `raised == face`, і саме вона дає право видавати номінал. Стан складається
/// руками, бо через саму програму в таку розбіжність не потрапити — без замка
/// емітент забрав би більше, ніж інвестори внесли.
#[test]
fn a_funded_issue_that_did_not_actually_raise_the_face_pays_out_nothing() {
    let short = funded();

    refuse_withdrawal(
        Issue {
            raised: short.face - short.min_lot,
            ..short
        },
        custom(ClubError::IssueNotFunded),
    );
}

/// У випуску два сховища на одній валюті й одній authority. Ескроу погашення
/// тримає гроші власників бондів, і виглядає воно цілком «своїм» — тому
/// підміна ловиться `has_one`, а не оком.
#[test]
fn the_repayment_escrow_is_not_a_vault_this_payout_may_touch() {
    let mut accounts = withdraw_accounts(funded());
    accounts[5] = (ESCROW_VAULT, usdc_account(demo_issue(), funded().face));

    setup().process_and_validate_instruction(
        &withdraw_ix_with(ISSUER, ESCROW_VAULT, FEE_VAULT, ISSUER_USDC),
        &accounts,
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintHasOne)],
    );
}

/// `FR-004`: емітента випуск не знає — його знає джерело. Чужий підпис не
/// доходить до тіла інструкції взагалі.
#[test]
fn only_the_issuer_behind_the_source_may_take_the_proceeds() {
    let mut accounts = withdraw_accounts(funded());
    accounts[3] = (OUTSIDER, wallet());
    accounts[4] = (ISSUER_USDC, usdc_account(OUTSIDER, 0));

    setup().process_and_validate_instruction(
        &withdraw_ix_with(OUTSIDER, SUBSCRIPTION_VAULT, FEE_VAULT, ISSUER_USDC),
        &accounts,
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintHasOne)],
    );
}

/// `FR-034`: комісія йде туди, куди показує протокол, а не туди, куди показав
/// викликач. Скарбницю прибиває `has_one` на конфігу.
#[test]
fn the_fee_goes_only_to_the_treasury_the_protocol_names() {
    let mut accounts = withdraw_accounts(funded());
    accounts[6] = (OUTSIDE_VAULT, usdc_account(OUTSIDER, 0));

    setup().process_and_validate_instruction(
        &withdraw_ix_with(ISSUER, SUBSCRIPTION_VAULT, OUTSIDE_VAULT, ISSUER_USDC),
        &accounts,
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintHasOne)],
    );
}

/// Скарбниця в чужому мінті прийняла б переказ, якого протокол не вміє
/// витрачати. Валюта звірена ще в `init_protocol`, але звіряється й тут — з
/// тим самим мінтом, яким рахується переказ.
#[test]
fn a_treasury_in_another_currency_takes_no_fee() {
    setup().process_and_validate_instruction(
        &withdraw_ix(),
        &replacing(
            &withdraw_accounts(funded()),
            FEE_VAULT,
            bond_account(ADMIN, 0),
        ),
        &[anchor_err(
            anchor_lang::error::ErrorCode::ConstraintTokenMint,
        )],
    );
}

/// `FR-012`: номінал отримує саме емітент. Підпис дає право забрати гроші, а
/// не право відправити їх кому завгодно.
#[test]
fn the_proceeds_go_only_to_an_account_the_issuer_controls() {
    setup().process_and_validate_instruction(
        &withdraw_ix(),
        &replacing(
            &withdraw_accounts(funded()),
            ISSUER_USDC,
            usdc_account(OUTSIDER, 0),
        ),
        &[anchor_err(
            anchor_lang::error::ErrorCode::ConstraintTokenOwner,
        )],
    );
}
