//! `SC-003` і `SC-004` — чи сходяться гроші після довільної послідовності
//! операцій.
//!
//! Обидві вимоги міряються **одним прогоном і на одному корпусі**, бо так їх і
//! написано у `SPEC.md`: `SC-004` починається словами «на тому ж наборі».
//! Набір — тисяча послідовностей, кожна з яких є окремим життям одного випуску:
//! від одного до чотирьох власників із нерівними лотами, а далі випадковий потік
//! надходжень комісії, виплат і дострокових погашень у довільному порядку.
//!
//! **Що доводиться:**
//! - `SC-003` — сума всього виплаченого власникам і залишку в ескроу дорівнює
//!   сумі всього перехопленого, до найменшої одиниці USDC і **після кожного
//!   кроку**, а не лише наприкінці послідовності;
//! - `SC-004` — у ескроу жодного разу не потрапило більше за залишок
//!   зобов'язання, і жоден власник не забрав більше, ніж дає його лот.
//!
//! **Чого набір не доводить — і це головне, що треба про нього сказати.**
//! `SC-003` називає чотири операції: надходження, `claim`, **передача**,
//! **продаж**. Тут є перші дві. Передачі немає не тому, що її забули вписати в
//! тест, а тому, що Token-2022 сьогодні не пропускає жодної: гук стоїть у самому
//! мінті, а `execute` — це `T032`; продаж (`create_offer` / `buy_offer`) — це
//! `T034`…`T036`. Тому на `T039` лишається рівно та половина `SC-003`, яка
//! починається зі зміни власника:
//!
//! - **рівність через зміну власника.** Тут баланс кожного власника
//!   зафіксований до першого надходження (підписка живе в `Subscribing`,
//!   розщеплення — у `Repaying`), тому «частка власника» — стала. Після `T032`
//!   вона стає кусково-сталою в часі, і рівність `SC-003` мусить пережити саме
//!   цей злам;
//! - **`accrued` тут завжди нуль.** Поле, у яке гук складає нараховане при
//!   передачі (`FR-017`), у цьому корпусі не рухається жодного разу — отже
//!   доданок `accrued` у `math::claimable` і шлях «продав усе, але прийшов за
//!   нарахованим до продажу» цим набором не покриті нічим;
//! - **контракт `claim` із `T032`.** `claim` рахує претензію на **поточному**
//!   балансі, і це законно рівно доти, доки кожна передача лишає по собі
//!   чекпоінт. Сьогодні контракт виконано з запасом — передач немає взагалі, —
//!   і саме тому цей файл його не перевіряє: перевірити його може лише набір, у
//!   якому передачі є;
//! - **гроші вторинки.** Ціна оферти й торгова комісія (`FR-035`) — це третій
//!   потік USDC, якого в рівності вище немає.
//!
//! `SC-004` ділиться тією ж лінією: «більше за свою частку» тут міряється
//! сталим лотом, а після `T032` частку доведеться міряти в часі.
//!
//! **Як влаштована одна послідовність.** Створення випуску → від одного до
//! чотирьох власників, кожен зі своїм обліком і своїм нерівним лотом (останній
//! пропонує більше, ніж лишилось, щоб у корпусі був і частковий прийом
//! `FR-009`) → видача, після якої випуск у `Repaying` → 4…12 випадкових
//! операцій → **вимітання**: кожен власник забирає своє → і ще одне, яке не
//! мусить дати нікому нічого. Вимітання не випадкове, бо воно не операція, а
//! замір: після нього чекпоінти всіх власників стоять на поточному індексі, і
//! залишок в ескроу — це рівно відкинуте при діленні, тобто те, про що
//! `CLAUDE.md` каже «округлення завжди вниз, залишок лишається у сховищі».
//!
//! **Чому рівність не є тавтологією токен-програми.** Три числа в ній
//! виміряні з трьох різних боків і жодне не взяте зі стану випуску: скільки
//! перехоплено — з рахунків **емітента** (наскільки схуд рахунок джерела й
//! рахунок, з якого гаситься достроково), скільки виплачено — з рахунків
//! **власників**, залишок — з ескроу. Те, що програма записала у власні книги
//! (`repaid_total`, `claimed_total`), звіряється з цими числами окремим тестом:
//! книга, яка розійшлася з грошима, — це та сама розбіжність, яку рахує
//! `SC-003`.
//!
//! **Чого замір не доводить, крім передач.** Рівність `SC-003` тримається
//! складом наборів акаунтів: у цих чотирьох інструкцій ескроу має рівно трьох
//! контрагентів, і всі три виміряні, тому жодна з одинадцяти мутацій набору не
//! змогла її зламати — ламались сусідні замки. Що саме стереже арифметику,
//! написано в доці до кожного з них.
//!
//! Замір знятий у mollusk — без черг, конкуренції за
//! стан і без плати за розмір транзакції; це та сама межа демо на змішаних
//! даних, що в `SPEC.md` → Припущення. І корпус детермінований: тисяча
//! послідовностей — це тисяча **різних** послідовностей, а не тисяча випадкових
//! чисел, які змінюються від прогону до прогону. Ціна відома: набір ловить те,
//! що в ньому є, і жодна властивість «на всіх входах» тут не доведена.

#[path = "harness.rs"]
mod harness;

