//! `open_position` (`FR-038`) на справжньому байткоді.
//!
//! Головне, що тут доводиться, — не «акаунт створився», а що створився **саме
//! той** акаунт. Список акаунтів гука лежить у мінті з моменту `create_issue`
//! і переписати його нічим: якщо `open_position` заведе чекпоінт на інших
//! seeds, розійдеться не тест, а продукт — передача бонду не пройде взагалі, і
//! виявиться це вже після деплою. Тому адреса створеного акаунта звіряється не
//! з нашою ж дерівацією, а з тим, що резолвить сам список.
//!
//! Випуск тут подається вже створеним: `create_issue` перевірений у своєму
//! файлі, а тягнути його сюди означало б міряти дві інструкції одним тестом.

#[path = "harness.rs"]
mod harness;

use {
    anchor_lang::{InstructionData, Space},
    daddys_club::{
        instructions::issue::hook_account_metas,
        state::{HolderCheckpoint, Issue, IssueState},
    },
    harness::*,
    mollusk_svm::result::{Check, InstructionResult},
    solana_account::Account,
    solana_address::Address as Pubkey,
    solana_instruction::{AccountMeta, Instruction},
    solana_program_error::ProgramError,
};

const SOURCE_SEQ: u64 = 0;
const SEQ: u64 = 0;

/// Сховища випуску — ключі довільні, бо дерівацією не задані (сесія 7); тут
/// вони лише заповнюють поля `Issue`.
const SUBSCRIPTION_VAULT: Pubkey = Pubkey::new_from_array([33u8; 32]);
const ESCROW_VAULT: Pubkey = Pubkey::new_from_array([34u8; 32]);

/// Індекс виплати, вже накопичений випуском. Ненульовий навмисно: чекпоінт у
/// нулі інакше не відрізнити від чекпоінта, знятого з індексу.
const INDEX_SO_FAR: u128 = 4_500_000_000_000;

fn issue_key() -> Pubkey {
    issue_pda(source_pda(ISSUER, SOURCE_SEQ).0, SEQ).0
}

/// Випуск у тому вигляді, в якому його лишає `create_issue`, плюс рух
/// погашення, який задає тест.
fn stored_issue(state: IssueState, payout_index: u128) -> Issue {
    Issue {
        source: anchor_key(source_pda(ISSUER, SOURCE_SEQ).0),
        bond_mint: anchor_key(BOND_MINT),
        escrow_vault: anchor_key(ESCROW_VAULT),
        subscription_vault: anchor_key(SUBSCRIPTION_VAULT),
        face: 250_000_000_000,
        coupon_bps: 950,
        pledge_bps: 1_200,
        maturity_ts: NOW + 90 * DAY,
        subscription_end_ts: NOW + 7 * DAY,
        min_lot: 1_000_000,
        raised: 0,
        obligation_total: 273_750_000_000,
        repaid_total: 0,
        payout_index,
        state,
        seq: SEQ,
        bump: issue_pda(source_pda(ISSUER, SOURCE_SEQ).0, SEQ).1,
    }
}

fn open_ix(payer: Pubkey, owner: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::OpenPosition {}.data(),
        vec![
            AccountMeta::new_readonly(issue_key(), false),
            AccountMeta::new(holder_pda(issue_key(), owner).0, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    )
}

fn open_accounts(payer: Pubkey, owner: Pubkey, issue: Issue) -> Vec<(Pubkey, Account)> {
    vec![
        (issue_key(), anchor_account(&issue)),
        (holder_pda(issue_key(), owner).0, uninitialized()),
        (payer, wallet()),
        (owner, wallet()),
        system_program(),
    ]
}

fn anchor_err(code: anchor_lang::error::ErrorCode) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(code)))
}

/// Прогін на живому випуску: підписка ще відкрита, індекс уже зрушений.
fn open(payer: Pubkey, owner: Pubkey) -> InstructionResult {
    setup().process_and_validate_instruction(
        &open_ix(payer, owner),
        &open_accounts(
            payer,
            owner,
            stored_issue(IssueState::Subscribing, INDEX_SO_FAR),
        ),
        &[Check::success()],
    )
}

