//! `init_protocol` і `update_config` (`FR-036`) на справжньому байткоді.
//!
//! Перевіряється три речі: параметри лягають туди, куди мають; набір, який не
//! можна створити, не можна й виставити зміною; і зміна не дотягується до вже
//! створеного випуску.

#[path = "harness.rs"]
mod harness;

use {
    anchor_lang::InstructionData,
    daddys_club::{
        errors::ClubError,
        instructions::protocol::ConfigParams,
        state::{Issue, IssueState, ProtocolConfig},
    },
    harness::*,
    mollusk_svm::result::Check,
    solana_account::Account,
    solana_address::Address as Pubkey,
    solana_instruction::{AccountMeta, Instruction},
    solana_program_error::ProgramError,
};

fn valid_params() -> ConfigParams {
    demo_config_params()
}

/// Той самий конфіг, що й у решти тестів, але на довільному адміністраторі:
/// `update_config` тільки й перевіряє, чий підпис прийшов.
fn config_of(admin: Pubkey) -> ProtocolConfig {
    ProtocolConfig {
        admin: anchor_key(admin),
        ..stored_config()
    }
}

fn init_ix(params: ConfigParams) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::InitProtocol { params }.data(),
        vec![
            AccountMeta::new(config_pda().0, false),
            AccountMeta::new(ADMIN, true),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(FEE_VAULT, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    )
}

fn init_accounts() -> Vec<(Pubkey, Account)> {
    vec![
        (config_pda().0, uninitialized()),
        (ADMIN, wallet()),
        (USDC_MINT, usdc_mint(0)),
        (FEE_VAULT, usdc_account(ADMIN, 0)),
        system_program(),
    ]
}

fn update_ix(admin: Pubkey, params: ConfigParams) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::UpdateConfig { params }.data(),
        vec![
            AccountMeta::new(config_pda().0, false),
            AccountMeta::new_readonly(admin, true),
        ],
    )
}

fn update_accounts(admin: Pubkey) -> Vec<(Pubkey, Account)> {
    vec![
        (config_pda().0, anchor_account(&config_of(ADMIN))),
        (admin, wallet()),
    ]
}

fn custom(error: ClubError) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(error)))
}

#[test]
fn init_protocol_writes_the_parameters_the_admin_asked_for() {
    let mollusk = setup();
    let params = valid_params();

    let result = mollusk.process_and_validate_instruction(
        &init_ix(params),
        &init_accounts(),
        &[Check::success()],
    );

    let config: ProtocolConfig = decode(&result, &config_pda().0);

    assert_eq!(config.admin, anchor_key(ADMIN));
    assert_eq!(config.usdc_mint, anchor_key(USDC_MINT));
    assert_eq!(config.fee_vault, anchor_key(FEE_VAULT));
    assert_eq!(config.origination_fee_bps, params.origination_fee_bps);
    assert_eq!(config.trading_fee_bps, params.trading_fee_bps);
    assert_eq!(config.max_pledge_bps, params.max_pledge_bps);
    assert_eq!(config.min_tenor_secs, params.min_tenor_secs);
    assert_eq!(config.max_tenor_secs, params.max_tenor_secs);
    assert_eq!(config.history_threshold_secs, params.history_threshold_secs);

    // Збережений bump — це той, який потім вимагають seeds у кожній наступній
    // інструкції: розійшовшись із канонічним, він закрив би доступ до конфігу.
    assert_eq!(config.bump, config_pda().1);
}

/// `FR-036` називає протокол singleton'ом. Доводить це не текст інструкції, а
/// те, що PDA один: другий виклик приходить на вже створений акаунт.
#[test]
fn init_protocol_refuses_to_create_the_config_twice() {
    let mollusk = setup();

    let occupied = replacing(
        &init_accounts(),
        config_pda().0,
        anchor_account(&config_of(ADMIN)),
    );

    let result = mollusk.process_instruction(&init_ix(valid_params()), &occupied);

    assert!(
        !result.program_result.is_ok(),
        "конфіг створився вдруге: {:?}",
        result.program_result
    );
}

#[test]
fn init_protocol_refuses_an_origination_fee_outside_one_to_two_percent() {
    let mollusk = setup();

    for fee in [0, 99, 201, 10_000] {
        mollusk.process_and_validate_instruction(
            &init_ix(ConfigParams {
                origination_fee_bps: fee,
                ..valid_params()
            }),
            &init_accounts(),
            &[custom(ClubError::FeeOutOfRange)],
        );
    }
}

