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

#[path = "harness.rs"]
mod harness;

use {
    anchor_lang::{
        solana_program::program_option::COption as HookCOption, InstructionData, Space,
    },
    anchor_spl::token_2022::spl_token_2022::{
        extension::{transfer_hook::TransferHook, BaseStateWithExtensions, StateWithExtensions},
        state::{Account as HookTokenAccount, Mint as HookMint},
    },
    daddys_club::{
        errors::ClubError,
        instructions::issue::{hook_account_metas, IssueParams, EXTRA_ACCOUNT_METAS},
        state::{Issue, IssueState, RevenueSource},
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

/// Мінт бонду і два сховища — звичайні акаунти, які емітент підписує при
/// створенні. Ключі довільні саме тому, що дерівацією не задані: знайти їх
/// можна лише з самого `Issue`.
const SUBSCRIPTION_VAULT: Pubkey = Pubkey::new_from_array([33u8; 32]);
const ESCROW_VAULT: Pubkey = Pubkey::new_from_array([34u8; 32]);

const SOURCE_SEQ: u64 = 0;
const SEQ: u64 = 0;

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
        (source_key(issuer), anchor_account(&stored_source(issuer, None))),
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
        &create_ix(ISSUER, SEQ, params),
        &create_accounts(ISSUER, SEQ),
        &[Check::success()],
    )
}

/// Умови, відхилені набором акаунтів або перевірками: жодного випуску, жодного
/// мінта, джерело лишається вільним.
fn refuse(params: IssueParams, expected: Check<'_>) {
    setup().process_and_validate_instruction(
        &create_ix(ISSUER, SEQ, params),
        &create_accounts(ISSUER, SEQ),
        &[expected],
    );
}

#[test]
fn create_issue_freezes_the_terms_the_issuer_asked_for() {
    let params = terms();
    let result = create(params);

    let issue: Issue = decode(&result, &issue_key(ISSUER, SEQ));

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
    assert_eq!(issue.seq, SEQ);
    assert_eq!(issue.bump, issue_pda(source_key(ISSUER), SEQ).1);
}

/// `FR-018`: зобов'язання рахується один раз, тут, і далі не залежить від того,
/// як швидко надходить revenue.
#[test]
fn a_fresh_issue_owes_face_plus_coupon_and_has_repaid_nothing() {
    let result = create(terms());

    let issue: Issue = decode(&result, &issue_key(ISSUER, SEQ));

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

    assert_eq!(source.active_issue, Some(anchor_key(issue_key(ISSUER, SEQ))));
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
        anchor_account(&stored_source(ISSUER, Some(issue_key(ISSUER, SEQ)))),
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
        HookCOption::Some(anchor_key(issue_key(ISSUER, SEQ)))
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
            anchor_key(issue_key(ISSUER, SEQ)),
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

    let metas = hook_account_metas(&anchor_key(issue_key(ISSUER, SEQ))).expect("меты будуються");
    let mut expected = vec![0u8; ExtraAccountMetaList::size_of(EXTRA_ACCOUNT_METAS).unwrap()];
    ExtraAccountMetaList::init::<ExecuteInstruction>(&mut expected, &metas).expect("список пишеться");

    assert_eq!(stored.owner, club_id());
    assert_eq!(stored.data, expected);
}

/// `FR-002`: єдина інструкція, що пише умови, — ця. Другий виклик на той самий
/// випуск упирається в уже створений акаунт, тому переписати умови нічим.
#[test]
fn create_issue_refuses_to_overwrite_an_issue_that_already_exists() {
    let mollusk = setup();
    let created = create(terms());

    let existing = create_accounts(ISSUER, SEQ);
    let existing = replacing(
        &existing,
        issue_key(ISSUER, SEQ),
        created
            .get_account(&issue_key(ISSUER, SEQ))
            .expect("випуск є в результаті")
            .clone(),
    );

    let result = mollusk.process_instruction(&create_ix(ISSUER, SEQ, terms()), &existing);

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
                SEQ,
                IssueParams {
                    maturity_ts: NOW + tenor,
                    subscription_end_ts: NOW + DAY,
                    ..terms()
                },
            ),
            &create_accounts(ISSUER, SEQ),
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
            SEQ,
            IssueParams {
                pledge_bps: cap,
                ..terms()
            },
        ),
        &create_accounts(ISSUER, SEQ),
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

    let mut instruction = create_ix(ISSUER, SEQ, terms());
    instruction.accounts[3] = AccountMeta::new(OUTSIDER, true);

    let mut accounts = create_accounts(ISSUER, SEQ);
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

    let mut instruction = create_ix(ISSUER, SEQ, terms());
    instruction.accounts[4] = AccountMeta::new_readonly(other_mint, false);

    let mut accounts = create_accounts(ISSUER, SEQ);
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
        &create_ix(ISSUER, SEQ, terms()),
        &replacing(&create_accounts(ISSUER, SEQ), config_pda().0, uninitialized()),
        &[anchor_err(anchor_lang::error::ErrorCode::AccountNotInitialized)],
    );
}

/// Випуск виділяється рівно під `INIT_SPACE`: інша довжина ламає декодери SDK,
/// які цю рівність і перевіряють.
#[test]
fn the_issue_account_is_exactly_the_size_the_clients_expect() {
    let result = create(terms());

    let stored = result
        .get_account(&issue_key(ISSUER, SEQ))
        .expect("випуск є в результаті");

    assert_eq!(stored.data.len(), 8 + Issue::INIT_SPACE);
    assert_eq!(stored.owner, club_id());
}
