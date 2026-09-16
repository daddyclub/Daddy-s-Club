//! `SC-005` — скільки коштує обробити одне надходження комісії і чи залежить
//! ця вартість від кількості власників бонду.
//!
//! **Головне питання тут не «скільки», а «що саме міряти».** У перехопленні
//! власники не беруть участі взагалі: набір акаунтів `Intercept` фіксований, і
//! жодного `HolderCheckpoint` у ньому немає й бути не може. Тому «випуск зі 100
//! власниками» неможливо подати в саму інструкцію — а замір, знятий двічі на
//! тому самому наборі акаунтів, був би не вимірюванням, а арифметикою: ті самі
//! байти, те саме число, нуль відсотків різниці й нуль доведеного.
//!
//! Тому міряється не інструкція, а **світ, у який вона приходить**. Обидва
//! випуски збираються справжніми інструкціями на справжньому байткоді: в
//! одному номінал вносить один власник однією підпискою, у другому — сто
//! власників сотнею підписок, кожен зі своїм `open_position`, своїм обліком і
//! своїм рахунком бонду. Далі в обидва світи приходить **одне й те саме**
//! надходження комісії, і порівнюються два числа з лічильника SVM.
//!
//! Замір без цих підпор був би декорацією, тому кожна з них має тут свій тест:
//!
//! 1. **Сто власників справді сто.** Сто окремих обліків, сто рахунків бонду,
//!    сума балансів дорівнює пропозиції. Інакше «1 vs 100» — це підпис під
//!    двома однаковими прогонами.
//! 2. **Набір акаунтів перехоплення не росте.** Це і є механізм, через який
//!    вартість не залежить від кількості власників (`FR-015`): виплата
//!    витягується власником по кумулятивному індексу, а не розсилається
//!    надходженням. Набір при цьому береться зі структури, яку згенерувала сама
//!    програма, тому дописаний в `Intercept` акаунт валить цей файл на
//!    компіляції, а не лишає його зеленим.
//! 3. **Скільки коштувало б протилежне.** Єдиний прогін, у якому сто власників
//!    таки подані в надходження — просто дописані в хвіст набору й навіть не
//!    прочитані. Самого лише завантаження вистачає, щоб вартість зросла втричі:
//!    10 852 CU проти 39 352, тобто ≈285 CU на власника **до** будь-якої
//!    роботи з ним. Ось чим насправді тримається `SC-005`: не тим, що
//!    перехоплення добре написане, а тим, що власники не потрапляють у
//!    транзакцію взагалі.
//! 4. **Число.** `SC-005` просить не аргумент, а цифру, зняту на справжньому
//!    байткоді, — і на двох шляхах: на самому перехопленні й на всьому
//!    надходженні цілком, тобто на свопі демо-емітента разом із CPI в ядро
//!    (`FR-004`). Другий шлях і є «вартість обробки одного надходження», яку
//!    платить емітент.
//! 5. **Прилад має роздільну здатність.** Лічильник, який повертає константу,
//!    показав би «0% різниці» на чому завгодно. Тому перевіряється, що два
//!    прогони в однакових світах дають **побітово** те саме число (отже 0%
//!    означає нуль, а не шум), і що на прогоні, який справді інший, той самий
//!    лічильник дає різницю, в рази більшу за бюджет.
//!
//! **Чого замір не доводить.** Він знятий у mollusk, тобто на лічильнику SVM
//! без черг, конкуренції за стан і без плати за розмір транзакції — це та сама
//! межа демо на змішаних даних, що в `SPEC.md` → Припущення. І він нічого не
//! каже про вартість **виплати**: вона справді на власника, але її платить сам
//! власник у власній транзакції, а не надходження комісії. Скільки саме — тут
//! теж заміряно, бо без цієї цифри не видно, від чого відмовились.

#[path = "harness.rs"]
mod harness;