/// `FR-035` діапазону торговій комісії не задає, тому єдина межа тут
/// арифметична: комісія понад усю ціну не лишає продавцю чого віддати.
#[test]
fn init_protocol_refuses_a_trading_fee_above_the_whole_price() {
    let mollusk = setup();

    mollusk.process_and_validate_instruction(
        &init_ix(ConfigParams {
            trading_fee_bps: 10_001,
            ..valid_params()
        }),
        &init_accounts(),
        &[custom(ClubError::FeeOutOfRange)],
    );

    // Рівно 100% ще проходить: межа саме там, де арифметика ламається, а не
    // там, де ставка виглядає дивною.
    mollusk.process_and_validate_instruction(
        &init_ix(ConfigParams {
            trading_fee_bps: 10_000,
            ..valid_params()
        }),
        &init_accounts(),
        &[Check::success()],
    );
}

#[test]
fn init_protocol_refuses_a_pledge_cap_that_is_not_a_cap() {
    let mollusk = setup();

    // Нуль забороняє будь-який випуск, понад 100% — перестає бути стелею
    // (`FR-005`).
    for cap in [0, 10_001] {
        mollusk.process_and_validate_instruction(
            &init_ix(ConfigParams {
                max_pledge_bps: cap,
                ..valid_params()
            }),
            &init_accounts(),
            &[custom(ClubError::PledgeCapOutOfRange)],
        );
    }
}

#[test]
fn init_protocol_refuses_a_term_range_outside_thirty_to_a_hundred_and_eighty_days() {
    let mollusk = setup();

    let outside = [
        // Коротший край, ніж дозволяє продукт.
        ConfigParams {
            min_tenor_secs: 29 * DAY,
            ..valid_params()
        },
        // Довший край.
        ConfigParams {
            max_tenor_secs: 181 * DAY,
            ..valid_params()
        },
        // Порожній діапазон: краї всередині дозволеного, але вивернуті.
        ConfigParams {
            min_tenor_secs: 90 * DAY,
            max_tenor_secs: 60 * DAY,
            ..valid_params()
        },
    ];

    for params in outside {
        mollusk.process_and_validate_instruction(
            &init_ix(params),
            &init_accounts(),
            &[custom(ClubError::TermRangeOutOfBounds)],
        );
    }
}

#[test]
fn init_protocol_refuses_a_history_threshold_that_admits_everyone() {
    let mollusk = setup();

    for threshold in [0, -1] {
        mollusk.process_and_validate_instruction(
            &init_ix(ConfigParams {
                history_threshold_secs: threshold,
                ..valid_params()
            }),
            &init_accounts(),
            &[custom(ClubError::HistoryThresholdInvalid)],
        );
    }
}

/// Скарбниця в чужій валюті виявилася б лише тоді, коли перший переказ комісії
/// уже мусив пройти, — тобто на найдорожчому кроці.
#[test]
fn init_protocol_refuses_a_fee_vault_in_another_currency() {
    let mollusk = setup();
    let other_mint = Pubkey::new_from_array([99u8; 32]);

    let mut foreign = usdc_account(ADMIN, 0);
    foreign.data[..32].copy_from_slice(other_mint.as_ref());

    mollusk.process_and_validate_instruction(
        &init_ix(valid_params()),
        &replacing(&init_accounts(), FEE_VAULT, foreign),
        &[Check::err(ProgramError::Custom(u32::from(
            anchor_lang::error::ErrorCode::ConstraintTokenMint,
        )))],
    );
}

#[test]
fn update_config_replaces_the_parameters_and_nothing_else() {
    let mollusk = setup();
    let changed = ConfigParams {
        origination_fee_bps: 200,
        trading_fee_bps: 0,
        max_pledge_bps: 10_000,
        min_tenor_secs: 45 * DAY,
        max_tenor_secs: 120 * DAY,
        history_threshold_secs: 1,
    };

    let result = mollusk.process_and_validate_instruction(
        &update_ix(ADMIN, changed),
        &update_accounts(ADMIN),
        &[Check::success()],
    );

    let config: ProtocolConfig = decode(&result, &config_pda().0);

    assert_eq!(config.origination_fee_bps, changed.origination_fee_bps);
    assert_eq!(config.trading_fee_bps, changed.trading_fee_bps);
    assert_eq!(config.max_pledge_bps, changed.max_pledge_bps);
    assert_eq!(config.min_tenor_secs, changed.min_tenor_secs);
    assert_eq!(config.max_tenor_secs, changed.max_tenor_secs);
    assert_eq!(config.history_threshold_secs, changed.history_threshold_secs);

    // `FR-036` перелічує рівно ці чотири групи параметрів. Ані власник прав,
    // ані валюта, ані скарбниця через цю інструкцію не переїжджають.
    assert_eq!(config.admin, anchor_key(ADMIN));
    assert_eq!(config.usdc_mint, anchor_key(USDC_MINT));
    assert_eq!(config.fee_vault, anchor_key(FEE_VAULT));
    assert_eq!(config.bump, config_pda().1);
}

