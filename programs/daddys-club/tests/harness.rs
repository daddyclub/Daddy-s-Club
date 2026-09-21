//! Спільний харнес mollusk: одне місце, де підіймаються обидві програми,
//! Token-2022, годинник і акаунти, з яких починається кожен тест на ланцюгу.
//!
//! **Потребує зібраних програм.** `Mollusk` вантажить справжній байткод із
//! `target/deploy/*.so`, а не підроблений процесор, тому перед `cargo test`
//! мусить пройти `anchor build`. Підроблена програма довела б лише те, що
//! підробка узгоджена сама з собою.
//!
//! Файл лежить просто в `tests/`, тому cargo збирає його ще й окремою тестовою
//! ціллю; решта тестів підключає його рядком
//!
//! ```ignore
//! #[path = "harness.rs"]
//! mod harness;
//! ```
//!
//! Самоперевірки внизу через це ганяються в кожному бінарнику, який харнес
//! використовує, а не лише у власному. Це навмисно: харнес, який перестав
//! піднімати програму, має падати там, де ним користуються.

#![allow(dead_code)]

use {
    anchor_lang::{
        solana_program::program_option::COption as HookCOption, AccountDeserialize,
        AccountSerialize, Space,
    },
    anchor_spl::token_2022::spl_token_2022::{
        extension::{
            transfer_hook::{TransferHook, TransferHookAccount},
            BaseStateWithExtensions, BaseStateWithExtensionsMut, ExtensionType,
            StateWithExtensions, StateWithExtensionsMut,
        },
        state::{Account as HookTokenAccount, AccountState as HookAccountState, Mint as HookMint},
    },
    daddys_club::{
        instructions::{
            issue::{hook_account_metas, EXTRA_ACCOUNT_METAS, EXTRA_METAS_SEED},
            protocol::ConfigParams,
        },
        state::{
            Issue, IssueState, ProtocolConfig, RevenueSource, CONFIG_SEED, HOLDER_SEED, ISSUE_SEED,
            OFFER_SEED, SOURCE_SEED,
        },
    },
    demo_issuer::POOL_SEED,
    mollusk_svm::{
        program::{
            create_program_account_loader_v3, keyed_account_for_system_program, loader_keys,
        },
        result::InstructionResult,
        Mollusk,
    },
    solana_account::Account,
    solana_address::Address as Pubkey,
    solana_program_option::COption,
    solana_rent::Rent,
    solana_svm_log_collector::LogCollector,
    spl_tlv_account_resolution::state::ExtraAccountMetaList,
    spl_token_interface::state::{Account as TokenAccount, AccountState, Mint},
    spl_transfer_hook_interface::instruction::ExecuteInstruction,
    std::{cell::RefCell, path::Path, rc::Rc, sync::Once},
};

// `COption` тут дві, і це не помилка: `spl-token-interface` зібраний на
// `solana-program-option` 3.x, а `spl-token-2022`, який дає розширення, — на
// 2.x. Типи однойменні, але різні, тож базові акаунти будуються першим,
// розширені — `HookCOption`. Обидва вміють `From<Option<T>>`.

/// Момент, у якому стоїть годинник кожного тесту. Фіксоване число, а не
/// поточний час: тест, який залежить від дати запуску, одного дня падає сам.
pub const NOW: i64 = 1_800_000_000;
pub const DAY: i64 = 86_400;

/// USDC має шість знаків; уся арифметика тестів рахується в цих одиницях.
pub const USDC_DECIMALS: u8 = 6;
/// Бонд неподільний: частка у випуску міряється лотами, а не дробами лота.
pub const BOND_DECIMALS: u8 = 0;

pub const ADMIN: Pubkey = Pubkey::new_from_array([11u8; 32]);
pub const ISSUER: Pubkey = Pubkey::new_from_array([12u8; 32]);
pub const INVESTOR: Pubkey = Pubkey::new_from_array([13u8; 32]);
pub const BUYER: Pubkey = Pubkey::new_from_array([14u8; 32]);
/// Гаманець, який ні до чого не належить, — ним перевіряються відмови прав.
pub const OUTSIDER: Pubkey = Pubkey::new_from_array([15u8; 32]);