use {
    anchor_lang::InstructionData,
    daddys_club::{
        instructions::issue::IssueParams,
        state::{HolderCheckpoint, Issue, IssueState},
    },
    harness::*,
    mollusk_svm::{
        account_store::AccountStore,
        result::{Check, InstructionResult},
        MolluskContext,
    },
    solana_account::Account,
    solana_address::Address as Pubkey,
    solana_instruction::{AccountMeta, Instruction},
    std::{collections::HashMap, sync::OnceLock},
};

/// Скільки власників у «великому» випуску. Число зі `SC-005`, не кругле «щоб
/// було»: критерій називає саме сотню.
const HOLDERS: usize = 100;

/// Бюджет `SC-005` у базисних пунктах: «менша за 5%».
const BUDGET_BPS: u64 = 500;

const ISSUER_USDC: Pubkey = Pubkey::new_from_array([61u8; 32]);

/// Демо-пул: другий бік свопу і рахунки, між якими їздять гроші трейдера.
/// Адреси локальні, як і в решті тестових файлів, — дерівацією вони не задані.
const TRADER: Pubkey = Pubkey::new_from_array([62u8; 32]);
const TRADER_USDC: Pubkey = Pubkey::new_from_array([63u8; 32]);
const TRADER_BASE: Pubkey = Pubkey::new_from_array([64u8; 32]);
const POOL_USDC: Pubkey = Pubkey::new_from_array([65u8; 32]);
const POOL_BASE: Pubkey = Pubkey::new_from_array([66u8; 32]);
const BASE_MINT: Pubkey = Pubkey::new_from_array([67u8; 32]);
const BASE_DECIMALS: u8 = 6;

const TRADER_FUNDS: u64 = 5_000_000_000;
const RESERVE: u64 = 5_000_000_000;

/// Своп на 1 000 USDC: 0.3% комісії = 3 USDC, 12% перехоплення від неї =
/// 0.36 USDC. Ті самі числа, що в `tests/swap.rs`.
const SWAP_IN: u64 = 1_000_000_000;
const FEE: u64 = 3_000_000;

/// Надходження, яке подається в перехоплення напряму. Дорівнює комісії свопу
/// навмисно: два шляхи мусять ділити однакову суму, інакше їх числа не
/// порівняти між собою.
const INFLOW: u64 = FEE;

/// Скільки лежить на рахунку джерела перед прямим перехопленням. У світі
/// рахунок починається порожнім — його наповнює своп, — тому для прямого
/// виклику він доливається окремо, і однаково в обох світах.
const VAULT_BALANCE: u64 = 10_000_000_000;

type Store = HashMap<Pubkey, Account>;

// ---- Гаманці власників -----------------------------------------------------
//
// Сто адрес не можна написати константами, тому вони дерівуються з номера.
// Перший байт розводить три ролі одного власника: гаманець, рахунок USDC і
// рахунок бонду.

fn keyed(tag: u8, index: usize) -> Pubkey {
    let mut bytes = [0x77u8; 32];
    bytes[0] = tag;
    bytes[1..3].copy_from_slice(&(index as u16).to_le_bytes());

    Pubkey::new_from_array(bytes)
}

fn holder_wallet(index: usize) -> Pubkey {
    keyed(0xA1, index)
}

fn holder_usdc(index: usize) -> Pubkey {
    keyed(0xA2, index)
}

fn holder_bond(index: usize) -> Pubkey {
    keyed(0xA3, index)
}

// ---- Інструкції, якими збирається світ -------------------------------------

/// Умови з картки випуску M0 — ті самі, які фіксує `stored_issue`.
fn terms() -> IssueParams {
    let issue = stored_issue(IssueState::Subscribing, 0);

    IssueParams {
        face: issue.face,
        coupon_bps: issue.coupon_bps,
        pledge_bps: issue.pledge_bps,
        maturity_ts: issue.maturity_ts,
        subscription_end_ts: issue.subscription_end_ts,
        min_lot: issue.min_lot,
    }
}