use {
    anchor_lang::{AccountDeserialize, InstructionData},
    anchor_spl::token_2022::spl_token_2022::{
        extension::StateWithExtensions,
        state::{Account as TokenState, Mint as MintState},
    },
    daddys_club::{
        errors::ClubError,
        instructions::issue::IssueParams,
        state::{HolderCheckpoint, Issue, IssueState},
    },
    harness::*,
    mollusk_svm::{
        account_store::AccountStore,
        result::{InstructionResult, ProgramResult},
        MolluskContext,
    },
    solana_account::Account,
    solana_address::Address as Pubkey,
    solana_instruction::{AccountMeta, Instruction},
    solana_program_error::ProgramError,
    std::{collections::HashMap, sync::OnceLock},
};

/// Розмір корпусу. Число зі `SC-003` («на наборі з ≥1000 випадкових
/// послідовностей»), а не кругле «щоб було».
const SEQUENCES: usize = 1_000;

/// Скільки власників може мати випуск. Верхня межа — не про реалізм, а про
/// час збірки корпусу: те, що ділиться між чотирма нерівними лотами, ділиться
/// між будь-якими; скільки коштує сотня власників, заміряно в `tests/compute.rs`.
const MAX_HOLDERS: usize = 4;

/// Довжина випадкової частини послідовності.
const MIN_OPS: u64 = 4;
const MAX_OPS: u64 = 12;

/// Зерно корпусу. Фіксоване: набір, який змінюється від прогону до прогону,
/// одного дня падає сам і забирає з собою послідовність, на якій упав.
const SEED: u64 = 0x0DDD_C1CB_2026_0827;

const ISSUER_USDC: Pubkey = Pubkey::new_from_array([61u8; 32]);

/// Скільки лежить на рахунку джерела до першого надходження і скільки має
/// емітент на дострокове погашення. Через перехоплення звідси не може піти
/// більше за зобов'язання найбільшого випуску в корпусі (4 000 000 USDC плюс
/// купон), тому обидва рахунки наповнені з запасом у чотири рази.
///
/// Виручка з видачі теж лягає на рахунок емітента, але покладатись на неї не
/// можна: у послідовності, яка гаситься першим же кроком, залишок зобов'язання
/// більший за виручку рівно на купон і на origination fee.
const VAULT_FUNDS: u64 = 20_000_000_000_000;
const PREPAY_FUNDS: u64 = 20_000_000_000_000;

const MINT_SUPPLY: u64 = 1_000_000_000_000_000;

type Store = HashMap<Pubkey, Account>;

// ---- Генератор -------------------------------------------------------------

/// SplitMix64. Свій, а не з крейта: `proptest` тягне за собою дерево
/// залежностей заради генератора на десять рядків, а пін
/// `spl-list-view` у `CLAUDE.md` вимагає, щоб кожна нова гілка графа мала
/// причину. Причина тут лише одна — відтворюваність, і вона забезпечується
/// зерном.
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Число в `0..bound`. Зсув на межі дільника тут не важить: корпус має бути
    /// різноманітним, а не рівномірним.
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    /// Число в `low..=high`.
    fn between(&mut self, low: u64, high: u64) -> u64 {
        low + self.below(high - low + 1)
    }
}

/// Операція випадкової частини послідовності.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Op {
    /// Надходження комісії: своп емітента, з якого відщеплюється частка.
    Arrival(u64),
    /// Виплата власникові за його номером.
    Claim(usize),
    /// Дострокове погашення залишку (`FR-021`).
    Prepay,
}

/// Надходження комісії. Чотири величини, і кожна ловить своє: копійчана
/// округлюється в нуль (12% від 8 — це 0, і в ескроу не йде нічого), дві
/// середні зараховуються цілими, велика впирається в стелю залишку (`FR-020`).
/// Три з чотирьох міряються від номіналу, інакше на випуску в десятки USDC
/// кожне перше надходження гасило б його цілком, а на випуску в мільйони
/// жодне не зрушило б нічого.
fn inflow(rng: &mut Rng, face: u64) -> u64 {
    match rng.below(10) {
        0..=1 => rng.between(1, 8),
        2..=4 => rng.between(1, (face / 1_000).max(1)),
        5..=7 => rng.between(1, (face / 10).max(1)),
        _ => rng.between(1, face.saturating_mul(10)),
    }
}

/// Випадкова частина послідовності. Стан випуску генератор не читає навмисно:
/// операція, яка приходить не вчасно, — це рівно те, що протокол мусить
/// пережити, і саме на ній видно, чи не зрушив хтось лічильники дарма.
fn script(rng: &mut Rng, holders: usize, face: u64) -> Vec<Op> {
    let count = rng.between(MIN_OPS, MAX_OPS);

    (0..count)
        .map(|_| match rng.below(100) {
            0..=59 => Op::Arrival(inflow(rng, face)),
            60..=89 => Op::Claim(rng.below(holders as u64) as usize),
            _ => Op::Prepay,
        })
        .collect()
}

/// Розклад номіналу на лоти. Рівних часток тут немає навмисно: саме на
/// нерівних лотах ділення індексу лишає залишок, і `SC-003` мусить зійтися
/// разом із ним, а не всупереч йому.
fn deal(rng: &mut Rng, holders: usize, face: u64) -> Vec<u64> {
    let min_lot = min_lot();
    let mut lots = Vec::with_capacity(holders);
    let mut left = face;

    for index in 0..holders {
        let rest = (holders - index - 1) as u64;
        if rest == 0 {
            lots.push(left);
            break;
        }
        // Стеля береться так, щоб решті власників лишилось хоча б по лоту:
        // підписка нижче мінімального лота відмовляє (`FR-008`).
        let lot = rng.between(min_lot, left - rest * min_lot);
        lots.push(lot);
        left -= lot;
    }

    lots
}

// ---- Гаманці власників -----------------------------------------------------
//
// Ті самі три ролі, що в `tests/compute.rs`, і дерівуються так само: чотири
// адреси константами написати можна, але тоді їх довелось би переписувати
// разом із `MAX_HOLDERS`.

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

