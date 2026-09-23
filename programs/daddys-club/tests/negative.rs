//! `SC-007` — перелік того, чого протокол не дозволяє **нікому**.
//!
//! Критерій каже: «спроба забрати виплату, змінити умови випуску або зняти
//! кошти з ескроу не тим гаманцем — 0 успішних із повного набору негативних
//! сценаріїв». Цей файл і є той набір, зібраний в одному місці, щоб його можна
//! було прочитати як перелік, а не збирати очима по шести файлах.
//!
//! **Він не дублює відмов, які вже має власний файл інструкції.** Відмова, що
//! належить одній інструкції, живе поруч із її позитивними тестами — там видно
//! світ, у якому вона трапилась. Сюди винесене рівно те, чого там немає:
//!
//! - **Гроші, що лежать у сховищі.** Жоден із файлів вище не питає, чи можна
//!   взяти гроші зі сховища **повз наші інструкції** — просто покликавши
//!   Token-2022. А саме так і виглядає найкоротша атака: наш набір `has_one`
//!   стереже двері, яких зловмисникові не треба відчиняти. Тут перевіряється,
//!   що інших дверей немає: обидва сховища відповідають лише PDA випуску, і
//!   гаманець — чий завгодно — не зрушить із них нічого, не візьме їх собі й не
//!   випише на них делегата.
//! - **Підпис як такий.** Тести прав скрізь вище підміняють **ключ**: підписав
//!   чужий. Жоден не питає, що буде, коли не підписав **ніхто** — а це рівно
//!   те, що надішле сторонній, бо чужого підпису в нього й немає.
//! - **Чужий випуск як другий бік ланцюга.** У `tests/issue.rs` є «чужим
//!   підписом не забереш», у `tests/invest.rs` — «з чужого обліку не забереш».
//!   Тут навпаки: свій підпис і свій облік, але прикладені **не до того
//!   випуску**. Це та сама діра, яку сесія 13 знайшла мутацією в `prepay`
//!   (`has_one = source`); у `withdraw_proceeds` і в `claim` вона досі не
//!   перевірена.
//! - **Гук, покликаний напряму.** `tests/hook.rs` ганяє `execute` лише
//!   зсередини справжнього `transfer_checked` — тобто перевіряє, що гук рахує
//!   правильно, коли переказ **є**. Протилежного питання — що буде, коли
//!   виклик є, а переказу немає, — там немає, а це найдешевша атака в
//!   протоколі: грошей не рухається, а облік переписується.
//! - **Пропозиція бонду після заморозки умов** (`FR-013`). Що мінтом володіє
//!   PDA випуску, `tests/issue.rs` показує на створенні; що з цього випливає —
//!   не показує ніде.
//!
//! Чого тут навмисно **немає**, бо воно вже перевірене по своїх файлах:
//! `withdraw_proceeds` до повного збору — `an_issue_that_was_not_funded_pays_the_issuer_nothing`
//! (`Subscribing` і `Failed`) та `a_funded_issue_that_did_not_actually_raise_the_face_pays_out_nothing`
//! (другий замок на рівність) у `tests/issue.rs`; чужий підпис у видачі й
//! достроковому погашенні — там же; чужий облік, чуже сховище й чужий мінт у
//! виплаті — `tests/invest.rs`; чужий випуск у перехопленні —
//! `tests/source.rs`; зміна умов адміністратором —
//! `update_config_does_not_reach_an_issue_that_already_exists` у
//! `tests/protocol.rs`, а другий випуск поверх наявного —
//! `create_issue_refuses_to_overwrite_an_issue_that_already_exists` і
//! `a_source_that_already_backs_an_issue_refuses_a_second_one`.
//!
//! **Межа цього файлу.** Замки на сховищах належать Token-2022, а не нам: наше
//! в них — рядки `token::authority = issue` і `mint::authority = issue` у
//! `create_issue`, і їх стережуть `both_vaults_are_empty_and_answer_only_to_the_issue`
//! та `the_bond_mint_is_born_empty_with_a_hook_nobody_can_move`. Тут доводиться
//! наслідок: з такою authority гаманець не рухає нічого. А от «PDA не підпише
//! сам» mollusk довести не може — він шанує `is_signer` у метаданих, тому
//! підписати PDA в тесті вийшло б, а на ланцюгу — ні. Це та сама межа, що й у
//! тестах перехоплення з сесії 11.