fn create_issue_ix() -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::CreateIssue {
            seq: ISSUE_SEQ,
            params: terms(),
        }
        .data(),
        vec![
            AccountMeta::new_readonly(config_pda().0, false),
            AccountMeta::new(demo_source(), false),
            AccountMeta::new(demo_issue(), false),
            AccountMeta::new(ISSUER, true),
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

fn open_ix(owner: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::OpenPosition {}.data(),
        vec![
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(holder_pda(demo_issue(), owner).0, false),
            AccountMeta::new(ISSUER, true),
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    )
}

fn subscribe_ix(index: usize, amount: u64) -> Instruction {
    let owner = holder_wallet(index);

    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Subscribe { amount }.data(),
        vec![
            AccountMeta::new(demo_issue(), false),
            AccountMeta::new_readonly(holder_pda(demo_issue(), owner).0, false),
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new(holder_usdc(index), false),
            AccountMeta::new(SUBSCRIPTION_VAULT, false),
            AccountMeta::new(BOND_MINT, false),
            AccountMeta::new(holder_bond(index), false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

fn withdraw_ix() -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::WithdrawProceeds {}.data(),
        vec![
            AccountMeta::new_readonly(config_pda().0, false),
            AccountMeta::new(demo_issue(), false),
            AccountMeta::new_readonly(demo_source(), false),
            AccountMeta::new_readonly(ISSUER, true),
            AccountMeta::new(ISSUER_USDC, false),
            AccountMeta::new(SUBSCRIPTION_VAULT, false),
            AccountMeta::new(FEE_VAULT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

// ---- Інструкції, які міряються ---------------------------------------------
//
// Набори акаунтів тут беруться зі структур, **згенерованих самою програмою**, а
// не переписуються метами вручну, як у решті тестових файлів. Різниця в тому,
// що саме доводить `the_account_set_of_one_arrival_does_not_grow_with_the_owners`:
// на переписаному наборі він казав би «у наборі вісім акаунтів, бо я так
// написав», а на згенерованому — «у наборі рівно те, чого просить інструкція».
// Дописаний в `Intercept` акаунт зламає цей файл на компіляції; переписаний
// набір лишив би його зеленим і після того, як через перехоплення повели б
// власників.
//
// Ціна — конверсія мет: Anchor віддає їх на своїй версії
// `solana-instruction`, mollusk приймає на нашій, і в графі ці версії дві.

fn metas<T: anchor_lang::ToAccountMetas>(accounts: &T) -> Vec<AccountMeta> {
    accounts
        .to_account_metas(None)
        .into_iter()
        .map(|meta| AccountMeta {
            pubkey: Pubkey::new_from_array(meta.pubkey.to_bytes()),
            is_signer: meta.is_signer,
            is_writable: meta.is_writable,
        })
        .collect()
}

/// Перехоплення напряму: підпис PDA пулу приймається mollusk за метаданими.
/// Це вужчий шлях, ніж справжній — там підпис ставить сама програма зсередини
/// CPI, — і саме тому поруч міряється своп.
fn intercept_ix(with_issue: bool) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Intercept { amount: INFLOW }.data(),
        metas(&daddys_club::accounts::Intercept {
            source: anchor_key(demo_source()),
            authority: anchor_key(issuer_authority().0),
            vault: anchor_key(SOURCE_VAULT),
            // Опційна трійця подається вся або жодна: без випуску дохід лише
            // спостерігається (`FR-028`).
            issue: with_issue.then(|| anchor_key(demo_issue())),
            escrow_vault: with_issue.then(|| anchor_key(ESCROW_VAULT)),
            bond_mint: with_issue.then(|| anchor_key(BOND_MINT)),
            usdc_mint: anchor_key(USDC_MINT),
            token_program: anchor_key(token_program().0),
        }),
    )
}

/// Надходження комісії цілком: своп трейдера, всередині якого комісія виникає
/// і тут же розщеплюється CPI в ядро (`FR-004`). Це і є та «вартість обробки
/// одного надходження», про яку питає `SC-005`.
fn swap_ix() -> Instruction {
    Instruction::new_with_bytes(
        issuer_program_id(),
        &demo_issuer::instruction::Swap { amount_in: SWAP_IN }.data(),
        metas(&demo_issuer::accounts::Swap {
            pool: anchor_key(issuer_authority().0),
            trader: anchor_key(TRADER),
            trader_usdc: anchor_key(TRADER_USDC),
            trader_base: anchor_key(TRADER_BASE),
            pool_usdc: anchor_key(POOL_USDC),
            pool_base: anchor_key(POOL_BASE),
            fee_vault: anchor_key(SOURCE_VAULT),
            source: anchor_key(demo_source()),
            issue: Some(anchor_key(demo_issue())),
            escrow_vault: Some(anchor_key(ESCROW_VAULT)),
            bond_mint: Some(anchor_key(BOND_MINT)),
            usdc_mint: anchor_key(USDC_MINT),
            base_mint: anchor_key(BASE_MINT),
            token_program: anchor_key(token_program().0),
            club_program: anchor_key(club_id()),
        }),
    )
}

/// Виплата одному власникові — та сама робота, яка в push-моделі лягала б на
/// кожне надходження, а тут не лягає на жодне.
fn claim_ix(index: usize) -> Instruction {
    let owner = holder_wallet(index);

    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Claim {}.data(),
        metas(&daddys_club::accounts::Claim {
            issue: anchor_key(demo_issue()),
            holder: anchor_key(holder_pda(demo_issue(), owner).0),
            owner: anchor_key(owner),
            owner_usdc: anchor_key(holder_usdc(index)),
            escrow_vault: anchor_key(ESCROW_VAULT),
            owner_bond: anchor_key(holder_bond(index)),
            bond_mint: anchor_key(BOND_MINT),
            usdc_mint: anchor_key(USDC_MINT),
            token_program: anchor_key(token_program().0),
        }),
    )
}

// ---- Світ --------------------------------------------------------------

/// Випуск у погашенні, зібраний **справжніми інструкціями**: створення,
/// `holders` разів «відкрити облік + внести лот», видача номіналу емітенту.
///
/// Літералів тут немає навмисно. Випуск, покладений у сховище готовим
/// (`repaying(…)`), був би однаковий при будь-якому `holders` — і тоді «1 vs
/// 100» перетворилось би на підпис під двома однаковими прогонами. Ціна
/// чесності — двісті інструкцій на світ; це єдина причина, чому світи
/// будуються один раз на бінарник і далі клонуються знімком.
fn world(holders: usize) -> Store {
    let pool = issuer_authority().0;
    let mut store = Store::new();

    store.insert(config_pda().0, anchor_account(&stored_config()));
    store.insert(demo_source(), anchor_account(&stored_source(None, 0)));
    store.insert(ISSUER, wallet());
    store.insert(ISSUER_USDC, usdc_account(ISSUER, 0));
    store.insert(FEE_VAULT, usdc_account(ADMIN, 0));
    store.insert(USDC_MINT, usdc_mint(1_000_000_000_000_000));

    // Пул демо-емітента. Рахунок джерела починається порожнім — так само, як у
    // `tests/swap.rs`: комісію на нього кладе сам своп, до виклику ядра.
    store.insert(pool, uninitialized());
    store.insert(SOURCE_VAULT, token_account(USDC_MINT, pool, 0));
    store.insert(TRADER, wallet());
    store.insert(TRADER_USDC, token_account(USDC_MINT, TRADER, TRADER_FUNDS));
    store.insert(TRADER_BASE, token_account(BASE_MINT, TRADER, 0));
    store.insert(POOL_USDC, token_account(USDC_MINT, pool, 0));
    store.insert(POOL_BASE, token_account(BASE_MINT, pool, RESERVE));
    store.insert(BASE_MINT, plain_mint(BASE_DECIMALS, 1_000_000_000_000_000));

    let context = setup().with_context(store);
    context.process_and_validate_instruction(&create_issue_ix(), &[Check::success()]);

    let face = terms().face;
    let lot = face / holders as u64;
    assert_eq!(
        lot * holders as u64,
        face,
        "номінал не розкладається на {holders} рівних лотів"
    );

    for index in 0..holders {
        let owner = holder_wallet(index);
        {
            let mut accounts = context.account_store.borrow_mut();
            accounts.store_account(owner, wallet());
            accounts.store_account(holder_usdc(index), usdc_account(owner, lot));
            accounts.store_account(holder_bond(index), bond_account(owner, 0));
        }

        context.process_and_validate_instruction(&open_ix(owner), &[Check::success()]);
        context.process_and_validate_instruction(&subscribe_ix(index, lot), &[Check::success()]);
    }

    // Видача переводить випуск у `Repaying` — єдиний стан, у якому перехоплення
    // розщеплює потік.
    context.process_and_validate_instruction(&withdraw_ix(), &[Check::success()]);

    let snapshot = context.account_store.borrow().clone();

    snapshot
}

/// Обидва світи будуються один раз на тестовий бінарник: 200 справжніх
/// інструкцій — це дорого, а знімок клонується задарма.
fn one_owner() -> &'static Store {
    static WORLD: OnceLock<Store> = OnceLock::new();
    WORLD.get_or_init(|| world(1))
}

fn hundred_owners() -> &'static Store {
    static WORLD: OnceLock<Store> = OnceLock::new();
    WORLD.get_or_init(|| world(HOLDERS))
}

fn at(world: &Store) -> MolluskContext<Store> {
    setup().with_context(world.clone())
}

/// Долив рахунку джерела під прямий виклик перехоплення: у світі його наповнює
/// своп, а тут свопу немає.
fn funded_source(world: &Store) -> Store {
    let mut store = world.clone();
    store.insert(
        SOURCE_VAULT,
        token_account(USDC_MINT, issuer_authority().0, VAULT_BALANCE),
    );

    store
}

// ---- Лічильник -------------------------------------------------------------

/// Один прогін у свіжому світі зі знімка.
fn run(world: &Store, ix: &Instruction) -> InstructionResult {
    setup()
        .with_context(world.clone())
        .process_and_validate_instruction(ix, &[Check::success()])
}

/// Один замір: той самий прогін, але потрібне з нього — число з лічильника SVM.
fn cu(world: &Store, ix: &Instruction) -> u64 {
    run(world, ix).compute_units_consumed
}

/// Розбіжність двох замірів у базисних пунктах від меншого з них.
fn spread_bps(left: u64, right: u64) -> u64 {
    let (low, high) = if left <= right {
        (left, right)
    } else {
        (right, left)
    };
    assert!(low > 0, "замір нульовий — лічильник нічого не порахував");

    (high - low) * 10_000 / low
}

fn stored<T: anchor_lang::AccountDeserialize>(world: &Store, key: &Pubkey) -> T {
    let account = world
        .get(key)
        .unwrap_or_else(|| panic!("акаунт {key} є у світі"));

    T::try_deserialize(&mut account.data.as_slice()).expect("акаунт розбирається")
}

fn bond_balance(world: &Store, key: &Pubkey) -> u64 {
    let account = world
        .get(key)
        .unwrap_or_else(|| panic!("рахунок {key} є у світі"));

    anchor_spl::token_2022::spl_token_2022::extension::StateWithExtensions::<
        anchor_spl::token_2022::spl_token_2022::state::Account,
    >::unpack(&account.data)
    .expect("рахунок розпаковується")
    .base
    .amount
}

// ---- 1. Сто власників справді сто ------------------------------------------

/// Підпора під увесь замір. «Випуск зі 100 власниками» — це не назва змінної:
/// сто обліків мусять існувати, сто рахунків бонду мусять тримати номінал, а їх
/// сума — дорівнювати пропозиції. Без цього тесту два прогони нижче могли б
/// відрізнятись лише підписом до них.
#[test]
fn a_hundred_owners_are_a_hundred_ledgers_and_a_hundred_balances() {
    let big = hundred_owners();
    let small = one_owner();

    let issue: Issue = stored(big, &demo_issue());
    assert_eq!(issue.state, IssueState::Repaying);
    assert_eq!(issue.raised, issue.face, "номінал зібрано не повністю");

    let lot = issue.face / HOLDERS as u64;
    let mut total = 0u64;

    for index in 0..HOLDERS {
        let owner = holder_wallet(index);
        let ledger: HolderCheckpoint = stored(big, &holder_pda(demo_issue(), owner).0);

        assert_eq!(ledger.issue, anchor_key(demo_issue()));
        assert_eq!(ledger.owner, anchor_key(owner), "облік {index} чужий");

        let balance = bond_balance(big, &holder_bond(index));
        assert_eq!(balance, lot, "власник {index} тримає не свій лот");
        total += balance;
    }

    assert_eq!(
        total, issue.face,
        "сума балансів ста власників не дорівнює пропозиції бонду"
    );

    // І дзеркально: у малому світі власник рівно один. Інакше «1» теж було б
    // лише назвою.
    let ledger: HolderCheckpoint = stored(small, &holder_pda(demo_issue(), holder_wallet(0)).0);
    assert_eq!(ledger.owner, anchor_key(holder_wallet(0)));
    assert_eq!(bond_balance(small, &holder_bond(0)), issue.face);
    assert!(
        small
            .get(&holder_pda(demo_issue(), holder_wallet(1)).0)
            .is_none(),
        "у світі з одним власником існує облік другого"
    );
}

// ---- 2. Набір акаунтів не росте --------------------------------------------

/// Причина, а не наслідок. Вартість перехоплення не залежить від кількості
/// власників тому, що подати їх туди нікуди: набір фіксований, і `FR-015`
/// лишає в ньому рівно джерело, випуск, два рахунки й мінт. Замір нижче показує
/// число; цей тест показує, чому воно таке — і почервоніє першим, щойно комусь
/// знадобиться провести власників крізь перехоплення.
#[test]
fn the_account_set_of_one_arrival_does_not_grow_with_the_owners() {
    let arrival = intercept_ix(true);

    assert_eq!(
        arrival.accounts.len(),
        8,
        "набір перехоплення змінився — замір 1 vs 100 треба переписати разом із ним"
    );

    for index in 0..HOLDERS {
        let owner = holder_wallet(index);
        let ledger = holder_pda(demo_issue(), owner).0;

        assert!(
            !arrival.accounts.iter().any(|meta| meta.pubkey == ledger),
            "облік власника {index} потрапив у набір перехоплення"
        );
        assert!(
            !arrival
                .accounts
                .iter()
                .any(|meta| meta.pubkey == holder_bond(index)),
            "рахунок бонду власника {index} потрапив у набір перехоплення"
        );
    }

    // Той самий набір їде і в CPI зі свопу: `Swap` веде трійцю
    // `issue`/`escrow_vault`/`bond_mint` як є, і власників у ній теж немає.
    let whole = swap_ix();
    for index in 0..HOLDERS {
        let ledger = holder_pda(demo_issue(), holder_wallet(index)).0;
        assert!(
            !whole.accounts.iter().any(|meta| meta.pubkey == ledger),
            "облік власника {index} потрапив у набір свопу"
        );
    }
}

// ---- 4. Скільки коштувало б протилежне -------------------------------------

/// Найважливіший замір файлу — той єдиний, у якому сто власників **справді
/// подані** в надходження.
///
/// Він відповідає на питання, від якого весь замір і залежить: що вважати
/// «випуском зі 100 власниками», якщо у перехопленні власники не беруть участі.
/// Тут вони беруть — їх сто акаунтів просто дописані в хвіст набору, і
/// інструкція їх навіть не читає. Одного лише завантаження вистачає, щоб
/// вартість зросла втричі: 10 852 CU проти 39 352 на момент заміру, тобто
/// близько 285 CU на кожного власника **до** будь-якої роботи з ним.
///
/// Звідси видно, чим насправді тримається `SC-005`. Не тим, що перехоплення
/// добре написане, і не щасливим збігом у межах 5%, а тим, що власники не
/// потрапляють у транзакцію взагалі: варто їм там опинитись — і бюджет
/// критерію перевищений у півсотні разів, ще до першого рядка коду, який з
/// ними щось робив би.
///
/// Обидві половини тесту важать однаково. Що прогін інертний — доводить, що
/// 285 CU на власника це **підлога**, ціна самої присутності; що різниця
/// величезна — доводить, що нуль у `SC-005` заміряний, а не вбудований.
#[test]
fn the_arrival_stays_cheap_only_because_the_owners_never_enter_it() {
    let world = funded_source(hundred_owners());

    let bare = run(&world, &intercept_ix(true));

    let mut loaded_ix = intercept_ix(true);
    for index in 0..HOLDERS {
        loaded_ix.accounts.push(AccountMeta::new_readonly(
            holder_pda(demo_issue(), holder_wallet(index)).0,
            false,
        ));
    }
    let loaded = run(&world, &loaded_ix);

    // Спершу — що сто дописаних акаунтів справді нічого не змінили. Інакше
    // порівнювались би два різні надходження, а не два способи подати одне.
    let after_bare: Issue = decode(&bare, &demo_issue());
    let after_loaded: Issue = decode(&loaded, &demo_issue());
    assert_eq!(after_bare.repaid_total, after_loaded.repaid_total);
    assert_eq!(after_bare.payout_index, after_loaded.payout_index);
    assert_eq!(
        token_balance(&bare, &ESCROW_VAULT),
        token_balance(&loaded, &ESCROW_VAULT),
        "сто дописаних обліків змінили розщеплення — інструкція їх читає"
    );

    let idle = bare.compute_units_consumed;
    let carried = loaded.compute_units_consumed;

    println!(
        "[SC-005] сто власників у наборі: {idle} CU проти {carried} CU, різниця {} bps \
         (≈{} CU на власника, і жодного рядка коду, який би з ним щось робив)",
        spread_bps(idle, carried),
        (carried - idle) / HOLDERS as u64
    );

    assert!(
        spread_bps(idle, carried) > BUDGET_BPS * 10,
        "подати сто обліків у надходження виявилось майже безкоштовно ({idle} проти {carried} CU) — \
         тоді `SC-005` тримається не на тому, що написано в шапці, і пояснення треба переписати"
    );
}

// ---- 5. Число --------------------------------------------------------------

/// `SC-005`: різниця у спожитих обчислювальних одиницях між випуском з 1 і зі
/// 100 власниками — менша за 5%.
///
/// Міряються два шляхи. Перший — саме перехоплення: це те, що робить протокол.
/// Другий — своп демо-емітента цілком, разом із CPI: це те, що платить емітент
/// за одне надходження комісії, і саме про нього говорить формулювання
/// критерію.
#[test]
fn one_arrival_costs_the_same_with_one_owner_and_with_a_hundred() {
    let small = funded_source(one_owner());
    let big = funded_source(hundred_owners());

    let split_1 = cu(&small, &intercept_ix(true));
    let split_100 = cu(&big, &intercept_ix(true));

    let arrival_1 = cu(one_owner(), &swap_ix());
    let arrival_100 = cu(hundred_owners(), &swap_ix());

    println!("[SC-005] перехоплення: 1 власник = {split_1} CU, 100 власників = {split_100} CU, різниця {} bps", spread_bps(split_1, split_100));
    println!("[SC-005] надходження цілком (своп + CPI): 1 власник = {arrival_1} CU, 100 власників = {arrival_100} CU, різниця {} bps", spread_bps(arrival_1, arrival_100));

    assert!(
        spread_bps(split_1, split_100) < BUDGET_BPS,
        "перехоплення: {split_1} проти {split_100} CU — більше за 5%"
    );
    assert!(
        spread_bps(arrival_1, arrival_100) < BUDGET_BPS,
        "надходження: {arrival_1} проти {arrival_100} CU — більше за 5%"
    );
}

// ---- 6. Прилад -------------------------------------------------------------

/// Нуль відсотків різниці означає нуль лише тоді, коли лічильник взагалі вміє
/// показувати різницю. Тому дві перевірки в одному тесті: той самий прогін у
/// тому самому світі дає **побітово** те саме число (отже шуму немає й «менше
/// за 5%» не куплене похибкою), а прогін, який справді інший, той самий
/// лічильник відрізняє з запасом у рази.
///
/// «Справді інший» тут — друга гілка того самого перехоплення: без випуску
/// дохід лише спостерігається (`FR-028`), без переказу й без індексу.
#[test]
fn the_meter_reads_the_run_and_not_a_constant() {
    let world = funded_source(hundred_owners());

    let first = cu(&world, &intercept_ix(true));
    let again = cu(&world, &intercept_ix(true));
    assert_eq!(
        first, again,
        "два однакові прогони дали різні числа — лічильник шумить, і бюджет 5% нічого не значить"
    );

    let mut watching = world.clone();
    // Спостереження без випуску: джерело вільне, трійця опційних акаунтів
    // порожня.
    watching.insert(demo_source(), anchor_account(&stored_source(None, 0)));
    let observed = cu(&watching, &intercept_ix(false));

    println!("[SC-005] роздільна здатність: розщеплення = {first} CU, спостереження = {observed} CU, різниця {} bps", spread_bps(first, observed));

    assert!(
        spread_bps(first, observed) > BUDGET_BPS * 4,
        "лічильник не розрізняє двох різних гілок ({first} проти {observed} CU) — на ньому не можна міряти 5%"
    );
}

// ---- Від чого відмовились --------------------------------------------------

/// Вартість, якої в надходженні немає.
///
/// Виплата справді коштує **на власника** — але платить її сам власник, у своїй
/// транзакції, коли захоче (`FR-015`, `FR-016`). Ця цифра стоїть поруч, щоб
/// нуль вище було з чим порівняти: без неї «різниця 0%» читається як «тут
/// нічого не коштує», а коштує — просто не тут і не тому, хто приніс комісію.
///
/// Замок: виплата одного власника має **порядок цілого надходження**, а не
/// дрібної добавки до нього. Саме тому економія від кумулятивного індексу
/// масштабується як кількість власників: розсилка ста коштувала б десятки
/// надходжень, а не відсотки одного. Якби виплата коштувала копійки, `SC-005`
/// не було б про що питати.
///
/// **Ця межа структурна, і це перевірено.** `claim` із порожнім тілом — усе
/// повернути одразу, нічого не порахувати й не переказати — коштує 7 916 CU з
/// 11 870, тобто дві третини вартості власника це завантаження й перевірка
/// його дев'яти акаунтів, а не арифметика виплати. Звідси два наслідки:
/// зробити виплату дешевшою оптимізацією тіла не вийде, і жодна мутація
/// **програми** цей замок не червонить — він тримається на розмірі набору, а
/// не на тому, що інструкція робить. Те саме число з іншого боку: ≈285 CU на
/// акаунт у тесті вище.
#[test]
fn the_payout_is_the_per_owner_cost_the_arrival_does_not_pay() {
    let world = funded_source(hundred_owners());

    // Спершу надходження — інакше в ескроу порожньо й забирати нічого.
    let context = at(&world);
    context.process_and_validate_instruction(&intercept_ix(true), &[Check::success()]);
    let after = context.account_store.borrow().clone();

    let payout = cu(&after, &claim_ix(0));
    let arrival = cu(&world, &intercept_ix(true));
    let pushing = payout * HOLDERS as u64;

    println!(
        "[SC-005] виплата одному власникові = {payout} CU; надходження = {arrival} CU; \
         розсилка ста власникам коштувала б {} надходжень",
        pushing / arrival
    );

    assert!(
        pushing >= arrival * (HOLDERS as u64 / 2),
        "виплата ({payout} CU) виявилась дрібницею проти надходження ({arrival} CU) — \
         тоді кумулятивний індекс економить сталий множник, а не порядок кількості власників, \
         і формулювання в шапці треба переписати"
    );
}