// ---- Інструкції ------------------------------------------------------------
//
// Мети тут написані руками, як у `tests/invest.rs` і `tests/issue.rs`, а не
// зібрані зі згенерованих Anchor'ом структур, як у `tests/compute.rs`. Різниця
// має причину: там набір акаунтів був предметом заміру, і прибити його до
// програми було сенсом тесту. Тут предмет інший — гроші, — і жоден замок цього
// файлу на складі набору не стоїть.

/// Умови випуску. Номінал приходить ззовні, бо він єдиний, що змінюється від
/// послідовності до послідовності; решта — картка M0.
fn terms(face: u64) -> IssueParams {
    let issue = stored_issue(IssueState::Subscribing, 0);

    IssueParams {
        face,
        coupon_bps: issue.coupon_bps,
        pledge_bps: issue.pledge_bps,
        maturity_ts: issue.maturity_ts,
        subscription_end_ts: issue.subscription_end_ts,
        min_lot: issue.min_lot,
    }
}

/// Мінімальний лот і частка перехоплення від номіналу не залежать: обидва —
/// з тієї ж картки M0.
fn min_lot() -> u64 {
    stored_issue(IssueState::Subscribing, 0).min_lot
}

fn pledge_bps() -> u16 {
    stored_issue(IssueState::Subscribing, 0).pledge_bps
}

/// Номінал випуску — від десятків USDC до мільйонів, п'ять порядків величини.
/// Це не декорація: номінал дорівнює пропозиції бонду, тобто знаменнику
/// індексу виплати, і саме він вирішує, на якому масштабі видно округлення.
/// Тисяча послідовностей з однаковим номіналом була б тисячею замірів одного
/// й того самого масштабу. Будується з лотів, бо `FR-001` вимагає, щоб номінал
/// розкладався на цілі лоти.
fn face(rng: &mut Rng) -> u64 {
    let lots = match rng.below(10) {
        0..=2 => rng.between(8, 50),
        3..=6 => rng.between(1_000, 500_000),
        _ => rng.between(1_000_000, 4_000_000),
    };

    lots * min_lot()
}