#[path = "harness.rs"]
mod harness;

use {
    anchor_lang::{solana_program::program_option::COption as HookCOption, InstructionData},
    anchor_spl::token_2022::spl_token_2022::{
        error::TokenError,
        extension::{
            transfer_hook::TransferHookAccount, BaseStateWithExtensionsMut, StateWithExtensions,
            StateWithExtensionsMut,
        },
        instruction::{AuthorityType, TokenInstruction},
        state::{Account as HookTokenAccount, Mint as HookMint},
    },
    daddys_club::{
        errors::ClubError,
        instructions::issue::IssueParams,
        math,
        state::{HolderCheckpoint, Issue, IssueState, RevenueSource},
    },
    harness::*,
    mollusk_svm::result::{Check, InstructionResult},
    solana_account::Account,
    solana_address::Address as Pubkey,
    solana_instruction::{AccountMeta, Instruction},
    solana_program_error::ProgramError,
};

/// Рахунок, куди сторонній складав би чуже.
const THIEF_USDC: Pubkey = Pubkey::new_from_array([51u8; 32]);
/// Його ж рахунок у бонді — на нього він друкував би пропозицію з повітря.
const THIEF_BOND: Pubkey = Pubkey::new_from_array([52u8; 32]);

const INVESTOR_USDC: Pubkey = Pubkey::new_from_array([53u8; 32]);
const INVESTOR_BOND: Pubkey = Pubkey::new_from_array([54u8; 32]);
const ISSUER_USDC: Pubkey = Pubkey::new_from_array([55u8; 32]);
/// Рахунок бонду другого власника — друга сторона прямого виклику гука.
const BUYER_BOND: Pubkey = Pubkey::new_from_array([56u8; 32]);

/// Скільки лежить у сховищі погашення — 12 000 USDC перехопленої частки.
const PAID_IN: u64 = 12_000_000_000;
/// Індекс, який лишило по собі це перехоплення: `PAID_IN * SCALE / face`.
const INDEX: u128 = 48_000_000_000;
/// Баланс власника — 10 000 одиниць номіналу з 250 000.
const BALANCE: u64 = 10_000_000_000;
/// Що йому з `PAID_IN` належить: 4% — 480 USDC.
const OWED: u64 = 480_000_000;

/// Чотири архетипи викликача. Сторонній тут не єдиний і навіть не головний:
/// `SC-007` питає про **будь-який** гаманець, а найнебезпечніші — саме ті, чиї
/// права десь у протоколі справжні. Емітент справді розпоряджається номіналом,
/// адміністратор — параметрами, власник бонду — своєю часткою; сховище не
/// відкривається жодному з них.
const EVERY_WALLET: [(&str, Pubkey); 4] = [
    ("сторонній", OUTSIDER),
    ("емітент", ISSUER),
    ("адміністратор протоколу", ADMIN),
    ("власник бонду", INVESTOR),
];

fn token_err(error: TokenError) -> Check<'static> {
    Check::err(ProgramError::Custom(error as u32))
}

fn anchor_err(code: anchor_lang::error::ErrorCode) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(code)))
}

fn club_err(error: ClubError) -> Check<'static> {
    Check::err(ProgramError::Custom(u32::from(error)))
}

// ---- Кому відповідають сховища й мінт: питаємо саму інструкцію -------------

/// Умови з картки випуску на M0 — ті самі, які фіксує `stored_issue`.
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

/// Authority, які `create_issue` справді проставила: двом сховищам і мінту
/// бонду.
struct AsCreated {
    escrow: Pubkey,
    subscription: Pubkey,
    bond: Pubkey,
}

fn token_owner(accounts: &InstructionResult, key: &Pubkey) -> Pubkey {
    let stored = accounts.get_account(key).expect("рахунок є в результаті");
    let owner = StateWithExtensions::<HookTokenAccount>::unpack(&stored.data)
        .expect("рахунок розпаковується")
        .base
        .owner;

    Pubkey::new_from_array(owner.to_bytes())
}