pub const USDC_MINT: Pubkey = Pubkey::new_from_array([21u8; 32]);
pub const FEE_VAULT: Pubkey = Pubkey::new_from_array([22u8; 32]);
pub const BOND_MINT: Pubkey = Pubkey::new_from_array([23u8; 32]);

/// Два сховища випуску, обидва в USDC і обидва на authority випуску. Ключі
/// довільні, бо дерівацією не задані: знайти їх можна лише з самого `Issue`.
/// Тому вони й лежать поруч — переплутати їх найлегше саме тут.
pub const SUBSCRIPTION_VAULT: Pubkey = Pubkey::new_from_array([33u8; 32]);
pub const ESCROW_VAULT: Pubkey = Pubkey::new_from_array([34u8; 32]);

/// Рахунок джерела revenue: сюди емітент кладе комісію, звідси `intercept`
/// бере частку. Ключ довільний із тієї ж причини, що й у сховищ випуску —
/// дерівацією він не заданий, а живе в самому `RevenueSource`.
pub const SOURCE_VAULT: Pubkey = Pubkey::new_from_array([32u8; 32]);

/// Нумерація демо-світу: у `ISSUER` одне джерело, під ним один випуск.
pub const SOURCE_SEQ: u64 = 0;
pub const ISSUE_SEQ: u64 = 0;

/// Програма ядра.
pub fn club_id() -> Pubkey {
    Pubkey::new_from_array(daddys_club::ID.to_bytes())
}

/// Референсний емітент. Він тут не для повноти: `intercept` приймає дохід лише
/// від того, хто підписав PDA програми-емітента (`FR-004`), і без справжнього
/// байткоду цієї програми такий виклик неможливо ані зробити, ані підробити.
pub fn issuer_program_id() -> Pubkey {
    Pubkey::new_from_array(demo_issuer::ID.to_bytes())
}

/// PDA пулу референсного емітента — той самий ключ, який джерело записує собі
/// в `authority`, і той, чий **підпис** автентифікує перехоплення (`FR-004`).
///
/// Деривується з програми, а не вигадується: на вигаданому ключі тести
/// лишились би зеленими після зміни seeds у demo-емітенті, тобто перестали б
/// вести туди, куди веде сам байткод.
pub fn issuer_authority() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[POOL_SEED], &issuer_program_id())
}

pub fn token_program_id() -> Pubkey {
    mollusk_svm_programs_token::token2022::ID
}

/// Ключ mollusk у ключ Anchor. Крейти різні, представлення те саме.
pub fn anchor_key(key: Pubkey) -> anchor_lang::prelude::Pubkey {
    anchor_lang::prelude::Pubkey::new_from_array(key.to_bytes())
}

// ---- Деривації PDA ---------------------------------------------------------
//
// Seed-константи беруться з `daddys_club::state`, а не переписуються тут: на
// переписаних байтах харнес лишився б зеленим після перейменування seed'а в
// програмі й перестав би вести тести туди, куди веде сама програма.

pub fn config_pda() -> (Pubkey, u8) {
    Pubkey::find_program_address(&[CONFIG_SEED], &club_id())
}

pub fn source_pda(issuer: Pubkey, seq: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[SOURCE_SEED, issuer.as_ref(), &seq.to_le_bytes()],
        &club_id(),
    )
}

pub fn issue_pda(source: Pubkey, seq: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[ISSUE_SEED, source.as_ref(), &seq.to_le_bytes()],
        &club_id(),
    )
}

pub fn holder_pda(issue: Pubkey, owner: Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[HOLDER_SEED, issue.as_ref(), owner.as_ref()], &club_id())
}

/// Список додаткових акаунтів гука. Seed береться з програми: він там уже
/// звірений із тим, що шукає Token-2022.
pub fn extra_metas_pda(mint: Pubkey) -> (Pubkey, u8) {
    Pubkey::find_program_address(&[EXTRA_METAS_SEED, mint.as_ref()], &club_id())
}

