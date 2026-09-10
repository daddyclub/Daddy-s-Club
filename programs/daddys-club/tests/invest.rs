//! `open_position` (`FR-038`) і `subscribe` (`FR-008`…`FR-010`, `FR-013`) на
//! справжньому байткоді.
//!
//! Головне, що доводить перша половина, — не «акаунт створився», а що створився
//! **саме той** акаунт. Список акаунтів гука лежить у мінті з моменту
//! `create_issue` і переписати його нічим: якщо `open_position` заведе чекпоінт
//! на інших seeds, розійдеться не тест, а продукт — передача бонду не пройде
//! взагалі, і виявиться це вже після деплою. Тому адреса створеного акаунта
//! звіряється не з нашою ж дерівацією, а з тим, що резолвить сам список.
//!
//! Друга половина міряє гроші. Внесок і бонд рухаються в одній транзакції
//! (`FR-009`), тому кожен прогін дивиться на чотири величини одразу: скільки
//! списано з інвестора, скільки лягло в сховище, скільки надруковано і що
//! записано у випуск. Розбіжність між ними — це або загублені кошти, або бонд
//! без покриття, і ловити її треба тут.
//!
//! Випуск тут подається вже створеним: `create_issue` перевірений у своєму
//! файлі, а тягнути його сюди означало б міряти дві інструкції одним тестом.

#[path = "harness.rs"]
mod harness;

use {
    anchor_lang::{InstructionData, Space},
    anchor_spl::token_2022::spl_token_2022::{
        extension::StateWithExtensions, state::Mint as HookMint,
    },
    daddys_club::{
        errors::ClubError,
        instructions::issue::hook_account_metas,
        state::{HolderCheckpoint, Issue, IssueState},
    },
    harness::*,
    mollusk_svm::{
        result::{Check, InstructionResult},
        Mollusk,
    },
    solana_account::Account,
    solana_address::Address as Pubkey,
    solana_instruction::{AccountMeta, Instruction},
    solana_program_error::ProgramError,
};

/// Індекс виплати, вже накопичений випуском. Ненульовий навмисно: чекпоінт у
/// нулі інакше не відрізнити від чекпоінта, знятого з індексу.
const INDEX_SO_FAR: u128 = 4_500_000_000_000;

fn open_ix(payer: Pubkey, owner: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::OpenPosition {}.data(),
        vec![
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(holder_pda(demo_issue(), owner).0, false),
            AccountMeta::new(payer, true),
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    )
}

fn open_accounts(payer: Pubkey, owner: Pubkey, issue: Issue) -> Vec<(Pubkey, Account)> {
    vec![
        (demo_issue(), anchor_account(&issue)),
        (holder_pda(demo_issue(), owner).0, uninitialized()),
        (payer, wallet()),
        (owner, wallet()),
        system_program(),
    ]
}

fn anchor_err(code: anchor_lang::error::ErrorCode) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(code)))
}

fn custom(error: ClubError) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(error)))
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
    let issue = anchor_key(demo_issue());
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
        holder_pda(demo_issue(), INVESTOR).0,
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
    let holder: HolderCheckpoint = decode(&result, &holder_pda(demo_issue(), INVESTOR).0);

    assert_eq!(holder.issue, anchor_key(demo_issue()));
    assert_eq!(holder.owner, anchor_key(INVESTOR));
    assert_eq!(holder.bump, holder_pda(demo_issue(), INVESTOR).1);
}

/// `FR-016` рахує претензію різницею індексів. Чекпоінт у нулі віддав би
/// новому власникові всі виплати, що накопичились до його появи, — тобто гроші
/// тих, хто тримав бонд увесь цей час.
#[test]
fn a_fresh_ledger_starts_from_todays_index_and_owes_nothing() {
    let result = open(INVESTOR, INVESTOR);
    let holder: HolderCheckpoint = decode(&result, &holder_pda(demo_issue(), INVESTOR).0);

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

    let holder: HolderCheckpoint = decode(&result, &holder_pda(demo_issue(), INVESTOR).0);

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
        holder_pda(demo_issue(), INVESTOR).0,
        holder_pda(demo_issue(), BUYER).0
    );

    let mine: HolderCheckpoint = decode(&mine, &holder_pda(demo_issue(), INVESTOR).0);
    let theirs: HolderCheckpoint = decode(&theirs, &holder_pda(demo_issue(), BUYER).0);

    assert_eq!(mine.owner, anchor_key(INVESTOR));
    assert_eq!(theirs.owner, anchor_key(BUYER));
    assert_eq!(
        hook_resolves_holder_of(BUYER),
        holder_pda(demo_issue(), BUYER).0
    );
}