/// Прогін справжнього створення випуску — рівно для того, щоб **не вигадувати**
/// authority сховищ і мінта, а прочитати їх із результату.
///
/// Тести нижче доводять наслідок: «з такою authority гаманець не рухає нічого».
/// Якби authority бралася з літерала, вони лишились би зеленими й після того, як
/// `create_issue` почала б віддавати сховище емітенту, — тобто доводили б
/// властивість світу, якого більше немає. Тому питається сама інструкція.
fn as_created() -> AsCreated {
    let create = Instruction::new_with_bytes(
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
    );

    let result = setup().process_and_validate_instruction(
        &create,
        &[
            (config_pda().0, anchor_account(&stored_config())),
            (demo_source(), anchor_account(&source_of(ISSUER, None))),
            (demo_issue(), uninitialized()),
            (ISSUER, wallet()),
            (USDC_MINT, usdc_mint(0)),
            (BOND_MINT, uninitialized()),
            (SUBSCRIPTION_VAULT, uninitialized()),
            (ESCROW_VAULT, uninitialized()),
            (extra_metas_pda(BOND_MINT).0, uninitialized()),
            token_program(),
            system_program(),
        ],
        &[Check::success()],
    );

    let mint = result.get_account(&BOND_MINT).expect("мінт є в результаті");
    let authority = StateWithExtensions::<HookMint>::unpack(&mint.data)
        .expect("мінт розпаковується")
        .base
        .mint_authority
        .expect("мінт має authority");

    AsCreated {
        escrow: token_owner(&result, &ESCROW_VAULT),
        subscription: token_owner(&result, &SUBSCRIPTION_VAULT),
        bond: Pubkey::new_from_array(authority.to_bytes()),
    }
}

// ---- Сховища: гроші рухає лише програма ------------------------------------

/// Виклик Token-2022 напряму. Дані пакує сам крейт токен-програми, а не
/// переписані тут байти: на переписаних тег інструкції одного дня розійшовся б
/// із тим, що читає Token-2022, і тест лишився б зеленим на виклику, якого вже
/// не існує. Тип `Instruction` при цьому наш — крейт токен-програми зібраний на
/// іншій його версії, ніж приймає mollusk.
fn token_ix(data: TokenInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction::new_with_bytes(token_program().0, &data.pack(), accounts)
}

fn transfer_out_of(vault: Pubkey, signer: Pubkey, amount: u64) -> Instruction {
    token_ix(
        TokenInstruction::TransferChecked {
            amount,
            decimals: USDC_DECIMALS,
        },
        vec![
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new(THIEF_USDC, false),
            AccountMeta::new_readonly(signer, true),
        ],
    )
}

/// Світ у погашенні, розкладений під прямий виклик токен-програми: в ескроу
/// погашення лежить перехоплене, у сховищі підписки — зібраний номінал. Обидва
/// сховища повні навмисно: відмова через порожній рахунок
/// (`InsufficientFunds`) виглядала б у цих тестах так само, як відмова через
/// права, і доводила б не те.
fn vault_world(created: &AsCreated) -> Vec<(Pubkey, Account)> {
    let issue = repaying(PAID_IN, INDEX);

    vec![
        (demo_issue(), anchor_account(&issue)),
        (ESCROW_VAULT, usdc_account(created.escrow, PAID_IN)),
        (
            SUBSCRIPTION_VAULT,
            usdc_account(created.subscription, issue.face),
        ),
        (THIEF_USDC, usdc_account(OUTSIDER, 0)),
        (USDC_MINT, usdc_mint(1_000_000_000_000_000)),
        (OUTSIDER, wallet()),
        (ISSUER, wallet()),
        (ADMIN, wallet()),
        (INVESTOR, wallet()),
        token_program(),
    ]
}

/// Сховище погашення тримає гроші власників бондів, і взяти їх можна рівно
/// однією інструкцією — `claim`, на суму, яку назвав облік. Прямий переказ із
/// нього — найкоротший шлях повз усі наші `has_one`, і він упирається в те, що
/// authority сховища — PDA випуску, а не гаманець.
#[test]
fn no_wallet_takes_anything_out_of_the_repayment_escrow() {
    let created = as_created();

    for (who, signer) in EVERY_WALLET {
        println!("прямий переказ зі сховища погашення, підписує: {who}");

        // Одна одиниця і все сховище — та сама відмова. Перевіряються обидві,
        // бо «на копійку можна, на все не можна» виглядало б саме як діра.
        for amount in [1, PAID_IN] {
            setup().process_and_validate_instruction(
                &transfer_out_of(ESCROW_VAULT, signer, amount),
                &vault_world(&created),
                &[token_err(TokenError::OwnerMismatch)],
            );
        }
    }
}