pub fn offer_pda(issue: Pubkey, seller: Pubkey, nonce: u64) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &[
            OFFER_SEED,
            issue.as_ref(),
            seller.as_ref(),
            &nonce.to_le_bytes(),
        ],
        &club_id(),
    )
}

// ---- Підняття середовища ---------------------------------------------------

/// `Mollusk` шукає `.so` у `SBF_OUT_DIR`, а cargo про `target/deploy` не знає.
/// Змінна ставиться один раз на процес і лише якщо її не задали ззовні —
/// інакше харнес перебивав би свідомий вибір іншої збірки.
fn point_mollusk_at_the_built_programs() {
    static ONCE: Once = Once::new();

    ONCE.call_once(|| {
        if std::env::var_os("SBF_OUT_DIR").is_none() {
            let deploy = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/deploy");
            assert!(
                deploy.join("daddys_club.so").exists(),
                "немає {}: спершу `anchor build`, інакше mollusk не має що вантажити",
                deploy.join("daddys_club.so").display()
            );
            std::env::set_var("SBF_OUT_DIR", deploy);
        }
    });
}

/// Обидві програми, Token-2022 і годинник на `NOW`.
pub fn setup() -> Mollusk {
    point_mollusk_at_the_built_programs();

    let mut mollusk = Mollusk::new(&club_id(), "daddys_club");
    mollusk.add_program_with_loader(&issuer_program_id(), "demo_issuer", &loader_keys::LOADER_V3);
    mollusk_svm_programs_token::token2022::add_program(&mut mollusk);
    mollusk.sysvars.clock.unix_timestamp = NOW;

    mollusk
}

/// Те саме, але з логером. Ліміт знімається навмисно: обрізаний лог не
/// відрізнити від того, якого не було.
pub fn setup_with_logs() -> (Mollusk, Rc<RefCell<LogCollector>>) {
    let logs = LogCollector::new_ref_with_limit(None);
    let mut mollusk = setup();
    mollusk.logger = Some(Rc::clone(&logs));

    (mollusk, logs)
}

pub fn log_lines(logs: &Rc<RefCell<LogCollector>>) -> Vec<String> {
    logs.borrow().get_recorded_content().to_vec()
}

// ---- Акаунти ---------------------------------------------------------------

/// Набір, яким протокол реально запускається в демо: origination fee посеред
/// дозволеного діапазону, стеля перехоплення 30%, повний діапазон строків і
/// короткий поріг історії (`SPEC.md` → Припущення).
pub fn demo_config_params() -> ConfigParams {
    ConfigParams {
        origination_fee_bps: 150,
        trading_fee_bps: 50,
        max_pledge_bps: 3_000,
        min_tenor_secs: 30 * DAY,
        max_tenor_secs: 180 * DAY,
        history_threshold_secs: 7 * DAY,
    }
}

/// Конфіг у тому вигляді, в якому його лишає `init_protocol` на цих параметрах.
/// Кожна інструкція, що читає конфіг, починається з нього.
pub fn stored_config() -> ProtocolConfig {
    let params = demo_config_params();

    ProtocolConfig {
        admin: anchor_key(ADMIN),
        origination_fee_bps: params.origination_fee_bps,
        trading_fee_bps: params.trading_fee_bps,
        max_pledge_bps: params.max_pledge_bps,
        min_tenor_secs: params.min_tenor_secs,
        max_tenor_secs: params.max_tenor_secs,
        history_threshold_secs: params.history_threshold_secs,
        usdc_mint: anchor_key(USDC_MINT),
        fee_vault: anchor_key(FEE_VAULT),
        bump: config_pda().1,
    }
}

/// Джерело і випуск демо-світу. Функції, а не константи: адреси дерівуються.
pub fn demo_source() -> Pubkey {
    source_pda(ISSUER, SOURCE_SEQ).0
}

pub fn demo_issue() -> Pubkey {
    issue_pda(demo_source(), ISSUE_SEQ).0
}

