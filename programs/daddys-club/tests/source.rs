//! `register_source` (`FR-004`, `FR-028`) на справжньому байткоді.
//!
//! Реєстрація — це момент, з якого джерело починає накопичувати історію
//! (`FR-028`), і єдине місце, де записується, чий підпис потім прийматиме
//! перехоплення (`FR-004`). Тому перевіряється три речі: що записано, що
//! історія починається зараз, і що чужа namespace недосяжна.

#[path = "harness.rs"]
mod harness;

use {
    anchor_lang::InstructionData,
    daddys_club::state::{ProtocolConfig, RevenueSource},
    harness::*,
    mollusk_svm::result::Check,
    solana_account::Account,
    solana_address::Address as Pubkey,
    solana_instruction::{AccountMeta, Instruction},
    solana_program_error::ProgramError,
};

/// PDA програми-емітента, чий підпис прийматиме `intercept` (`FR-004`).
/// Справжня деривація з'явиться разом із demo-емітентом; тут важливо лише, що
/// це не гаманець, який реєструє джерело.
const AUTHORITY: Pubkey = Pubkey::new_from_array([31u8; 32]);
const SOURCE_VAULT: Pubkey = Pubkey::new_from_array([32u8; 32]);

const SEQ: u64 = 0;

/// Конфіг, який на цій інструкції читається рівно заради `usdc_mint`: решта
/// параметрів працює на створенні випуску, не на реєстрації джерела.
fn stored_config() -> ProtocolConfig {
    ProtocolConfig {
        admin: anchor_key(ADMIN),
        origination_fee_bps: 150,
        trading_fee_bps: 50,
        max_pledge_bps: 3_000,
        min_tenor_secs: 30 * DAY,
        max_tenor_secs: 180 * DAY,
        history_threshold_secs: 7 * DAY,
        usdc_mint: anchor_key(USDC_MINT),
        fee_vault: anchor_key(FEE_VAULT),
        bump: config_pda().1,
    }
}

fn register_ix(issuer: Pubkey, seq: u64) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::RegisterSource { seq }.data(),
        vec![
            AccountMeta::new_readonly(config_pda().0, false),
            AccountMeta::new(source_pda(issuer, seq).0, false),
            AccountMeta::new(issuer, true),
            AccountMeta::new_readonly(AUTHORITY, false),
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
        (AUTHORITY, wallet()),
        (USDC_MINT, usdc_mint(0)),
        (SOURCE_VAULT, usdc_account(AUTHORITY, 0)),
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
    assert_eq!(source.authority, anchor_key(AUTHORITY));
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
            authority: anchor_key(AUTHORITY),
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

    let mut foreign = usdc_account(AUTHORITY, 0);
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

    let mut foreign_vault = usdc_account(AUTHORITY, 0);
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