/// Дзеркало для другого сховища. `FR-008`: до успішного закриття внески
/// належать інвесторам і емітенту не доступні, а `FR-011` веде їх звідси назад
/// до інвесторів. Обидві обіцянки тримаються на тому, що з цього рахунку не
/// бере ніхто, доки не покликано `withdraw_proceeds` або `refund`.
#[test]
fn no_wallet_takes_anything_out_of_the_subscription_escrow() {
    let created = as_created();
    let face = repaying(PAID_IN, INDEX).face;

    for (who, signer) in EVERY_WALLET {
        println!("прямий переказ зі сховища підписки, підписує: {who}");

        for amount in [1, face] {
            setup().process_and_validate_instruction(
                &transfer_out_of(SUBSCRIPTION_VAULT, signer, amount),
                &vault_world(&created),
                &[token_err(TokenError::OwnerMismatch)],
            );
        }
    }
}

/// Довга атака: не забрати гроші зараз, а стати тим, хто зможе забрати їх
/// потім. Двері тут двоє — переписати authority сховища або виписати на нього
/// делегата, — і обидві мусять бути замкнені тим самим замком, що й переказ.
/// Без цього тесту попередні два доводили б лише те, що перший хід не
/// проходить.
#[test]
fn no_wallet_makes_itself_the_owner_of_an_escrow() {
    let created = as_created();

    for (who, signer) in EVERY_WALLET {
        println!("захоплення сховища погашення, підписує: {who}");

        let seize = token_ix(
            TokenInstruction::SetAuthority {
                authority_type: AuthorityType::AccountOwner,
                new_authority: HookCOption::Some(anchor_key(OUTSIDER)),
            },
            vec![
                AccountMeta::new(ESCROW_VAULT, false),
                AccountMeta::new_readonly(signer, true),
            ],
        );

        let delegate = token_ix(
            TokenInstruction::Approve { amount: PAID_IN },
            vec![
                AccountMeta::new(ESCROW_VAULT, false),
                AccountMeta::new_readonly(OUTSIDER, false),
                AccountMeta::new_readonly(signer, true),
            ],
        );

        for instruction in [seize, delegate] {
            setup().process_and_validate_instruction(
                &instruction,
                &vault_world(&created),
                &[token_err(TokenError::OwnerMismatch)],
            );
        }
    }
}

// ---- Пропозиція бонду: заморожена назавжди (`FR-013`) ----------------------

/// `FR-013` каже прямо: після повного збору пропозиція фіксується назавжди і не
/// може бути збільшена **ані емітентом, ані адміністратором**. Замок на це
/// один — `mint::authority = issue` у `create_issue`, тобто друкувати вміє лише
/// програма і лише в `subscribe`. Що authority саме така, показано на
/// створенні; що з цього випливає — не показано ніде, а випливає найдорожче:
/// надрукований із повітря бонд ділить той самий індекс на більшу пропозицію й
/// забирає гроші в тих, хто платив.
#[test]
fn no_wallet_prints_bond_after_the_terms_are_frozen() {
    let created = as_created();
    let issue = repaying(PAID_IN, INDEX);

    let world = vec![
        (demo_issue(), anchor_account(&issue)),
        (BOND_MINT, bond_mint(created.bond, issue.raised)),
        (THIEF_BOND, bond_account(OUTSIDER, 0)),
        (OUTSIDER, wallet()),
        (ISSUER, wallet()),
        (ADMIN, wallet()),
        (INVESTOR, wallet()),
        token_program(),
    ];

    for (who, signer) in EVERY_WALLET {
        println!("друк бонду понад пропозицію, підписує: {who}");

        let print = token_ix(
            TokenInstruction::MintTo { amount: issue.face },
            vec![
                AccountMeta::new(BOND_MINT, false),
                AccountMeta::new(THIEF_BOND, false),
                AccountMeta::new_readonly(signer, true),
            ],
        );

        setup().process_and_validate_instruction(
            &print,
            &world,
            &[token_err(TokenError::OwnerMismatch)],
        );
    }
}

// ---- Виплата: підпис і той випуск, до якого належить облік -----------------