/// Випуск у тому вигляді, в якому його лишає `create_issue` на умовах картки
/// M0 (250 000 USDC під 9.5% на 90 днів, 12% перехоплення, лот 1 USDC), плюс
/// рух погашення, який задає тест.
///
/// Живе тут, а не в тестовому файлі, з третього споживача: `open_position` і
/// `subscribe` беруть його з `tests/invest.rs`, `withdraw_proceeds` — з
/// `tests/issue.rs`, і три копії одного випуску розійшлися б у той бік, який
/// ніхто не ганяє. Що опис не розійшовся з інструкцією, стереже
/// `create_issue_freezes_the_terms_the_issuer_asked_for` — він міряє те саме
/// поле за полем на справжньому створенні.
pub fn stored_issue(state: IssueState, payout_index: u128) -> Issue {
    Issue {
        source: anchor_key(demo_source()),
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
        seq: ISSUE_SEQ,
        bump: issue_pda(demo_source(), ISSUE_SEQ).1,
    }
}

/// Випуск у погашенні: номінал зібрано, гроші видано, зобов'язання живе. Це
/// єдиний стан, у якому перехоплення розщеплює потік, тому з нього починається
/// кожен тест про гроші.
pub fn repaying(repaid_total: u64, payout_index: u128) -> Issue {
    let issue = stored_issue(IssueState::Repaying, payout_index);

    Issue {
        raised: issue.face,
        repaid_total,
        ..issue
    }
}

/// Джерело в тому вигляді, в якому його лишає `register_source`: `authority` —
/// PDA пулу демо-емітента, `vault` — рахунок, з якого йде частка.
///
/// Живе тут із другого споживача: перехоплення бере його з `tests/source.rs`,
/// своп — з `tests/swap.rs`. Дві копії розійшлися б саме в `authority`, тобто в
/// тому полі, на якому тримається вся автентифікація (`FR-004`).
pub fn stored_source(active_issue: Option<Pubkey>, total_observed: u64) -> RevenueSource {
    RevenueSource {
        issuer: anchor_key(ISSUER),
        authority: anchor_key(issuer_authority().0),
        vault: anchor_key(SOURCE_VAULT),
        first_seen_ts: NOW - 30 * DAY,
        total_observed,
        observed_before_issue: 0,
        active_issue: active_issue.map(anchor_key),
        seq: SOURCE_SEQ,
        bump: source_pda(ISSUER, SOURCE_SEQ).1,
    }
}

/// Гаманець із лампортами під оренду створюваних акаунтів.
pub fn wallet() -> Account {
    Account::new(10_000_000_000, 0, &Pubkey::default())
}

/// Порожнє місце під `init`: саме таким акаунт приходить у транзакцію до того,
/// як Anchor його створить.
pub fn uninitialized() -> Account {
    Account::default()
}

/// Готовий акаунт Anchor: дискримінатор, стан, добивка до `INIT_SPACE`.
///
/// Добивка потрібна тому, що `None` пишеться коротше за `Some`
/// (`RevenueSource::active_issue`), а місця в реальному акаунті виділено
/// завжди під довший варіант.
pub fn anchor_account<T: AccountSerialize + Space>(value: &T) -> Account {
    let mut data = Vec::with_capacity(8 + T::INIT_SPACE);
    value.try_serialize(&mut data).expect("стан серіалізується");
    data.resize(8 + T::INIT_SPACE, 0);

    Account {
        lamports: Rent::default().minimum_balance(data.len()),
        data,
        owner: club_id(),
        executable: false,
        rent_epoch: 0,
    }
}

pub fn decode<T: AccountDeserialize>(result: &InstructionResult, key: &Pubkey) -> T {
    let stored = result.get_account(key).expect("акаунт є в результаті");

    T::try_deserialize(&mut stored.data.as_slice()).expect("акаунт розбирається")
}

/// Мінт без розширень. Гук стоїть лише на бонді: на розрахунковій валюті він
/// зробив би перевірки перехоплення оманливо простішими.
pub fn plain_mint(decimals: u8, supply: u64) -> Account {
    mollusk_svm_programs_token::token2022::create_account_for_mint(Mint {
        mint_authority: COption::None,
        supply,
        decimals,
        is_initialized: true,
        freeze_authority: COption::None,
    })
}