fn create_issue_ix(face: u64) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::CreateIssue {
            seq: ISSUE_SEQ,
            params: terms(face),
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

fn open_ix(index: usize) -> Instruction {
    let owner = holder_wallet(index);

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

/// Перехоплення напряму: підпис PDA пулу mollusk приймає за метаданими. Шлях
/// вужчий за справжній — там підпис ставить сама програма-емітент зсередини
/// CPI, — але гроші рухаються тим самим кодом, а предмет цього файлу саме вони.
/// Що CPI-шлях доходить сюди цілим, показують `tests/swap.rs` і `T024`.
fn arrival_ix(amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Intercept { amount }.data(),
        vec![
            AccountMeta::new(demo_source(), false),
            AccountMeta::new_readonly(issuer_authority().0, true),
            AccountMeta::new(SOURCE_VAULT, false),
            AccountMeta::new(demo_issue(), false),
            AccountMeta::new(ESCROW_VAULT, false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

fn claim_ix(index: usize) -> Instruction {
    let owner = holder_wallet(index);

    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Claim {}.data(),
        vec![
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(holder_pda(demo_issue(), owner).0, false),
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new(holder_usdc(index), false),
            AccountMeta::new(ESCROW_VAULT, false),
            AccountMeta::new_readonly(holder_bond(index), false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

fn prepay_ix() -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Prepay {}.data(),
        vec![
            AccountMeta::new(demo_issue(), false),
            AccountMeta::new_readonly(demo_source(), false),
            AccountMeta::new_readonly(ISSUER, true),
            AccountMeta::new(ISSUER_USDC, false),
            AccountMeta::new(ESCROW_VAULT, false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

fn ix_for(op: Op) -> Instruction {
    match op {
        Op::Arrival(amount) => arrival_ix(amount),
        Op::Claim(index) => claim_ix(index),
        Op::Prepay => prepay_ix(),
    }
}

// ---- Стенд -----------------------------------------------------------------

/// Чим скінчився крок.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Outcome {
    Ok,
    /// Іменована відмова: код, який програма назвала сама.
    Refused(u32),
    /// Будь-що інше — відмова токен-програми, обмеження Anchor, аварія SVM.
    Broke(String),
}

impl Outcome {
    fn of(result: &InstructionResult) -> Self {
        match &result.program_result {
            ProgramResult::Success => Self::Ok,
            ProgramResult::Failure(ProgramError::Custom(code)) => Self::Refused(*code),
            other => Self::Broke(format!("{other:?}")),
        }
    }
}

fn code(error: ClubError) -> u32 {
    u32::from(error)
}

/// Один SVM на весь корпус: `Mollusk::new` щоразу вантажить `.so` з диска, а
/// тисяча послідовностей — це тисяча таких завантажень. Стан між
/// послідовностями не тече: сховище акаунтів підмінюється цілим, а програми й
/// сисвари контекст підкладає сам на кожному виклику.
struct Bench {
    context: MolluskContext<Store>,
}

impl Bench {
    fn new() -> Self {
        Self {
            context: setup().with_context(Store::new()),
        }
    }

    fn load(&self, store: Store) {
        *self.context.account_store.borrow_mut() = store;
    }

    fn put(&self, key: Pubkey, account: Account) {
        self.context
            .account_store
            .borrow_mut()
            .store_account(key, account);
    }

    fn send(&self, ix: &Instruction) -> Outcome {
        Outcome::of(&self.context.process_instruction(ix))
    }

    fn account(&self, key: &Pubkey) -> Account {
        self.context
            .account_store
            .borrow()
            .get_account(key)
            .unwrap_or_else(|| panic!("акаунт {key} є у світі"))
    }

    fn state<T: AccountDeserialize>(&self, key: &Pubkey) -> T {
        T::try_deserialize(&mut self.account(key).data.as_slice()).expect("акаунт розбирається")
    }

    fn balance(&self, key: &Pubkey) -> i128 {
        let account = self.account(key);
        let amount = StateWithExtensions::<TokenState>::unpack(&account.data)
            .expect("рахунок розпаковується")
            .base
            .amount;

        i128::from(amount)
    }

    fn supply(&self, key: &Pubkey) -> u64 {
        let account = self.account(key);

        StateWithExtensions::<MintState>::unpack(&account.data)
            .expect("мінт розпаковується")
            .base
            .supply
    }
}

/// Світ до створення випуску: протокол, джерело, гаманець емітента і рахунок,
/// на який приходить комісія. Демо-пул сюди не піднімається — свопу в цьому
/// файлі немає, а перехоплення потребує від його PDA лише підпису.
fn base_store() -> Store {
    let pool = issuer_authority().0;
    let mut store = Store::new();

    store.insert(config_pda().0, anchor_account(&stored_config()));
    store.insert(demo_source(), anchor_account(&stored_source(None, 0)));
    store.insert(ISSUER, wallet());
    store.insert(ISSUER_USDC, usdc_account(ISSUER, PREPAY_FUNDS));
    store.insert(FEE_VAULT, usdc_account(ADMIN, 0));
    store.insert(USDC_MINT, usdc_mint(MINT_SUPPLY));
    store.insert(pool, uninitialized());
    store.insert(SOURCE_VAULT, token_account(USDC_MINT, pool, VAULT_FUNDS));

    store
}

// ---- Слід послідовності ----------------------------------------------------

/// Де в послідовності стоїть крок. Випадкова частина — це набір зі `SC-003`;
/// вимітання — замір після нього.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Випадкова частина.
    Script,
    /// Кожен власник забирає своє.
    Sweep,
    /// І ще раз — після вимітання не мусить лишитись нічого.
    Rinse,
}

#[derive(Clone, Debug)]
struct Step {
    phase: Phase,
    op: Op,
    outcome: Outcome,
    /// Скільки USDC пішло з рахунків емітента (надходження або погашення).
    from_issuer: i128,
    /// Скільки USDC прийшло власникові.
    to_owner: i128,
    /// Залишок зобов'язання **до** кроку.
    owed_before: i128,
    /// Баланс ескроу **після** кроку.
    escrow_after: i128,
    /// Обидві сторони рівності `SC-003`, накопичені на цей момент.
    credited: i128,
    paid: i128,
}

#[derive(Clone, Debug)]
struct Sequence {
    seed: u64,
    lots: Vec<u64>,
    script: Vec<Op>,
    steps: Vec<Step>,
    /// Крок, який відмовив так, як не передбачено, і обірвав послідовність.
    broke_at: Option<String>,
    supply: u64,
    obligation: i128,
    state: IssueState,
    /// Що записала собі програма.
    repaid_total: i128,
    claimed_by: Vec<i128>,
    /// Що виміряно по рахунках.
    paid_to: Vec<i128>,
    escrow_final: i128,
}

impl Sequence {
    fn holders(&self) -> usize {
        self.lots.len()
    }

    fn credited(&self) -> i128 {
        self.steps.last().map_or(0, |step| step.credited)
    }

    fn paid(&self) -> i128 {
        self.steps.last().map_or(0, |step| step.paid)
    }

    fn steps_in(&self, phase: Phase) -> impl Iterator<Item = &Step> + '_ {
        self.steps.iter().filter(move |step| step.phase == phase)
    }
}

/// Послідовність, яка не дійшла до кінця: щось у побудові світу або в кроці
/// відмовило так, як не передбачено. Порожній слід — це і є сигнал, і його
/// ловить `no_step_of_any_sequence_is_refused_for_a_reason_that_was_not_named`.
fn aborted(seed: u64, lots: Vec<u64>, script: Vec<Op>, at: String) -> Sequence {
    Sequence {
        seed,
        lots,
        script,
        steps: Vec::new(),
        broke_at: Some(at),
        supply: 0,
        obligation: 0,
        state: IssueState::Subscribing,
        repaid_total: 0,
        claimed_by: Vec::new(),
        paid_to: Vec::new(),
        escrow_final: 0,
    }
}

/// Одна послідовність від створення випуску до останньої виплати.
fn play(bench: &Bench, seed: u64) -> Sequence {
    let mut rng = Rng::new(seed);
    let holders = rng.between(1, MAX_HOLDERS as u64) as usize;
    let face = face(&mut rng).max(min_lot() * holders as u64);
    let lots = deal(&mut rng, holders, face);
    let script = script(&mut rng, holders, face);
    // Останній підписник пропонує більше, ніж лишилось: `FR-009` приймає
    // `min(запропоноване, залишок)`, і надлишок мусить лишитись у нього на
    // рахунку, а не піти у сховище.
    let over_offer = rng.between(1, min_lot() * 10);

    bench.load(base_store());
    for (index, lot) in lots.iter().enumerate() {
        let owner = holder_wallet(index);
        let offer = if index + 1 == holders {
            lot + over_offer
        } else {
            *lot
        };

        bench.put(owner, wallet());
        bench.put(holder_usdc(index), usdc_account(owner, offer));
        bench.put(holder_bond(index), bond_account(owner, 0));
    }

    // Побудова світу — не частина випадкового набору, але й не привід
    // панікувати: відмова тут означає, що зламалась одна з інструкцій, і це
    // рівно те, про що мусить сказати тест, а не стек виклику.
    let outcome = bench.send(&create_issue_ix(face));
    if outcome != Outcome::Ok {
        return aborted(seed, lots, script, format!("create_issue: {outcome:?}"));
    }

    for (index, lot) in lots.iter().enumerate() {
        let offer = if index + 1 == holders {
            lot + over_offer
        } else {
            *lot
        };

        let outcome = bench.send(&open_ix(index));
        if outcome != Outcome::Ok {
            return aborted(
                seed,
                lots,
                script,
                format!("open_position {index}: {outcome:?}"),
            );
        }

        let outcome = bench.send(&subscribe_ix(index, offer));
        if outcome != Outcome::Ok {
            return aborted(
                seed,
                lots,
                script,
                format!("subscribe {index}: {outcome:?}"),
            );
        }
    }

    let outcome = bench.send(&withdraw_ix());
    if outcome != Outcome::Ok {
        return aborted(
            seed,
            lots,
            script,
            format!("withdraw_proceeds: {outcome:?}"),
        );
    }

    // Звідси починається замір. Усе, що прийшло раніше, — це номінал, і до
    // рівності `SC-003` він не належить: вона про перехоплений дохід.
    let supply = bench.supply(&BOND_MINT);
    let escrow_start = bench.balance(&ESCROW_VAULT);

    let plan = script
        .iter()
        .map(|op| (Phase::Script, *op))
        .chain((0..holders).map(|index| (Phase::Sweep, Op::Claim(index))))
        .chain((0..holders).map(|index| (Phase::Rinse, Op::Claim(index))));

    let mut steps: Vec<Step> = Vec::new();
    let mut paid_to = vec![0i128; holders];
    let mut credited = 0i128;
    let mut paid = 0i128;

    for (phase, op) in plan {
        let issue: Issue = bench.state(&demo_issue());
        let owed_before = i128::from(issue.obligation_total) - i128::from(issue.repaid_total);

        let issuer_before = bench.balance(&SOURCE_VAULT) + bench.balance(&ISSUER_USDC);
        let owner_before = match op {
            Op::Claim(index) => bench.balance(&holder_usdc(index)),
            _ => 0,
        };

        let outcome = bench.send(&ix_for(op));

        let issuer_after = bench.balance(&SOURCE_VAULT) + bench.balance(&ISSUER_USDC);
        let from_issuer = issuer_before - issuer_after;
        let to_owner = match op {
            Op::Claim(index) => bench.balance(&holder_usdc(index)) - owner_before,
            _ => 0,
        };

        if let Op::Claim(index) = op {
            paid_to[index] += to_owner;
        }
        credited += from_issuer;
        paid += to_owner;

        steps.push(Step {
            phase,
            op,
            outcome,
            from_issuer,
            to_owner,
            owed_before,
            escrow_after: bench.balance(&ESCROW_VAULT) - escrow_start,
            credited,
            paid,
        });
    }

    let issue: Issue = bench.state(&demo_issue());
    let claimed_by = (0..holders)
        .map(|index| {
            let holder: HolderCheckpoint =
                bench.state(&holder_pda(demo_issue(), holder_wallet(index)).0);

            i128::from(holder.claimed_total)
        })
        .collect();

    Sequence {
        seed,
        lots,
        script,
        steps,
        broke_at: None,
        supply,
        obligation: i128::from(issue.obligation_total),
        state: issue.state,
        repaid_total: i128::from(issue.repaid_total),
        claimed_by,
        paid_to,
        escrow_final: bench.balance(&ESCROW_VAULT) - escrow_start,
    }
}

/// Корпус будується один раз на бінарник: тисяча послідовностей — це десятки
/// тисяч справжніх інструкцій, а слід із них клонується задарма.
fn corpus() -> &'static Vec<Sequence> {
    static CORPUS: OnceLock<Vec<Sequence>> = OnceLock::new();

    CORPUS.get_or_init(|| {
        let bench = Bench::new();
        let mut master = Rng::new(SEED);

        (0..SEQUENCES)
            .map(|_| play(&bench, master.next()))
            .collect()
    })
}

// ---- 1. Корпус ------------------------------------------------------------

/// Підпора під усі інші тести. «Тисяча випадкових послідовностей» — це не назва
/// змінної: послідовностей мусить бути тисяча, вони мусять бути різні, і в них
/// мусить траплятись те, заради чого набір узагалі ганяється. Без цього тесту
/// решта файлу могла б доводити нуль розбіжностей на тисячі однакових прогонів,
/// у кожному з яких не відбувається нічого.
#[test]
fn a_thousand_sequences_are_a_thousand_different_sequences() {
    let corpus = corpus();
    assert_eq!(corpus.len(), SEQUENCES, "корпус не того розміру");

    let shapes: std::collections::HashSet<Vec<Op>> =
        corpus.iter().map(|run| run.script.clone()).collect();
    assert_eq!(
        shapes.len(),
        SEQUENCES,
        "у корпусі є дві однакові послідовності — генератор повторюється"
    );

    // Форма корпусу прибита числами, а не нерівностями: вона не залежить від
    // програми — генератор її будує з самого зерна, — тому будь-яка правка
    // генератора мусить бути видна як зміна цифри, а не як тихо інший набір.
    let mut with_holders = [0usize; MAX_HOLDERS + 1];
    for run in corpus {
        with_holders[run.holders()] += 1;
    }
    assert_eq!(
        with_holders,
        [0, 233, 257, 242, 268],
        "розклад по власниках"
    );

    // Номінали мусять розійтись на порядки, інакше корпус міряє один масштаб.
    let smallest = corpus.iter().map(|run| run.supply).min().unwrap_or(0);
    let largest = corpus.iter().map(|run| run.supply).max().unwrap_or(0);
    assert!(
        smallest > 0 && largest / smallest >= 1_000,
        "номінали в корпусі не розійшлись: від {smallest} до {largest}"
    );

    let ops = || corpus.iter().flat_map(|run| &run.script);
    let arrivals = ops().filter(|op| matches!(op, Op::Arrival(_))).count();
    let claims = ops().filter(|op| matches!(op, Op::Claim(_))).count();
    let prepays = ops().filter(|op| matches!(op, Op::Prepay)).count();
    assert_eq!((arrivals, claims, prepays), (4_856, 2_452, 780));

    // Кроків більше, ніж операцій: після випадкової частини кожну послідовність
    // домітають двічі по одній виплаті на власника.
    let steps: usize = corpus.iter().map(|run| run.steps.len()).sum();
    assert_eq!(
        steps,
        arrivals + claims + prepays + 2 * corpus.iter().map(Sequence::holders).sum::<usize>()
    );
    assert_eq!(steps, 13_178, "корпус не тієї довжини");

    // Копійчані надходження рахуються так само з форми: 12% від суми, меншої за
    // дев'ять, — це нуль, і в ескроу з такого надходження не йде нічого.
    let dust = ops()
        .filter(|op| match op {
            Op::Arrival(amount) => {
                *amount > 0 && i128::from(*amount) * i128::from(pledge_bps()) / 10_000 == 0
            }
            _ => false,
        })
        .count();
    assert_eq!(dust, 994, "копійчаних надходжень у корпусі не стільки");

    // Далі — не форма, а те, що з неї вийшло. Ці числа залежать від програми,
    // тому тут стоять межі: тест мусить червоніти від порожнього корпусу, а не
    // від того, що виплата стала на одиницю іншою.
    let mut capped = 0; // надходження, яке вперлось у залишок зобов'язання
    let mut after_repaid = 0; // надходження на вже погашений випуск
    let mut paid_claims = 0; // виплата, яка щось дала
    let mut empty_claims = 0; // виплата, по якій нічого не належало
    let mut prepaid = 0; // дострокове погашення, яке пройшло
    let mut late_prepay = 0; // і яке прийшло на вже погашений випуск

    for run in corpus {
        for step in run.steps_in(Phase::Script) {
            match step.op {
                Op::Arrival(amount) => {
                    let share = i128::from(amount) * i128::from(pledge_bps()) / 10_000;
                    if step.owed_before > 0 && share > step.owed_before {
                        capped += 1;
                    }
                    if step.owed_before == 0 && share > 0 {
                        after_repaid += 1;
                    }
                }
                Op::Claim(_) => {
                    if step.to_owner > 0 {
                        paid_claims += 1;
                    } else {
                        empty_claims += 1;
                    }
                }
                Op::Prepay => {
                    if step.outcome == Outcome::Ok {
                        prepaid += 1;
                    } else {
                        late_prepay += 1;
                    }
                }
            }
        }
    }

    assert!(
        capped > 0,
        "у корпусі немає надходження, що вперлось у залишок (FR-020)"
    );
    assert!(
        after_repaid > 0,
        "у корпусі немає надходження на вже погашений випуск (FR-019)"
    );
    assert!(
        paid_claims > 0,
        "у корпусі немає жодної виплати, яка щось дала"
    );
    assert!(
        empty_claims > 0,
        "у корпусі немає виплати, по якій нічого не належало"
    );
    assert!(prepaid > 0, "у корпусі немає дострокового погашення");
    assert!(
        late_prepay > 0,
        "у корпусі немає погашення вже погашеного випуску"
    );

    let repaid = corpus
        .iter()
        .filter(|run| run.state == IssueState::Repaid)
        .count();
    assert!(
        repaid > 0,
        "жоден випуск у корпусі не дійшов до повного погашення"
    );
    assert!(
        repaid < SEQUENCES,
        "усі випуски погашено — у корпусі немає живого зобов'язання"
    );

    // І залишок від ділення: те, заради чого `CLAUDE.md` вимагає округлення
    // вниз. Після вимітання забирати вже нічого, а в ескроу щось лежить.
    let dusty = corpus
        .iter()
        .filter(|run| run.escrow_final > 0 && run.credited() > 0)
        .count();
    assert!(
        dusty > 0,
        "у жодній послідовності в ескроу не лишилось відкинутого при діленні"
    );
}

// ---- 2. `SC-003` -----------------------------------------------------------

/// `SC-003`: «сума всього виплаченого власникам і залишку в ескроу дорівнює
/// сумі всього перехопленого, з точністю до найменшої одиниці USDC».
///
/// Три числа виміряні з трьох різних боків і жодне не взяте зі стану випуску:
/// перехоплене — наскільки схудли рахунки емітента, виплачене — наскільки
/// потовщали рахунки власників, залишок — скільки лежить в ескроу. Рівність
/// перевіряється **після кожного кроку**: набір, у якому вона зійшлась лише
/// наприкінці, пропустив би дірку, що затягнулась наступною операцією.
///
/// **Межа цього замка, і вона така сама, як у `SC-005`.** З одинадцяти мутацій
/// набору цей тест не почервонів **на жодній** — червоніли сусіди. Причина не в
/// тому, що мутацій замало, а в тому, на чому замок стоїть: у наборах акаунтів
/// цих чотирьох інструкцій ескроу має рівно трьох контрагентів — рахунок
/// джерела, рахунок емітента і рахунок власника, — і всі три тут виміряні.
/// Зламати рівність могла б лише інструкція, яка провела б гроші повз них, а
/// подати такий акаунт нікуди. Тобто «нічого не зникло» протокол виконує
/// **складом наборів акаунтів**, а не арифметикою; арифметику стережуть
/// `the_escrow_never_takes_more_than_the_obligation_still_owes`,
/// `no_owner_ever_takes_more_than_the_share_his_lot_entitles_him_to`,
/// `the_books_the_program_keeps_agree_with_the_money_that_moved` і
/// `the_same_money_is_never_claimed_twice`, і в кожного з них своя мутація.
/// Замок лишається тому, що `SC-003` просить саме цю цифру, а не аргумент про
/// набори акаунтів. Це та сама властивість, що в `tests/compute.rs`, де
/// вартість власника виявилась структурною.
#[test]
fn nothing_is_created_and_nothing_is_lost_at_any_step_of_any_sequence() {
    let mut checked = 0usize;

    for run in corpus() {
        for (position, step) in run.steps.iter().enumerate() {
            assert_eq!(
                step.credited,
                step.paid + step.escrow_after,
                "seed {:#x}, крок {position} ({:?}): перехоплено {}, виплачено {}, в ескроу {}",
                run.seed,
                step.op,
                step.credited,
                step.paid,
                step.escrow_after
            );
            checked += 1;
        }

        assert_eq!(
            run.credited(),
            run.paid() + run.escrow_final,
            "seed {:#x}: рівність не зійшлась наприкінці послідовності",
            run.seed
        );
    }

    assert!(
        checked >= SEQUENCES * (MIN_OPS as usize + 2),
        "перевірено замало кроків: {checked}"
    );
}

// ---- 3. `SC-004`, перша половина -------------------------------------------

/// `SC-004`: «0 випадків, коли в ескроу потрапило більше за залишок
/// зобов'язання».
///
/// Міряється на кожному кроці окремо, бо саме покроково це й порушується:
/// надходження, більше за залишок, мусить віддати в ескроу рівно залишок і ні
/// одиницею більше (`FR-020`), а на погашеному випуску — нічого (`FR-019`).
/// Накопичена перевірка нижче — друга сторона того самого: виплачене ніколи не
/// переростає зобов'язання.
#[test]
fn the_escrow_never_takes_more_than_the_obligation_still_owes() {
    for run in corpus() {
        for (position, step) in run.steps.iter().enumerate() {
            assert!(
                step.from_issuer >= 0,
                "seed {:#x}, крок {position}: з рахунків емітента гроші не пішли, а прийшли",
                run.seed
            );
            assert!(
                step.from_issuer <= step.owed_before,
                "seed {:#x}, крок {position} ({:?}): в ескроу пішло {}, а зобов'язання винне {}",
                run.seed,
                step.op,
                step.from_issuer,
                step.owed_before
            );
        }

        assert!(
            run.credited() <= run.obligation,
            "seed {:#x}: перехоплено {} при зобов'язанні {}",
            run.seed,
            run.credited(),
            run.obligation
        );

        // `FR-019`: погашений випуск закривається рівно на зобов'язанні, а не
        // «десь біля».
        if run.state == IssueState::Repaid {
            assert_eq!(
                run.credited(),
                run.obligation,
                "seed {:#x}: випуск у Repaid, але перехоплено не все зобов'язання",
                run.seed
            );
        } else {
            assert!(
                run.credited() < run.obligation,
                "seed {:#x}: зобов'язання виплачене, але випуск не в Repaid",
                run.seed
            );
        }
    }
}

// ---- 4. `SC-004`, друга половина -------------------------------------------

/// `SC-004`: «0 випадків, коли власник забрав більше за свою частку».
///
/// «Своя частка» тут — не те, що порахував індекс: індексом міряти індекс
/// означало б перевірити його самим собою. Частка береться з двох чисел, які
/// індексу не належать: скільки всього зайшло в ескроу і яку частину
/// пропозиції бонду тримає власник. Округлення вниз (`CLAUDE.md`) робить
/// нерівність нестрогою в один бік і тільки в один: забрати менше за свою
/// частку можна завжди, більше — ніколи.
///
/// Ця форма законна саме тому, що передач у наборі немає: баланс кожного
/// власника зафіксований до першого надходження. З `T032` частка стане
/// величиною в часі, і `T039` доведеться рахувати її по відрізках між
/// чекпоінтами.
#[test]
fn no_owner_ever_takes_more_than_the_share_his_lot_entitles_him_to() {
    for run in corpus() {
        assert_eq!(
            run.lots.iter().sum::<u64>(),
            run.supply,
            "seed {:#x}: сума лотів не дорівнює пропозиції бонду",
            run.seed
        );

        for (index, lot) in run.lots.iter().enumerate() {
            let share = run.credited() * i128::from(*lot) / i128::from(run.supply);

            assert!(
                run.paid_to[index] <= share,
                "seed {:#x}: власник {index} забрав {}, а його лот дає щонайбільше {share}",
                run.seed,
                run.paid_to[index]
            );
            assert!(
                run.paid_to[index] >= 0,
                "seed {:#x}: власник {index} віддав гроші назад",
                run.seed
            );
        }

        assert!(
            run.paid() <= run.credited(),
            "seed {:#x}: власники разом забрали більше, ніж зайшло в ескроу",
            run.seed
        );
    }
}

// ---- 5. Книга проти грошей -------------------------------------------------

/// Що робить рівність `SC-003` не тавтологією токен-програми.
///
/// Три числа в ній виміряні по рахунках, і самі по собі вони зійшлися б і в
/// протоколі, який веде облік навмання: скільки з рахунку пішло, стільки на
/// інший і прийшло — це властивість Token-2022, а не наша. Доводить тут інше:
/// що **книги програми** кажуть про ті самі гроші. `repaid_total` — це те, з
/// чого рахується залишок зобов'язання, а `claimed_total` — те, що побачить
/// власник; книга, яка розійшлась із рахунком, і є розбіжність, яку рахує
/// `SC-003`.
#[test]
fn the_books_the_program_keeps_agree_with_the_money_that_moved() {
    for run in corpus() {
        assert_eq!(
            run.repaid_total,
            run.credited(),
            "seed {:#x}: випуск записав погашеним {}, а з рахунків емітента пішло {}",
            run.seed,
            run.repaid_total,
            run.credited()
        );

        for (index, claimed) in run.claimed_by.iter().enumerate() {
            assert_eq!(
                *claimed, run.paid_to[index],
                "seed {:#x}: облік власника {index} каже {claimed}, рахунок — {}",
                run.seed, run.paid_to[index]
            );
        }
    }
}

// ---- 6. Відмови ------------------------------------------------------------

/// Жодна операція не відмовляє з причини, якої тут не назвали.
///
/// У випадковому наборі операція приходить не вчасно постійно: виплата, по
/// якій нічого не належить, погашення вже погашеного випуску. Це нормальні
/// відмови, і кожна має ім'я. Усе інше — обмеження Anchor, відмова
/// токен-програми, аварія SVM — означає, що протокол спіткнувся на порядку
/// операцій, а не відмовив у ньому.
#[test]
fn no_step_of_any_sequence_is_refused_for_a_reason_that_was_not_named() {
    for run in corpus() {
        assert!(
            run.broke_at.is_none(),
            "seed {:#x}: послідовність обірвалась — {}",
            run.seed,
            run.broke_at.clone().unwrap_or_default()
        );

        for (position, step) in run.steps.iter().enumerate() {
            let allowed: &[u32] = match step.op {
                // Перехоплення — фільтр, а не ворота: воно не відмовляє ніколи
                // (сесія 11), хоч би в якому стані був випуск.
                Op::Arrival(_) => &[],
                Op::Claim(_) => &[code(ClubError::NothingToClaim)],
                Op::Prepay => &[code(ClubError::ObligationAlreadyRepaid)],
            };

            match &step.outcome {
                Outcome::Ok => {}
                Outcome::Refused(actual) if allowed.contains(actual) => {}
                other => panic!(
                    "seed {:#x}, крок {position} ({:?}, {:?}): {other:?} — очікувались {allowed:?}",
                    run.seed, step.phase, step.op
                ),
            }
        }
    }
}

// ---- 7. Двічі те саме ------------------------------------------------------

/// Ті самі гроші не забираються двічі.
///
/// Вимітання лишає чекпоінт кожного власника на поточному індексі, тому друге
/// вимітання мусить не дати нікому нічого — і відмовити всім однаково, іменем
/// `NothingToClaim`. Це найкоротший шлях до `SC-004`: чекпоінт, який не
/// зрушився, видно тут одразу, ще до того, як ескроу спорожніє.
#[test]
fn the_same_money_is_never_claimed_twice() {
    for run in corpus() {
        for step in run.steps_in(Phase::Rinse) {
            assert_eq!(
                step.to_owner, 0,
                "seed {:#x}: {:?} забрала ще раз те, що вже забрала",
                run.seed, step.op
            );
            assert_eq!(
                step.outcome,
                Outcome::Refused(code(ClubError::NothingToClaim)),
                "seed {:#x}: після вимітання {:?} не відмовила",
                run.seed,
                step.op
            );
        }

        // І дзеркально: вимітання справді щось вимело хоча б там, де було що.
        if run.credited() > 0 {
            let swept: i128 = run.steps_in(Phase::Sweep).map(|step| step.to_owner).sum();
            let in_script: i128 = run.steps_in(Phase::Script).map(|step| step.to_owner).sum();
            assert_eq!(
                swept + in_script,
                run.paid(),
                "seed {:#x}: виплачене не складається з кроків",
                run.seed
            );
        }
    }
}

// ---- 8. Відтворюваність ----------------------------------------------------

/// Прогін відтворюється із зерна.
///
/// Корпус, який не відтворюється, не є доказом: розбіжність, знайдена на
/// тисячному прогоні, мусить лишитись на тому ж місці й наступного разу — інакше
/// повідомлення про падіння вказує в нікуди. Тест бере ту саму послідовність із
/// корпусу і програє її наново з чистого світу.
#[test]
fn the_same_seed_replays_the_same_sequence() {
    let corpus = corpus();
    let bench = Bench::new();

    for run in [&corpus[0], &corpus[SEQUENCES / 2], &corpus[SEQUENCES - 1]] {
        let replay = play(&bench, run.seed);

        assert_eq!(
            replay.script, run.script,
            "seed {:#x}: інша форма",
            run.seed
        );
        assert_eq!(replay.lots, run.lots, "seed {:#x}: інші лоти", run.seed);
        assert_eq!(
            replay.credited(),
            run.credited(),
            "seed {:#x}: інше перехоплене",
            run.seed
        );
        assert_eq!(
            replay.paid_to, run.paid_to,
            "seed {:#x}: інші виплати",
            run.seed
        );
        assert_eq!(
            replay.escrow_final, run.escrow_final,
            "seed {:#x}: інший залишок",
            run.seed
        );
    }
}