fn holder_of(issue: Pubkey, owner: Pubkey) -> HolderCheckpoint {
    HolderCheckpoint {
        issue: anchor_key(issue),
        owner: anchor_key(owner),
        index_at_checkpoint: 0,
        accrued: 0,
        claimed_total: 0,
        bump: holder_pda(issue, owner).1,
    }
}

/// Виплата в тому вигляді, в якому її подають: усе на місці, окрім однієї речі,
/// яку задає тест, — чи підписав власник і який облік він приклав.
fn claim_ix(holder: Pubkey, owner_signs: bool) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Claim {}.data(),
        vec![
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(holder, false),
            AccountMeta::new_readonly(INVESTOR, owner_signs),
            AccountMeta::new(INVESTOR_USDC, false),
            AccountMeta::new(ESCROW_VAULT, false),
            AccountMeta::new_readonly(INVESTOR_BOND, false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

/// Світ виплати: у сховищі лежить перехоплене, у власника є облік і бонд, а
/// претензія в нього справжня — тобто відмовити цим тестам може лише те, що
/// вони перевіряють, а не порожня претензія.
fn claim_world(extra: Option<(Pubkey, Account)>) -> Vec<(Pubkey, Account)> {
    let issue = repaying(PAID_IN, INDEX);

    let mut world = vec![
        (demo_issue(), anchor_account(&issue)),
        (
            holder_pda(demo_issue(), INVESTOR).0,
            anchor_account(&holder_of(demo_issue(), INVESTOR)),
        ),
        (INVESTOR, wallet()),
        (INVESTOR_USDC, usdc_account(INVESTOR, 0)),
        (INVESTOR_BOND, bond_account(INVESTOR, BALANCE)),
        (ESCROW_VAULT, usdc_account(demo_issue(), PAID_IN)),
        (BOND_MINT, bond_mint(demo_issue(), issue.raised)),
        (USDC_MINT, usdc_mint(1_000_000_000_000_000)),
        token_program(),
    ];
    world.extend(extra);

    world
}

/// Тести прав вище всі підміняють **ключ**: підписав не той. Цей питає інше —
/// що буде, коли не підписав ніхто. Саме таку транзакцію й надішле сторонній:
/// чужого підпису в нього немає й не буде, тому єдина його спроба — подати
/// чужий гаманець без підпису й сподіватись, що звірять лише адресу.
///
/// Претензія тут справжня, облік правильний, рахунки власникові. Не сходиться
/// рівно одна річ, і вона й мусить відмовити.
#[test]
fn a_payout_without_the_owners_signature_is_not_a_payout() {
    setup().process_and_validate_instruction(
        &claim_ix(holder_pda(demo_issue(), INVESTOR).0, false),
        &claim_world(None),
        &[anchor_err(anchor_lang::error::ErrorCode::AccountNotSigner)],
    );
}

/// Свій підпис, свій облік — але з **іншого випуску**. Це другий бік ланцюга
/// «власник → облік → випуск», і перевірений він досі не був: у
/// `tests/invest.rs` підміняється власник, тут — випуск.
///
/// Атака має сенс саме тоді, коли обидва випуски справжні: тримати бонд
/// дешевого випуску й прикласти його облік до сховища дорогого — це спосіб
/// забрати чуже, не підробляючи жодного підпису. Seeds обліку містять випуск,
/// тому набір не сходиться ще до тіла інструкції.
#[test]
fn the_ledger_of_one_issue_does_not_open_the_escrow_of_another() {
    // Випуск іншого емітента під його ж джерелом — той, у якому власник справді
    // має облік.
    let other_issue = issue_pda(source_pda(BUYER, SOURCE_SEQ).0, ISSUE_SEQ).0;
    let other_ledger = holder_pda(other_issue, INVESTOR).0;

    setup().process_and_validate_instruction(
        &claim_ix(other_ledger, true),
        &claim_world(Some((
            other_ledger,
            anchor_account(&holder_of(other_issue, INVESTOR)),
        ))),
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintSeeds)],
    );
}

// ---- Видача: те саме з боку емітента ---------------------------------------