/// Мінт USDC.
pub fn usdc_mint(supply: u64) -> Account {
    plain_mint(USDC_DECIMALS, supply)
}

/// Рахунок у мінті без розширень.
pub fn token_account(mint: Pubkey, owner: Pubkey, amount: u64) -> Account {
    mollusk_svm_programs_token::token2022::create_account_for_token_account(TokenAccount {
        mint,
        owner,
        amount,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    })
}

/// USDC-рахунок: гаманець інвестора, сховище випуску, скарбниця протоколу.
pub fn usdc_account(owner: Pubkey, amount: u64) -> Account {
    token_account(USDC_MINT, owner, amount)
}

fn owned_by_token_program(data: Vec<u8>) -> Account {
    Account {
        lamports: Rent::default().minimum_balance(data.len()),
        data,
        owner: token_program_id(),
        executable: false,
        rent_epoch: 0,
    }
}

/// Мінт бонду `BOND_MINT`: Token-2022 із `TransferHook`, який показує на ядро.
///
/// Саме це розширення робить `FR-017` можливим — облік при передачі діє й тоді,
/// коли переказ іде повз наш застосунок. Мінт без гука виглядав би в тесті так
/// само, а доводив би протилежне.
pub fn bond_mint(authority: Pubkey, supply: u64) -> Account {
    let len = ExtensionType::try_calculate_account_len::<HookMint>(&[ExtensionType::TransferHook])
        .expect("довжина мінта рахується");
    let mut data = vec![0u8; len];

    {
        let mut state = StateWithExtensionsMut::<HookMint>::unpack_uninitialized(&mut data)
            .expect("порожній мінт розпаковується");

        let hook = state
            .init_extension::<TransferHook>(true)
            .expect("гук ініціалізується");
        // Тип полів — `OptionalNonZeroPubkey` зі `spl-pod`, а самого крейта в
        // залежностях немає: `try_into` бере тип із поля, якому присвоюють.
        hook.authority = None.try_into().expect("authority гука вміщується");
        hook.program_id = Some(anchor_key(club_id()))
            .try_into()
            .expect("id гука вміщується");

        state.base = HookMint {
            mint_authority: HookCOption::Some(anchor_key(authority)),
            supply,
            decimals: BOND_DECIMALS,
            is_initialized: true,
            freeze_authority: HookCOption::None,
        };
        state.pack_base();
        state.init_account_type().expect("тип акаунта проставлено");
    }

    owned_by_token_program(data)
}

/// Рахунок бонду. Мінт із гуком вимагає від рахунку розширення
/// `TransferHookAccount`: у ньому Token-2022 і піднімає прапорець `transferring`
/// на час переказу, а `execute` цей прапорець перевіряє (`FR-017`).
pub fn bond_account(owner: Pubkey, amount: u64) -> Account {
    let len = ExtensionType::try_calculate_account_len::<HookTokenAccount>(&[
        ExtensionType::TransferHookAccount,
    ])
    .expect("довжина рахунку рахується");
    let mut data = vec![0u8; len];

    {
        let mut state = StateWithExtensionsMut::<HookTokenAccount>::unpack_uninitialized(&mut data)
            .expect("порожній рахунок розпаковується");

        state
            .init_extension::<TransferHookAccount>(true)
            .expect("розширення рахунку ініціалізується");

        state.base = HookTokenAccount {
            mint: anchor_key(BOND_MINT),
            owner: anchor_key(owner),
            amount,
            delegate: HookCOption::None,
            state: HookAccountState::Initialized,
            is_native: HookCOption::None,
            delegated_amount: 0,
            close_authority: HookCOption::None,
        };
        state.pack_base();
        state.init_account_type().expect("тип акаунта проставлено");
    }

    owned_by_token_program(data)
}