/// Той самий резолв, що його робить Token-2022 перед `execute`: адреса
/// чекпоінта береться зі списку в мінті, а не з наших seed-байтів. Розкладка
/// набору — `issue.rs`: акаунт 2 — рахунок отримувача, акаунт 5 — випуск.
fn hook_resolves_holder_of(owner: Pubkey) -> Pubkey {
    let issue = anchor_key(issue_key());
    let mint = anchor_key(BOND_MINT);

    // Токен-акаунт: `mint` (32 байти), далі `owner`. Решта для резолву не
    // потрібна — гук читає рівно ці 32 байти зі зсуву 32.
    let mut destination_token = [0u8; 72];
    destination_token[32..64].copy_from_slice(anchor_key(owner).as_ref());

    let accounts = |index: usize| -> Option<(&anchor_lang::prelude::Pubkey, Option<&[u8]>)> {
        match index {
            2 => Some((&mint, Some(&destination_token[..]))),
            5 => Some((&issue, None)),
            _ => None,
        }
    };

    let metas = hook_account_metas(&issue).expect("список гука будується");
    let resolved = metas[2]
        .resolve(&[], &daddys_club::ID, accounts)
        .expect("чекпоінт отримувача резолвиться");

    Pubkey::new_from_array(resolved.pubkey.to_bytes())
}

/// Контракт із T018 і причина, чому ця інструкція взагалі існує. Якщо seeds
/// розійдуться, тут упаде саме звірка з резолвом — а не абстрактна деривація.
#[test]
fn the_hook_resolves_to_the_ledger_this_instruction_creates() {
    let result = open(INVESTOR, INVESTOR);

    let created = hook_resolves_holder_of(INVESTOR);

    assert_eq!(
        created,
        holder_pda(issue_key(), INVESTOR).0,
        "гук веде не туди, куди дерівує протокол"
    );
    assert!(
        result.get_account(&created).is_some_and(|account| {
            account.owner == club_id() && account.data.len() == 8 + HolderCheckpoint::INIT_SPACE
        }),
        "за адресою, яку резолвить гук, немає готового чекпоінта"
    );
}

#[test]
fn open_position_records_whose_ledger_it_is_and_on_which_issue() {
    let result = open(INVESTOR, INVESTOR);
    let holder: HolderCheckpoint = decode(&result, &holder_pda(issue_key(), INVESTOR).0);

    assert_eq!(holder.issue, anchor_key(issue_key()));
    assert_eq!(holder.owner, anchor_key(INVESTOR));
    assert_eq!(holder.bump, holder_pda(issue_key(), INVESTOR).1);
}

/// `FR-016` рахує претензію різницею індексів. Чекпоінт у нулі віддав би
/// новому власникові всі виплати, що накопичились до його появи, — тобто гроші
/// тих, хто тримав бонд увесь цей час.
#[test]
fn a_fresh_ledger_starts_from_todays_index_and_owes_nothing() {
    let result = open(INVESTOR, INVESTOR);
    let holder: HolderCheckpoint = decode(&result, &holder_pda(issue_key(), INVESTOR).0);

    assert_eq!(holder.index_at_checkpoint, INDEX_SO_FAR);
    assert_eq!(holder.accrued, 0);
    assert_eq!(holder.claimed_total, 0);
}

/// `FR-038`: відкриття дозвільне. Власник не підписує нічого — і це те, на чому
/// стоїть `FR-024`: наша вторинка відкриває облік покупця в тій самій
/// транзакції, що й купівля.
#[test]
fn anyone_may_open_a_ledger_for_a_wallet_that_did_not_sign() {
    let result = setup().process_and_validate_instruction(
        &open_ix(OUTSIDER, INVESTOR),
        &open_accounts(
            OUTSIDER,
            INVESTOR,
            stored_issue(IssueState::Subscribing, INDEX_SO_FAR),
        ),
        &[Check::success()],
    );

    let holder: HolderCheckpoint = decode(&result, &holder_pda(issue_key(), INVESTOR).0);

    assert_eq!(
        holder.owner,
        anchor_key(INVESTOR),
        "облік дістався тому, хто заплатив, а не тому, для кого відкривали"
    );
}

/// Два власники — два акаунти. Спільний облік означав би спільну претензію.
#[test]
fn the_ledger_of_one_wallet_is_not_the_ledger_of_another() {
    let mine = open(INVESTOR, INVESTOR);
    let theirs = open(BUYER, BUYER);

    assert_ne!(
        holder_pda(issue_key(), INVESTOR).0,
        holder_pda(issue_key(), BUYER).0
    );

    let mine: HolderCheckpoint = decode(&mine, &holder_pda(issue_key(), INVESTOR).0);
    let theirs: HolderCheckpoint = decode(&theirs, &holder_pda(issue_key(), BUYER).0);

    assert_eq!(mine.owner, anchor_key(INVESTOR));
    assert_eq!(theirs.owner, anchor_key(BUYER));
    assert_eq!(
        hook_resolves_holder_of(BUYER),
        holder_pda(issue_key(), BUYER).0
    );
}