/// Джерело в тому вигляді, в якому його лишає `register_source`. Складається
/// тут, а не береться з харнесу, бо цим тестам потрібні **два** джерела різних
/// емітентів, а харнесне знає лише одного.
fn source_of(issuer: Pubkey, active_issue: Option<Pubkey>) -> RevenueSource {
    RevenueSource {
        issuer: anchor_key(issuer),
        authority: anchor_key(issuer_authority().0),
        vault: anchor_key(SOURCE_VAULT),
        first_seen_ts: NOW - 30 * DAY,
        total_observed: 0,
        observed_before_issue: 0,
        active_issue: active_issue.map(anchor_key),
        seq: SOURCE_SEQ,
        bump: source_pda(issuer, SOURCE_SEQ).1,
    }
}

fn funded() -> Issue {
    let issue = stored_issue(IssueState::Funded, 0);

    Issue {
        raised: issue.face,
        ..issue
    }
}

fn withdraw_ix(source: Pubkey, issuer: Pubkey, issuer_usdc: Pubkey, signs: bool) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::WithdrawProceeds {}.data(),
        vec![
            AccountMeta::new_readonly(config_pda().0, false),
            AccountMeta::new(demo_issue(), false),
            AccountMeta::new_readonly(source, false),
            AccountMeta::new_readonly(issuer, signs),
            AccountMeta::new(issuer_usdc, false),
            AccountMeta::new(SUBSCRIPTION_VAULT, false),
            AccountMeta::new(FEE_VAULT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

/// Світ на момент видачі: випуск зібрано повністю, номінал лежить у сховищі
/// підписки. Тобто відмовити тут може лише право, а не стан.
fn withdraw_world(extra: Vec<(Pubkey, Account)>) -> Vec<(Pubkey, Account)> {
    let issue = funded();

    let mut world = vec![
        (config_pda().0, anchor_account(&stored_config())),
        (demo_issue(), anchor_account(&issue)),
        (
            demo_source(),
            anchor_account(&source_of(ISSUER, Some(demo_issue()))),
        ),
        (ISSUER, wallet()),
        (ISSUER_USDC, usdc_account(ISSUER, 0)),
        (SUBSCRIPTION_VAULT, usdc_account(demo_issue(), issue.raised)),
        (FEE_VAULT, usdc_account(ADMIN, 0)),
        (USDC_MINT, usdc_mint(1_000_000_000_000_000)),
        token_program(),
    ];
    world.extend(extra);

    world
}

/// Дзеркало виплати з боку емітента: чужий підпис у `tests/issue.rs` уже
/// перевірений, відсутній — ні. Номінал лежить у сховищі підписки, і транзакція
/// без підпису емітента мусить розбитись об `Signer`, а не пройти на тому, що
/// адреса збіглася.
#[test]
fn the_proceeds_do_not_move_without_the_issuers_signature() {
    setup().process_and_validate_instruction(
        &withdraw_ix(demo_source(), ISSUER, ISSUER_USDC, false),
        &withdraw_world(vec![]),
        &[anchor_err(anchor_lang::error::ErrorCode::AccountNotSigner)],
    );
}

/// Свій підпис і **своє** джерело — але прикладені до чужого випуску. Це та
/// сама діра, яку сесія 13 знайшла мутацією в `prepay`: підмінити лише
/// підписанта замало, бо тоді відмовляє `has_one = issuer` на джерелі, і замок
/// `has_one = source` на випуску лишається без господаря. Справжня спроба подає
/// джерело **разом** із підписом — тоді ланцюг «випуск → джерело» рветься на
/// першому кроці, і саме це тут і міряється.
///
/// Сторонній для цього мусить бути справжнім емітентом зі справжнім джерелом —
/// тобто це не спроба навмання, а те, що зробить сусід по маркетплейсу.
#[test]
fn the_proceeds_are_not_unlocked_by_a_source_that_backs_another_issue() {
    let his_source = source_pda(OUTSIDER, SOURCE_SEQ).0;

    setup().process_and_validate_instruction(
        &withdraw_ix(his_source, OUTSIDER, THIEF_USDC, true),
        &withdraw_world(vec![
            (his_source, anchor_account(&source_of(OUTSIDER, None))),
            (OUTSIDER, wallet()),
            (THIEF_USDC, usdc_account(OUTSIDER, 0)),
        ]),
        &[anchor_err(anchor_lang::error::ErrorCode::ConstraintHasOne)],
    );
}

// ---- Гук: чекпоінти рухає лише справжній переказ ---------------------------

/// Виклик `execute` у тому вигляді, в якому його надішле атакувальник: тими
/// самими байтами дискримінатора, що й Token-2022 (`instruction::Execute` несе
/// тег інтерфейсу, не Anchor-ів sha256), і без жодного підпису — `owner` тут
/// `UncheckedAccount`, бо підпис переказу перевіряє токен-програма, а не ми.
///
/// Тобто зібрати цю транзакцію може будь-хто, не маючи взагалі нічого: усі
/// вісім акаунтів публічні, а `amount` він називає сам.
fn execute_ix(amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Execute { amount }.data(),
        vec![
            AccountMeta::new(INVESTOR_BOND, false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new(BUYER_BOND, false),
            AccountMeta::new_readonly(INVESTOR, false),
            AccountMeta::new_readonly(extra_metas_pda(BOND_MINT).0, false),
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(holder_pda(demo_issue(), INVESTOR).0, false),
            AccountMeta::new(holder_pda(demo_issue(), BUYER).0, false),
        ],
    )
}

/// Той самий рахунок бонду, але з піднятим прапорцем `transferring` — тобто
/// такий, яким його бачить гук **усередині** справжнього переказу.
///
/// На ланцюгу так зробити не можна: рахунком володіє Token-2022, прапорець
/// підіймає його ж процесор перед CPI в гук і опускає одразу після
/// (`the_transferring_flag_is_down_again_…` у `tests/hook.rs`). Тут він
/// виставляється руками — і це не дірка, а єдиний спосіб подати гукові
/// **одну** справжню сторону: рахунок, який справді стоїть у чужому переказі,
/// атакувальник міг би прикласти до свого виклику як другу сторону.
fn mid_transfer(mut account: Account) -> Account {
    {
        let mut state = StateWithExtensionsMut::<HookTokenAccount>::unpack(&mut account.data)
            .expect("рахунок бонду розпаковується");
        let flag = state
            .get_extension_mut::<TransferHookAccount>()
            .expect("рахунок бонду має розширення гука");

        flag.transferring = true.into();
    }

    account
}

/// Світ прямого виклику: обидва власники тримають однакову позицію з
/// чекпоінтами в нулі, а у випуску вже перехоплено `PAID_IN`. Прапорці рахунків
/// задає тест — більше в цьому світі не бракує нічого, тому відмовити тут може
/// лише замок гука.
fn hook_world(source: Account, destination: Account) -> Vec<(Pubkey, Account)> {
    let issue = repaying(PAID_IN, INDEX);

    vec![
        (demo_issue(), anchor_account(&issue)),
        (
            holder_pda(demo_issue(), INVESTOR).0,
            anchor_account(&holder_of(demo_issue(), INVESTOR)),
        ),
        (
            holder_pda(demo_issue(), BUYER).0,
            anchor_account(&holder_of(demo_issue(), BUYER)),
        ),
        (INVESTOR_BOND, source),
        (BUYER_BOND, destination),
        (BOND_MINT, bond_mint(demo_issue(), issue.raised)),
        (extra_metas_pda(BOND_MINT).0, extra_metas_account()),
        (INVESTOR, wallet()),
        (BUYER, wallet()),
        token_program(),
    ]
}

fn ledger_after(result: &InstructionResult, owner: Pubkey) -> HolderCheckpoint {
    decode(result, &holder_pda(demo_issue(), owner).0)
}

/// Скільки коштує прямий виклик, якщо він проходить: атакувальник називає
/// `amount` сам, а гук відновлює з нього баланси «до передачі». Назвавши чужу
/// позицію, він нараховує собі на подвійний баланс і водночас зсуває чекпоінт
/// другої сторони на нуль її балансу — тобто списує їй усе зароблене.
///
/// Ці два числа й перевіряються нижче як те, чого **не** сталося.
fn the_damage() -> (u64, u64) {
    let doubled =
        math::claimable(INDEX, 0, u128::from(2 * BALANCE), 0).expect("претензія рахується");
    let honest = math::claimable(INDEX, 0, u128::from(BALANCE), 0).expect("претензія рахується");

    assert_eq!(
        honest,
        u128::from(OWED),
        "світ тесту розійшовся з арифметикою"
    );
    assert_eq!(
        doubled,
        2 * u128::from(OWED),
        "атака мусить бути на реальну суму"
    );

    (OWED, 2 * OWED)
}

/// Гук — звичайна інструкція, і покликати її може будь-хто. Замок один:
/// прапорець `transferring`, який Token-2022 тримає піднятим рівно на час
/// переказу. Поза переказом обидва прапорці опущені — і виклик відмовляє
/// названою помилкою, не зачепивши жодного чекпоінта.
///
/// Без цього замка найдешевша атака в протоколі виглядала б так: назвати
/// `amount` завбільшки з чужу позицію й покликати `execute`. Грошей не
/// рухається, але облік переписується — собі нараховується вдвічі, другій
/// стороні чекпоінт зсувається на нуль її балансу, і зароблене нею зникає.
#[test]
fn the_hook_called_outside_a_transfer_moves_no_checkpoint() {
    let (honest, doubled) = the_damage();

    let result = setup().process_and_validate_instruction(
        &execute_ix(BALANCE),
        &hook_world(
            bond_account(INVESTOR, BALANCE),
            bond_account(BUYER, BALANCE),
        ),
        &[club_err(ClubError::NotTransferring)],
    );

    let attacker = ledger_after(&result, INVESTOR);
    assert_eq!(
        attacker.accrued, 0,
        "нараховано без переказу: {doubled} замість {honest}"
    );
    assert_eq!(
        attacker.index_at_checkpoint, 0,
        "чекпоінт зрушено без переказу"
    );

    let victim = ledger_after(&result, BUYER);
    assert_eq!(victim.accrued, 0);
    assert_eq!(
        victim.index_at_checkpoint, 0,
        "чужий чекпоінт зсунуто: {honest} списано з власника, який нічого не робив"
    );
}

/// Одного піднятого прапорця замало — і саме це найтонше місце замка.
/// Справжній переказ підіймає прапорець на обох рахунках, тому рахунок із
/// **чужого** справжнього переказу — єдина «справжня» сторона, яку
/// атакувальник може десь узяти. Якби гук питав лише відправника, вистачило б
/// підсунути такий рахунок першим; якби лише отримувача — другим. Тому
/// перевіряються обидва напрямки.
#[test]
fn one_raised_flag_is_not_a_transfer() {
    for (who, source, destination) in [
        (
            "відправник стоїть у чужому переказі",
            mid_transfer(bond_account(INVESTOR, BALANCE)),
            bond_account(BUYER, BALANCE),
        ),
        (
            "отримувач стоїть у чужому переказі",
            bond_account(INVESTOR, BALANCE),
            mid_transfer(bond_account(BUYER, BALANCE)),
        ),
    ] {
        println!("прямий виклик гука, {who}");

        let result = setup().process_and_validate_instruction(
            &execute_ix(BALANCE),
            &hook_world(source, destination),
            &[club_err(ClubError::NotTransferring)],
        );

        assert_eq!(ledger_after(&result, INVESTOR).index_at_checkpoint, 0);
        assert_eq!(ledger_after(&result, BUYER).index_at_checkpoint, 0);
    }
}

/// Контроль до двох тестів вище: з піднятими прапорцями той самий виклик
/// проходить і списує рівно ту суму, про яку вони кажуть. Тобто відмовляють
/// вони на прапорці, а не на чомусь випадковому — не на розкладці акаунтів, не
/// на seeds і не на порожній претензії, які лишили б їх зеленими назавжди.
///
/// Це не дірка, а межа mollusk — та сама, що й з `is_signer` у тестах сховищ:
/// у тесті дані акаунта пишемо ми, на ланцюгу — лише Token-2022, і підняти там
/// прапорець не може ніхто, включно з власником рахунку.
#[test]
fn nothing_but_the_flag_stands_between_a_direct_call_and_the_ledger() {
    let (honest, doubled) = the_damage();

    let result = setup().process_and_validate_instruction(
        &execute_ix(BALANCE),
        &hook_world(
            mid_transfer(bond_account(INVESTOR, BALANCE)),
            mid_transfer(bond_account(BUYER, BALANCE)),
        ),
        &[Check::success()],
    );

    assert_eq!(
        ledger_after(&result, INVESTOR).accrued,
        doubled,
        "замок знято, а нарахування не подвоїлось — тест міряє не те"
    );
    assert_eq!(
        ledger_after(&result, BUYER).accrued,
        0,
        "замок знято, а {honest} у другої сторони вціліли — тест міряє не те"
    );
}