/// Список додаткових акаунтів гука в тому вигляді, в якому його лишає
/// `create_issue`: TLV на три мети з `hook_account_metas`, власник — ядро.
///
/// Без нього Token-2022 покликав би `execute` з чотирма обов'язковими
/// акаунтами, і гук упав би на нестачі акаунтів, а не на обліку. Тобто
/// передача бонду в тесті починається саме звідси.
pub fn extra_metas_account() -> Account {
    let metas = hook_account_metas(&anchor_key(demo_issue())).expect("список гука будується");
    let len = ExtraAccountMetaList::size_of(EXTRA_ACCOUNT_METAS).expect("розмір списку рахується");
    let mut data = vec![0u8; len];
    ExtraAccountMetaList::init::<ExecuteInstruction>(&mut data, &metas)
        .expect("список ініціалізується");

    Account {
        lamports: Rent::default().minimum_balance(data.len()),
        data,
        owner: club_id(),
        executable: false,
        rent_epoch: 0,
    }
}

/// Баланс токен-акаунта з результату — однаково для USDC і для бонду:
/// `StateWithExtensions` читає й рахунок без розширень.
pub fn token_balance(result: &InstructionResult, key: &Pubkey) -> u64 {
    let stored = result
        .get_account(key)
        .expect("токен-акаунт є в результаті");

    StateWithExtensions::<HookTokenAccount>::unpack(&stored.data)
        .expect("токен-акаунт розпаковується")
        .base
        .amount
}

/// Системна програма потрібна всюди, де щось створюється.
pub fn system_program() -> (Pubkey, Account) {
    keyed_account_for_system_program()
}

pub fn token_program() -> (Pubkey, Account) {
    mollusk_svm_programs_token::token2022::keyed_account()
}

/// Акаунт програми. Потрібен у наборі щоразу, коли на програму хтось
/// посилається: `Program<'info, _>` у списку акаунтів, ціль CPI, порожній слот
/// опційного акаунта.
pub fn program_account(program: Pubkey) -> (Pubkey, Account) {
    (program, create_program_account_loader_v3(&program))
}

/// Порожнє місце опційного акаунта Anchor. `None` подається program id тієї
/// програми, **яку кличуть**: Anchor звіряє ключ у слоті з `program_id` і, якщо
/// вони збіглися, не читає акаунт узагалі. Тобто «акаунта немає» — це не
/// коротший список, а окремий ключ у повному.
///
/// Параметр не для краси: у ланцюжку CPI програм дві, і слот, порожній для
/// демо-емітента, несе його id, а не id ядра. Переплутати їх — це подати ядру
/// акаунт, який воно спробує розібрати як випуск.
pub fn omitted(callee: Pubkey) -> (Pubkey, Account) {
    program_account(callee)
}