/// Повторне відкриття мусить упертися в уже створений акаунт, а не переписати
/// його: тіло інструкції пише поля безумовно, тож прохідний другий виклик
/// обнулив би `accrued` живому власникові. Саме тому тут `init`, а не
/// `init_if_needed`.
#[test]
fn open_position_refuses_to_reopen_a_ledger_that_already_exists() {
    let live = HolderCheckpoint {
        issue: anchor_key(demo_issue()),
        owner: anchor_key(INVESTOR),
        index_at_checkpoint: INDEX_SO_FAR,
        accrued: 7_000_000,
        claimed_total: 3_000_000,
        bump: holder_pda(demo_issue(), INVESTOR).1,
    };

    let accounts = replacing(
        &open_accounts(
            INVESTOR,
            INVESTOR,
            stored_issue(IssueState::Repaying, INDEX_SO_FAR),
        ),
        holder_pda(demo_issue(), INVESTOR).0,
        anchor_account(&live),
    );

    let result = setup().process_instruction(&open_ix(INVESTOR, INVESTOR), &accounts);

    assert!(
        !result.program_result.is_ok(),
        "облік переписався: {:?}",
        result.program_result
    );

    let untouched: HolderCheckpoint = decode(&result, &holder_pda(demo_issue(), INVESTOR).0);
    assert_eq!(untouched.accrued, live.accrued, "нараховане стерлось");
    assert_eq!(untouched.claimed_total, live.claimed_total);
}