/// Повторне відкриття мусить упертися в уже створений акаунт, а не переписати
/// його: тіло інструкції пише поля безумовно, тож прохідний другий виклик
/// обнулив би `accrued` живому власникові. Саме тому тут `init`, а не
/// `init_if_needed`.
#[test]
fn open_position_refuses_to_reopen_a_ledger_that_already_exists() {
    let live = HolderCheckpoint {
        issue: anchor_key(issue_key()),
        owner: anchor_key(INVESTOR),
        index_at_checkpoint: INDEX_SO_FAR,
        accrued: 7_000_000,
        claimed_total: 3_000_000,
        bump: holder_pda(issue_key(), INVESTOR).1,
    };

    let accounts = replacing(
        &open_accounts(
            INVESTOR,
            INVESTOR,
            stored_issue(IssueState::Repaying, INDEX_SO_FAR),
        ),
        holder_pda(issue_key(), INVESTOR).0,
        anchor_account(&live),
    );

    let result = setup().process_instruction(&open_ix(INVESTOR, INVESTOR), &accounts);

    assert!(
        !result.program_result.is_ok(),
        "облік переписався: {:?}",
        result.program_result
    );

    let untouched: HolderCheckpoint = decode(&result, &holder_pda(issue_key(), INVESTOR).0);
    assert_eq!(untouched.accrued, live.accrued, "нараховане стерлось");
    assert_eq!(untouched.claimed_total, live.claimed_total);
}

/// Seeds чекпоінта містять випуск, тому облік з одного випуску не підставити в
/// інший: набір не сходиться ще до тіла інструкції.
#[test]
fn a_ledger_of_another_issue_does_not_fit_this_one() {
    let other_issue = issue_pda(source_pda(ISSUER, SOURCE_SEQ).0, SEQ + 1).0;
    let stray = holder_pda(other_issue, INVESTOR).0;

    let instruction = Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::OpenPosition {}.data(),
        vec![
            AccountMeta::new_readonly(issue_key(), false),
            AccountMeta::new(stray, false),
            AccountMeta::new(INVESTOR, true),
            AccountMeta::new_readonly(INVESTOR, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    );

    let accounts = vec![
        (
            issue_key(),
            anchor_account(&stored_issue(IssueState::Subscribing, INDEX_SO_FAR)),
        ),
        (stray, uninitialized()),
        (INVESTOR, wallet()),
        system_program(),
    ];

    setup().process_and_validate_instruction(
        &instruction,
        &accounts,
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintSeeds)],
    );
}

/// Випуску немає — обліку теж бути не може: інакше чекпоінти заводились би під
/// адреси, за якими ніколи не з'явиться жодного бонду.
#[test]
fn open_position_refuses_an_issue_that_does_not_exist() {
    let accounts = replacing(
        &open_accounts(
            INVESTOR,
            INVESTOR,
            stored_issue(IssueState::Subscribing, INDEX_SO_FAR),
        ),
        issue_key(),
        uninitialized(),
    );

    setup().process_and_validate_instruction(
        &open_ix(INVESTOR, INVESTOR),
        &accounts,
        &[anchor_err(
            anchor_lang::error::ErrorCode::AccountNotInitialized,
        )],
    );
}

/// Погашений випуск обліку не забороняє: вимоги на це немає, а передавати вже
/// погашений бонд `FR-017` не боронить. Тест прибиває саме відсутність
/// політики — інакше вона тихо з'явиться в наступній задачі.
#[test]
fn a_repaid_issue_still_accepts_a_new_ledger() {
    let result = setup().process_and_validate_instruction(
        &open_ix(BUYER, BUYER),
        &open_accounts(BUYER, BUYER, stored_issue(IssueState::Repaid, INDEX_SO_FAR)),
        &[Check::success()],
    );

    let holder: HolderCheckpoint = decode(&result, &holder_pda(issue_key(), BUYER).0);

    assert_eq!(holder.index_at_checkpoint, INDEX_SO_FAR);
}

/// Розмір акаунта — це те, що читають клієнти: `accounts.ts` розбирає його на
/// фіксованих зсувах, і зайве поле в `HolderCheckpoint` має червонити тут, а не
/// на першому декодуванні у вебі.
#[test]
fn the_ledger_account_is_exactly_the_size_the_clients_expect() {
    let result = open(INVESTOR, INVESTOR);
    let stored = result
        .get_account(&holder_pda(issue_key(), INVESTOR).0)
        .expect("чекпоінт є в результаті");

    assert_eq!(stored.data.len(), 8 + HolderCheckpoint::INIT_SPACE);
    assert_eq!(stored.owner, club_id());
}