/// Підміна одного акаунта в готовому наборі — так пишеться негативний тест:
/// у ньому видно рівно те, що відрізняється від робочого випадку.
pub fn replacing(
    accounts: &[(Pubkey, Account)],
    key: Pubkey,
    account: Account,
) -> Vec<(Pubkey, Account)> {
    let mut accounts = accounts.to_vec();
    let slot = accounts
        .iter_mut()
        .find(|(existing, _)| *existing == key)
        .expect("акаунт, який підміняють, є у наборі");
    slot.1 = account;

    accounts
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_instruction::{AccountMeta, Instruction},
    };

    /// Виклик із дискримінатором, якого в програмі немає. Байти навмисно не
    /// посилаються на жодну інструкцію: самоперевірка нижче доводить, що
    /// байткод виконується, і не має ламатись щоразу, коли інструкція змінює
    /// підпис. Відкинути такий виклик програма мусить сама — а щоб відкинути,
    /// їй треба спершу запуститись.
    fn unknown_call() -> Instruction {
        Instruction::new_with_bytes(club_id(), &[0u8; 8], vec![AccountMeta::new(ADMIN, true)])
    }

    #[test]
    fn both_programs_are_loaded_from_the_built_bytecode() {
        let mollusk = setup();

        for (name, id) in [
            ("daddys_club", club_id()),
            ("demo_issuer", issuer_program_id()),
            ("token-2022", token_program_id()),
        ] {
            let elf = mollusk
                .program_cache
                .get_program_elf_bytes(&id)
                .unwrap_or_else(|| panic!("{name} не піднявся"));

            assert!(!elf.is_empty(), "{name} піднявся порожнім");
        }
    }

    /// Байткод у кеші ще не означає, що він виконується: ELF може завантажитись
    /// і впасти на верифікації при першому ж виклику.
    #[test]
    fn the_core_program_actually_runs() {
        let mollusk = setup();

        let result = mollusk.process_instruction(&unknown_call(), &[(ADMIN, wallet())]);

        // Що саме поверне інструкція — справа задачі, яка її напише. Тут
        // важливо лише, що програма дійшла до виконання і спалила одиниці.
        assert!(
            result.compute_units_consumed > 0,
            "програма не виконувалась: {:?}",
            result.program_result
        );
    }

    #[test]
    fn the_clock_stands_where_the_tests_expect_it() {
        assert_eq!(setup().sysvars.clock.unix_timestamp, NOW);
    }

    /// Seed-константи читаються з програми, тому звірити їх треба з літералами:
    /// перейменування seed'а в `state.rs` мовчки переїхало б і сюди.
    #[test]
    fn the_pda_helpers_derive_from_the_seeds_the_protocol_documents() {
        let source = source_pda(ISSUER, 0).0;
        let issue = issue_pda(source, 0).0;

        assert_eq!(
            config_pda(),
            Pubkey::find_program_address(&[b"config"], &club_id())
        );
        assert_eq!(
            source_pda(ISSUER, 7),
            Pubkey::find_program_address(
                &[b"source", ISSUER.as_ref(), &7u64.to_le_bytes()],
                &club_id()
            )
        );
        assert_eq!(
            issue_pda(source, 3),
            Pubkey::find_program_address(
                &[b"issue", source.as_ref(), &3u64.to_le_bytes()],
                &club_id()
            )
        );
        assert_eq!(
            holder_pda(issue, INVESTOR),
            Pubkey::find_program_address(
                &[b"holder", issue.as_ref(), INVESTOR.as_ref()],
                &club_id()
            )
        );
        assert_eq!(
            extra_metas_pda(BOND_MINT),
            Pubkey::find_program_address(&[b"extra-account-metas", BOND_MINT.as_ref()], &club_id())
        );
        // Seeds чужої програми, а пін той самий: цей ключ джерело записує собі
        // в `authority`, тому його зміна відрізала б від погашення всі вже
        // зареєстровані джерела (`FR-004`).
        assert_eq!(
            issuer_authority(),
            Pubkey::find_program_address(&[b"pool"], &issuer_program_id())
        );
        assert_eq!(
            offer_pda(issue, INVESTOR, 5),
            Pubkey::find_program_address(
                &[
                    b"offer",
                    issue.as_ref(),
                    INVESTOR.as_ref(),
                    &5u64.to_le_bytes()
                ],
                &club_id()
            )
        );
    }

    #[test]
    fn an_anchor_account_round_trips_through_the_builder() {
        let (_, bump) = config_pda();
        let config = ProtocolConfig {
            admin: anchor_key(ADMIN),
            origination_fee_bps: 150,
            trading_fee_bps: 25,
            max_pledge_bps: 3_000,
            min_tenor_secs: 30 * DAY,
            max_tenor_secs: 180 * DAY,
            history_threshold_secs: 30 * DAY,
            usdc_mint: anchor_key(USDC_MINT),
            fee_vault: anchor_key(FEE_VAULT),
            bump,
        };

        let account = anchor_account(&config);

        assert_eq!(account.owner, club_id());
        assert_eq!(account.data.len(), 8 + ProtocolConfig::INIT_SPACE);

        let decoded = ProtocolConfig::try_deserialize(&mut account.data.as_slice())
            .expect("конфіг розбирається");

        assert_eq!(decoded.admin, config.admin);
        assert_eq!(decoded.max_pledge_bps, config.max_pledge_bps);
        assert_eq!(decoded.bump, bump);
    }

    /// Найдовший акаунт протоколу, ще й з `u128` усередині: якщо добивка
    /// коротша за `INIT_SPACE`, ламається саме він.
    #[test]
    fn the_largest_account_fits_the_space_the_builder_reserves() {
        let source = source_pda(ISSUER, 0).0;
        let issue = Issue {
            source: anchor_key(source),
            bond_mint: anchor_key(BOND_MINT),
            escrow_vault: anchor_key(FEE_VAULT),
            subscription_vault: anchor_key(FEE_VAULT),
            face: 100_000_000,
            coupon_bps: 800,
            pledge_bps: 2_000,
            maturity_ts: NOW + 90 * DAY,
            subscription_end_ts: NOW + 7 * DAY,
            min_lot: 1_000_000,
            raised: 0,
            obligation_total: 108_000_000,
            repaid_total: 0,
            payout_index: u128::MAX,
            state: IssueState::Subscribing,
            seq: 0,
            bump: issue_pda(source, 0).1,
        };

        let account = anchor_account(&issue);
        assert_eq!(account.data.len(), 8 + Issue::INIT_SPACE);

        let decoded = Issue::try_deserialize(&mut account.data.as_slice()).expect("випуск");

        assert_eq!(decoded.payout_index, u128::MAX);
        assert_eq!(decoded.state, IssueState::Subscribing);
    }

    #[test]
    fn usdc_accounts_belong_to_token_2022_and_carry_their_balance() {
        let mint = usdc_mint(1_000_000);
        let account = usdc_account(INVESTOR, 250_000);

        assert_eq!(mint.owner, token_program_id());
        assert_eq!(account.owner, token_program_id());

        let unpacked = StateWithExtensions::<HookTokenAccount>::unpack(&account.data)
            .expect("рахунок розпаковується");

        assert_eq!(unpacked.base.amount, 250_000);
        assert_eq!(unpacked.base.owner, anchor_key(INVESTOR));
        assert_eq!(unpacked.base.mint, anchor_key(USDC_MINT));
    }

    /// Без цієї перевірки харнес міг би роздавати мінт без гука, і кожен тест на
    /// передачу доводив би поведінку, якої в продукті немає.
    #[test]
    fn the_bond_mint_points_its_transfer_hook_at_this_program() {
        let mint = bond_mint(ISSUER, 0);
        let unpacked =
            StateWithExtensions::<HookMint>::unpack(&mint.data).expect("мінт розпаковується");

        let hook: &TransferHook = unpacked.get_extension().expect("гук на місці");
        let program_id: Option<anchor_lang::prelude::Pubkey> = hook.program_id.into();

        assert_eq!(program_id, Some(anchor_key(club_id())));
        assert_eq!(unpacked.base.decimals, BOND_DECIMALS);
        assert_eq!(
            unpacked.base.mint_authority,
            HookCOption::Some(anchor_key(ISSUER))
        );
    }

    #[test]
    fn a_bond_account_starts_outside_a_transfer() {
        let account = bond_account(INVESTOR, 42);
        let unpacked = StateWithExtensions::<HookTokenAccount>::unpack(&account.data)
            .expect("рахунок розпаковується");

        let flag: &TransferHookAccount = unpacked.get_extension().expect("розширення на місці");

        assert!(
            !bool::from(flag.transferring),
            "поза переказом прапорець має лежати: інакше прямий виклик гука виглядав би законним"
        );
        assert_eq!(unpacked.base.amount, 42);
    }

    #[test]
    fn the_logger_records_what_the_program_printed() {
        let (mollusk, logs) = setup_with_logs();

        mollusk.process_instruction(&unknown_call(), &[(ADMIN, wallet())]);

        let lines = log_lines(&logs);

        assert!(
            lines
                .iter()
                .any(|line| line.contains(&club_id().to_string()) && line.contains("invoke")),
            "у логу немає сліду виклику ядра: {lines:?}"
        );
    }

    #[test]
    fn replacing_swaps_exactly_one_account() {
        let accounts = vec![
            (INVESTOR, usdc_account(INVESTOR, 100)),
            (BUYER, usdc_account(BUYER, 200)),
        ];

        let swapped = replacing(&accounts, BUYER, usdc_account(BUYER, 0));

        assert_eq!(swapped.len(), accounts.len());
        assert_eq!(swapped[0].1.data, accounts[0].1.data);
        assert_ne!(swapped[1].1.data, accounts[1].1.data);
    }
}