/// Seeds чекпоінта містять випуск, тому облік з одного випуску не підставити в
/// інший: набір не сходиться ще до тіла інструкції.
#[test]
fn a_ledger_of_another_issue_does_not_fit_this_one() {
    let other_issue = issue_pda(demo_source(), ISSUE_SEQ + 1).0;
    let stray = holder_pda(other_issue, INVESTOR).0;

    let instruction = Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::OpenPosition {}.data(),
        vec![
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(stray, false),
            AccountMeta::new(INVESTOR, true),
            AccountMeta::new_readonly(INVESTOR, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    );

    let accounts = vec![
        (
            demo_issue(),
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
        demo_issue(),
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

    let holder: HolderCheckpoint = decode(&result, &holder_pda(demo_issue(), BUYER).0);

    assert_eq!(holder.index_at_checkpoint, INDEX_SO_FAR);
}

/// Розмір акаунта — це те, що читають клієнти: `accounts.ts` розбирає його на
/// фіксованих зсувах, і зайве поле в `HolderCheckpoint` має червонити тут, а не
/// на першому декодуванні у вебі.
#[test]
fn the_ledger_account_is_exactly_the_size_the_clients_expect() {
    let result = open(INVESTOR, INVESTOR);
    let stored = result
        .get_account(&holder_pda(demo_issue(), INVESTOR).0)
        .expect("чекпоінт є в результаті");

    assert_eq!(stored.data.len(), 8 + HolderCheckpoint::INIT_SPACE);
    assert_eq!(stored.owner, club_id());
}

// ---- subscribe (`FR-008`…`FR-010`, `FR-013`) --------------------------------

/// USDC і бонд інвестора. Ключі довільні: жодною дерівацією вони не задані —
/// це звичайні токен-акаунти, які інвестор заводить сам.
const INVESTOR_USDC: Pubkey = Pubkey::new_from_array([41u8; 32]);
const INVESTOR_BOND: Pubkey = Pubkey::new_from_array([42u8; 32]);

/// Гаманець інвестора наповнений із запасом: тести міряють, скільки списано, а
/// не скільки він міг заплатити.
const FUNDS: u64 = 300_000_000_000;

/// Внесок середнього розміру — п'ять лотів.
const LOTS: u64 = 5_000_000;

fn stored_holder(owner: Pubkey) -> HolderCheckpoint {
    HolderCheckpoint {
        issue: anchor_key(demo_issue()),
        owner: anchor_key(owner),
        // Поки випуск у `Subscribing`, індекс стоїть на нулі: рухає його лише
        // перехоплення, а воно працює в `Repaying`.
        index_at_checkpoint: 0,
        accrued: 0,
        claimed_total: 0,
        bump: holder_pda(demo_issue(), owner).1,
    }
}

/// Випуск у стані підписки з уже зібраною сумою.
fn subscribing_with(raised: u64) -> Issue {
    Issue {
        raised,
        ..stored_issue(IssueState::Subscribing, 0)
    }
}

fn subscribe_ix(amount: u64) -> Instruction {
    subscribe_ix_with(
        holder_pda(demo_issue(), INVESTOR).0,
        SUBSCRIPTION_VAULT,
        amount,
    )
}

/// Той самий виклик із підміненим обліком або сховищем — так пишеться
/// негативний тест на акаунт, який неможливо підмінити через `replacing`:
/// у ньому змінюється не вміст акаунта, а те, який акаунт подали.
fn subscribe_ix_with(holder: Pubkey, vault: Pubkey, amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Subscribe { amount }.data(),
        vec![
            AccountMeta::new(demo_issue(), false),
            AccountMeta::new_readonly(holder, false),
            AccountMeta::new_readonly(INVESTOR, true),
            AccountMeta::new(INVESTOR_USDC, false),
            AccountMeta::new(vault, false),
            AccountMeta::new(BOND_MINT, false),
            AccountMeta::new(INVESTOR_BOND, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

/// Світ, узгоджений сам із собою: у сховищі лежить рівно зібране, а пропозиція
/// бонду дорівнює йому ж (`FR-013`). Інвестор у цьому світі ще не вносив нічого
/// — усе зібране до нього приніс хтось інший.
fn subscribe_accounts(issue: Issue) -> Vec<(Pubkey, Account)> {
    vec![
        (demo_issue(), anchor_account(&issue)),
        (
            holder_pda(demo_issue(), INVESTOR).0,
            anchor_account(&stored_holder(INVESTOR)),
        ),
        (INVESTOR, wallet()),
        (INVESTOR_USDC, usdc_account(INVESTOR, FUNDS)),
        (SUBSCRIPTION_VAULT, usdc_account(demo_issue(), issue.raised)),
        (BOND_MINT, bond_mint(demo_issue(), issue.raised)),
        (INVESTOR_BOND, bond_account(INVESTOR, 0)),
        (USDC_MINT, usdc_mint(1_000_000_000_000_000)),
        token_program(),
    ]
}

/// Годинник у заданій миті вікна підписки.
fn at(now: i64) -> Mollusk {
    let mut mollusk = setup();
    mollusk.sysvars.clock.unix_timestamp = now;

    mollusk
}

fn subscribe(issue: Issue, amount: u64) -> InstructionResult {
    setup().process_and_validate_instruction(
        &subscribe_ix(amount),
        &subscribe_accounts(issue),
        &[Check::success()],
    )
}

fn refuse(issue: Issue, amount: u64, expected: Check<'_>) {
    setup().process_and_validate_instruction(
        &subscribe_ix(amount),
        &subscribe_accounts(issue),
        &[expected],
    );
}

fn bond_supply(result: &InstructionResult) -> u64 {
    let stored = result.get_account(&BOND_MINT).expect("мінт є в результаті");

    StateWithExtensions::<HookMint>::unpack(&stored.data)
        .expect("мінт розпаковується")
        .base
        .supply
}

/// `FR-009`: етапу розподілу не існує — бонд з'являється в тій самій
/// транзакції, що й гроші. Тому дивимось на всі чотири величини одразу: те, що
/// вони зійшлися поодинці, ще не означає, що вони зійшлися між собою.
#[test]
fn subscribe_moves_usdc_into_the_escrow_and_mints_the_bond_at_once() {
    let result = subscribe(subscribing_with(0), LOTS);

    assert_eq!(token_balance(&result, &INVESTOR_USDC), FUNDS - LOTS);
    assert_eq!(token_balance(&result, &SUBSCRIPTION_VAULT), LOTS);
    assert_eq!(token_balance(&result, &INVESTOR_BOND), LOTS);
    assert_eq!(bond_supply(&result), LOTS);

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.raised, LOTS);
    assert_eq!(
        issue.state,
        IssueState::Subscribing,
        "випуск закрився, не зібравши номіналу"
    );
}

/// `FR-013`: пропозиція росте рівно на суму внесків. Внесок у випуск, де вже
/// щось зібрано, мусить додаватись до зібраного, а не заміняти його.
#[test]
fn a_contribution_adds_to_what_others_already_raised() {
    let already = 40_000_000;
    let result = subscribe(subscribing_with(already), LOTS);

    let issue: Issue = decode(&result, &demo_issue());

    assert_eq!(issue.raised, already + LOTS);
    assert_eq!(token_balance(&result, &SUBSCRIPTION_VAULT), already + LOTS);
    assert_eq!(bond_supply(&result), already + LOTS);
    assert_eq!(
        token_balance(&result, &INVESTOR_BOND),
        LOTS,
        "інвесторові дістався чужий внесок"
    );
}

/// `FR-009`: внесок понад залишок приймається частково — рівно на залишок,
/// решта не списується. Хвіст навмисно менший за лот: саме він і показує, що
/// лот міряється пропозицією, а не тим, що зрештою прийняли. Інакше номінал,
/// добраний до останніх копійок, не добрав би вже ніхто, і `raised == face` з
/// `FR-010` стало б недосяжним.
#[test]
fn the_last_contribution_is_accepted_only_up_to_what_is_left() {
    let tail = 400_000;
    let terms = subscribing_with(stored_issue(IssueState::Subscribing, 0).face - tail);

    assert!(tail < terms.min_lot, "хвіст мусить бути меншим за лот");

    let result = subscribe(terms, LOTS);

    assert_eq!(
        token_balance(&result, &INVESTOR_USDC),
        FUNDS - tail,
        "списали більше, ніж прийняли"
    );
    assert_eq!(token_balance(&result, &INVESTOR_BOND), tail);

    let issue: Issue = decode(&result, &demo_issue());
    assert_eq!(issue.raised, issue.face);
}

/// `FR-010`: успішним випуск стає рівно на повному номіналі — і тільки на
/// ньому.
#[test]
fn a_full_raise_closes_the_issue_as_funded() {
    let face = stored_issue(IssueState::Subscribing, 0).face;

    let short = subscribe(subscribing_with(face - LOTS - 1), LOTS);
    let short: Issue = decode(&short, &demo_issue());
    assert_eq!(short.state, IssueState::Subscribing);
    assert_eq!(short.raised, face - 1);

    let full = subscribe(subscribing_with(face - LOTS), LOTS);
    let full: Issue = decode(&full, &demo_issue());
    assert_eq!(full.state, IssueState::Funded);
    assert_eq!(full.raised, face);
}

/// `FR-009`: мінімальний лот. Рівно лот проходить — межа включна.
#[test]
fn a_contribution_below_the_minimum_lot_is_refused() {
    let min_lot = stored_issue(IssueState::Subscribing, 0).min_lot;

    refuse(
        subscribing_with(0),
        min_lot - 1,
        custom(ClubError::BelowMinimumLot),
    );

    let result = subscribe(subscribing_with(0), min_lot);
    assert_eq!(token_balance(&result, &INVESTOR_BOND), min_lot);
}

/// `FR-008`: вікно підписки. Закрите воно **з** `subscription_end_ts`, не
/// після: у цю саму мить уже можна вимагати повернення (`FR-011`), і дві
/// протилежні дії не мають ділити одну секунду.
#[test]
fn a_contribution_after_the_window_closed_is_refused() {
    let closes_at = stored_issue(IssueState::Subscribing, 0).subscription_end_ts;

    at(closes_at - 1).process_and_validate_instruction(
        &subscribe_ix(LOTS),
        &subscribe_accounts(subscribing_with(0)),
        &[Check::success()],
    );

    for now in [closes_at, closes_at + 1] {
        at(now).process_and_validate_instruction(
            &subscribe_ix(LOTS),
            &subscribe_accounts(subscribing_with(0)),
            &[custom(ClubError::SubscriptionWindowClosed)],
        );
    }
}

/// `FR-008`: підписка живе в одному стані. Зібраний, погашуваний і провалений
/// випуск внесків не беруть — інакше гроші лягли б у сховище, з якого їх уже
/// ніхто не поверне.
#[test]
fn an_issue_that_is_not_subscribing_takes_no_contributions() {
    for state in [
        IssueState::Funded,
        IssueState::Repaying,
        IssueState::PastDue,
        IssueState::Repaid,
        IssueState::Failed,
    ] {
        refuse(
            Issue {
                raised: 0,
                ..stored_issue(state, 0)
            },
            LOTS,
            custom(ClubError::IssueNotSubscribing),
        );
    }
}

/// Другий замок на `FR-009`. Через саму інструкцію в цей стан не потрапити —
/// повний збір одразу переводить випуск у `Funded`, — тому стан складений
/// руками. Замок тут не зайвий: без нього розбіжність стану й зібраного
/// означала б бонд, надрукований понад номінал.
#[test]
fn a_fully_subscribed_issue_refuses_another_contribution() {
    let face = stored_issue(IssueState::Subscribing, 0).face;

    refuse(
        subscribing_with(face),
        LOTS,
        custom(ClubError::IssueFullySubscribed),
    );
}

/// `FR-038`: облік мусить бути відкритий **до** того, як з'являться бонди.
/// Інвестор із бондом і без обліку не зміг би ані забрати виплату, ані передати
/// бонд далі: гук не знайшов би його чекпоінта.
#[test]
fn subscribe_refuses_a_wallet_with_no_open_ledger() {
    let accounts = replacing(
        &subscribe_accounts(subscribing_with(0)),
        holder_pda(demo_issue(), INVESTOR).0,
        uninitialized(),
    );

    setup().process_and_validate_instruction(
        &subscribe_ix(LOTS),
        &accounts,
        &[anchor_err(
            anchor_lang::error::ErrorCode::AccountNotInitialized,
        )],
    );
}

/// Чужим обліком не підписатись: seeds містять підписанта.
#[test]
fn subscribe_refuses_the_ledger_of_another_wallet() {
    let mut accounts = subscribe_accounts(subscribing_with(0));
    accounts[1] = (
        holder_pda(demo_issue(), BUYER).0,
        anchor_account(&stored_holder(BUYER)),
    );

    setup().process_and_validate_instruction(
        &subscribe_ix_with(holder_pda(demo_issue(), BUYER).0, SUBSCRIPTION_VAULT, LOTS),
        &accounts,
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintSeeds)],
    );
}

/// Сховище прибите до випуску через `has_one`. Підставити інше означало б
/// покласти внесок повз ескроу, яке й тримає гроші інвесторів (`FR-008`) —
/// причому сховище погашення в цьому ж випуску виглядає цілком «своїм».
#[test]
fn subscribe_refuses_an_escrow_that_is_not_the_subscription_one() {
    let mut accounts = subscribe_accounts(subscribing_with(0));
    accounts[4] = (ESCROW_VAULT, usdc_account(demo_issue(), 0));

    setup().process_and_validate_instruction(
        &subscribe_ix_with(holder_pda(demo_issue(), INVESTOR).0, ESCROW_VAULT, LOTS),
        &accounts,
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintHasOne)],
    );
}

/// Чужим USDC не заплатити: рахунок мусить належати тому, хто підписав.
#[test]
fn subscribe_refuses_usdc_that_belongs_to_somebody_else() {
    let accounts = replacing(
        &subscribe_accounts(subscribing_with(0)),
        INVESTOR_USDC,
        usdc_account(OUTSIDER, FUNDS),
    );

    setup().process_and_validate_instruction(
        &subscribe_ix(LOTS),
        &accounts,
        &[anchor_err(
            anchor_lang::error::ErrorCode::ConstraintTokenOwner,
        )],
    );
}