#[test]
fn update_config_refuses_a_signature_that_is_not_the_admin() {
    let mollusk = setup();

    mollusk.process_and_validate_instruction(
        &update_ix(OUTSIDER, valid_params()),
        &update_accounts(OUTSIDER),
        &[custom(ClubError::Unauthorized)],
    );
}

/// Найдешевший спосіб обійти перевірку — виставити заборонений набір не при
/// створенні, а зміною. Перевірка в обох інструкціях одна саме тому.
#[test]
fn update_config_refuses_everything_init_refuses() {
    let mollusk = setup();

    let refused = [
        (
            ConfigParams {
                origination_fee_bps: 201,
                ..valid_params()
            },
            ClubError::FeeOutOfRange,
        ),
        (
            ConfigParams {
                max_pledge_bps: 0,
                ..valid_params()
            },
            ClubError::PledgeCapOutOfRange,
        ),
        (
            ConfigParams {
                max_tenor_secs: 181 * DAY,
                ..valid_params()
            },
            ClubError::TermRangeOutOfBounds,
        ),
        (
            ConfigParams {
                history_threshold_secs: 0,
                ..valid_params()
            },
            ClubError::HistoryThresholdInvalid,
        ),
    ];

    for (params, error) in refused {
        mollusk.process_and_validate_instruction(
            &update_ix(ADMIN, params),
            &update_accounts(ADMIN),
            &[custom(error)],
        );
    }
}

/// Друга половина `FR-036`: зміна не застосовується до вже створених випусків.
/// Випуск подається в ту саму транзакцію зайвим акаунтом — інструкція не має
/// куди його взяти, і після зміни його байти мусять лишитись тими самими.
#[test]
fn update_config_does_not_reach_an_issue_that_already_exists() {
    let mollusk = setup();
    let source = source_pda(ISSUER, 0).0;
    let (issue_key, issue_bump) = issue_pda(source, 0);

    let issue = Issue {
        source: anchor_key(source),
        bond_mint: anchor_key(BOND_MINT),
        escrow_vault: anchor_key(FEE_VAULT),
        subscription_vault: anchor_key(FEE_VAULT),
        face: 100_000_000,
        coupon_bps: 800,
        pledge_bps: 3_000,
        maturity_ts: NOW + 90 * DAY,
        subscription_end_ts: NOW + 7 * DAY,
        min_lot: 1_000_000,
        raised: 100_000_000,
        obligation_total: 108_000_000,
        repaid_total: 0,
        payout_index: 0,
        state: IssueState::Repaying,
        seq: 0,
        bump: issue_bump,
    };
    let before = anchor_account(&issue);

    let mut accounts = update_accounts(ADMIN);
    accounts.push((issue_key, before.clone()));

    let mut instruction = update_ix(
        ADMIN,
        ConfigParams {
            // Стеля піднімається вдвічі проти тієї, під якою випуск
            // створювався: якби конфіг діяв на випуски заднім числом,
            // помітно було б саме на цьому полі.
            max_pledge_bps: 6_000,
            ..valid_params()
        },
    );
    instruction.accounts.push(AccountMeta::new(issue_key, false));

    let result =
        mollusk.process_and_validate_instruction(&instruction, &accounts, &[Check::success()]);

    let after: Issue = decode(&result, &issue_key);

    assert_eq!(after.pledge_bps, issue.pledge_bps);
    assert_eq!(after.obligation_total, issue.obligation_total);
    assert_eq!(after.state, issue.state);
    assert_eq!(
        result.get_account(&issue_key).expect("випуск на місці").data,
        before.data,
        "зміна конфігу переписала вже створений випуск"
    );
}
