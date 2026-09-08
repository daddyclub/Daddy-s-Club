//! Випуск бонду: створення (`FR-001`…`FR-006`, `FR-013`).
//!
//! Тут умови стають незмінними. `FR-002` виконується не перевіркою, а
//! відсутністю: інструкції, яка редагує `Issue`, у програмі немає взагалі, тому
//! змінити умови не може ані емітент, ані адміністратор — навіть помилково.
//! Єдине, що дописується у випуск потім, — це рух погашення (`raised`,
//! `repaid_total`, `payout_index`, `state`), і жодне з цих полів умовою не є.
//!
//! Три речі створюються тут же, бо без них випуск не має сенсу окремо:
//!
//! - **мінт бонду** (`FR-013`) — окремий на кожен випуск, `decimals = 0`,
//!   authority — PDA випуску. Гук прибивається намертво: `program_id` показує
//!   на це ядро, а authority гука лишається порожньою, тому переставити або
//!   зняти гук неможливо нікому й ніколи. Саме це робить `FR-017` властивістю
//!   інструмента, а не нашого застосунку;
//! - **два USDC-сховища** — підписки (кошти інвесторів до закриття, `FR-008`) і
//!   погашення (перехоплена частка, `FR-014`). Обидва на authority випуску:
//!   підписати переказ звідти може лише програма;
//! - **`ExtraAccountMetaList`** — список, за яким Token-2022 сам добере акаунти
//!   для `execute`. Наповнюється тут, бо адреса випуску відома саме тут;
//!   читатиме його T032.
//!
//! Мінт і два сховища — звичайні акаунти, які емітент підписує при створенні, а
//! не PDA: їхні ключі лежать в `Issue`, і знайти їх можна з нього самого
//! (`FR-037`). Двома PDA-сховищами на одній валюті це й не вийшло б зробити
//! через ATA — адреса в них одна на пару «власник + мінт».
//!
//! Чого тут навмисно немає — порогу історії доходу (`FR-007`). Він приїде
//! окремою задачею (T040) разом зі своїм негативним тестом; до того випуск може
//! створити будь-хто, і віха M1 каже про це прямим текстом.

use {
    crate::{
        errors::ClubError,
        math,
        state::{
            Issue, IssueState, ProtocolConfig, RevenueSource, CONFIG_SEED, HOLDER_SEED, ISSUE_SEED,
            SOURCE_SEED,
        },
    },
    anchor_lang::prelude::*,
    anchor_spl::{
        token_2022::Token2022,
        token_interface::{Mint, TokenAccount},
    },
    spl_tlv_account_resolution::{
        account::ExtraAccountMeta, seeds::Seed, state::ExtraAccountMetaList,
    },
    spl_transfer_hook_interface::instruction::ExecuteInstruction,
};

/// `FR-013`: одиниця бонд-токена дорівнює одиниці номіналу в USDC, тому дробів
/// у бонда немає — частка у випуску міряється лотами, а не долями лота.
pub const BOND_DECIMALS: u8 = 0;

/// Seed списку додаткових акаунтів гука.
///
/// Байти належать не нам, а `spl-transfer-hook-interface`: Token-2022 шукає
/// список саме за ними. Тому константа стоїть тут, а не в `state.rs` — це не
/// наш seed, а чужий контракт, і в переписі seed-констант протоколу їй не
/// місце. Що байти ті самі, доводить `the_metas_seed_is_the_one_token_2022_looks_for`.
pub const EXTRA_METAS_SEED: &[u8] = b"extra-account-metas";

/// Скільки акаунтів понад п'ять обов'язкових Token-2022 добере для `execute`.
pub const EXTRA_ACCOUNT_METAS: usize = 3;

// Розкладка акаунтів у виклику `execute`, яку задає Token-2022. Індекси
// наскрізні — додаткові акаунти нумеруються далі за обов'язковими, і саме на
// них посилається `Seed::AccountKey`:
//
// ```text
// 0  source_token       рахунок відправника
// 1  bond_mint
// 2  destination_token  рахунок отримувача
// 3  owner              authority відправника
// 4  extra_metas        цей список
// 5  issue              прибитий адресою: вона відома вже при створенні
// 6  holder(відправник)
// 7  holder(отримувач)
// ```
const HOOK_SOURCE_TOKEN_INDEX: u8 = 0;
const HOOK_DESTINATION_TOKEN_INDEX: u8 = 2;
const HOOK_ISSUE_INDEX: u8 = 5;

/// Зміщення `owner` у токен-акаунті: спершу `mint` (32 байти), далі `owner`.
///
/// Береться саме воно, а не ключ authority з акаунта 3: authority відправника
/// може бути делегатом, а в отримувача authority в наборі взагалі немає. Власник
/// же записаний у самому рахунку — і в того, і в того.
const TOKEN_ACCOUNT_OWNER_OFFSET: u8 = 32;
const PUBKEY_LEN: u8 = 32;

/// Умови випуску, які задає емітент (`FR-001`). Після запису не змінюються
/// (`FR-002`).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct IssueParams {
    /// Сума збору в USDC. Вона ж — уся пропозиція бонду (`FR-013`).
    pub face: u64,
    /// `FR-018`: купон рахується один раз, на дату створення.
    pub coupon_bps: u16,
    /// `FR-005`: частка перехоплення, не вища за стелю протоколу.
    pub pledge_bps: u16,
    /// `FR-003`.
    pub maturity_ts: i64,
    /// `FR-008`: доки приймаються внески.
    pub subscription_end_ts: i64,
    /// `FR-009`: мінімальний внесок.
    pub min_lot: u64,
}

impl IssueParams {
    fn validate(&self, config: &ProtocolConfig, now: i64) -> Result<()> {
        // `FR-001`: номінал і лот задає емітент, але номінал мусить розкладатись
        // на цілі лоти. Інакше останній внесок ніколи не дійшов би до рівності
        // `raised == face`, якої вимагає `FR-010`: залишок, менший за лот,
        // приймати не можна, а не прийняти означає не зібрати.
        require!(self.face > 0, ClubError::FaceAmountInvalid);
        require!(
            self.min_lot > 0 && self.min_lot <= self.face,
            ClubError::LotSizeInvalid
        );
        require!(self.face % self.min_lot == 0, ClubError::FaceAmountInvalid);

        // `FR-003`: строк рахується від «зараз», а не від закриття підписки —
        // купон нараховано на всю дистанцію від створення.
        let tenor = self
            .maturity_ts
            .checked_sub(now)
            .ok_or(ClubError::MathOverflow)?;
        require!(
            tenor >= config.min_tenor_secs && tenor <= config.max_tenor_secs,
            ClubError::TermOutOfRange
        );

        // Вікно, що закінчилось до створення, не дає підписатись нікому; вікно
        // після погашення дало б підписатись у вже прострочений випуск.
        require!(
            self.subscription_end_ts > now && self.subscription_end_ts < self.maturity_ts,
            ClubError::SubscriptionWindowInvalid
        );

        // `FR-005`. Нульова частка тут не відхиляється: вимоги на це немає, і
        // глухим кутом вона не є — `FR-022` після дати погашення підніме
        // перехоплення до стелі протоколу, і випуск усе одно погаситься.
        require!(
            self.pledge_bps <= config.max_pledge_bps,
            ClubError::PledgeAboveCap
        );

        Ok(())
    }

    /// `FR-018`: номінал + купон, зафіксовані назавжди.
    fn obligation_total(&self) -> Result<u64> {
        let total = math::obligation_total(u128::from(self.face), self.coupon_bps)
            .ok_or(ClubError::MathOverflow)?;

        u64::try_from(total).map_err(|_| error!(ClubError::MathOverflow))
    }
}

/// Додаткові акаунти, які Token-2022 добере для `execute` (`FR-017`).
///
/// Випуск прибивається адресою, а не seeds: його власні seeds містять `seq`,
/// якого в наборі акаунтів переказу немає взагалі. Зате адреса відома вже тут,
/// у створенні, і мінт із випуском пов'язані назавжди — на кожен випуск свій
/// мінт, а на кожен мінт свій список.
///
/// Чекпоінти обох сторін резолвляться від цієї адреси і від власника,
/// прочитаного з самого токен-акаунта.
pub fn hook_account_metas(issue: &Pubkey) -> Result<[ExtraAccountMeta; EXTRA_ACCOUNT_METAS]> {
    let holder_of = |token_account_index: u8| {
        [
            Seed::Literal {
                bytes: HOLDER_SEED.to_vec(),
            },
            Seed::AccountKey {
                index: HOOK_ISSUE_INDEX,
            },
            Seed::AccountData {
                account_index: token_account_index,
                data_index: TOKEN_ACCOUNT_OWNER_OFFSET,
                length: PUBKEY_LEN,
            },
        ]
    };

    Ok([
        ExtraAccountMeta::new_with_pubkey(issue, false, false)?,
        ExtraAccountMeta::new_with_seeds(&holder_of(HOOK_SOURCE_TOKEN_INDEX), false, true)?,
        ExtraAccountMeta::new_with_seeds(&holder_of(HOOK_DESTINATION_TOKEN_INDEX), false, true)?,
    ])
}

#[derive(Accounts)]
#[instruction(seq: u64)]
pub struct CreateIssue<'info> {
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, ProtocolConfig>,

    /// Seeds містять підписанта, тому під чуже джерело випуск не створити:
    /// набір не зійдеться ще до тіла інструкції.
    #[account(
        mut,
        seeds = [SOURCE_SEED, issuer.key().as_ref(), &source.seq.to_le_bytes()],
        bump = source.bump,
    )]
    pub source: Account<'info, RevenueSource>,

    #[account(
        init,
        payer = issuer,
        space = 8 + Issue::INIT_SPACE,
        seeds = [ISSUE_SEED, source.key().as_ref(), &seq.to_le_bytes()],
        bump,
    )]
    pub issue: Account<'info, Issue>,

    #[account(mut)]
    pub issuer: Signer<'info>,

    #[account(address = config.usdc_mint)]
    pub usdc_mint: InterfaceAccount<'info, Mint>,

    /// `FR-013`: окремий мінт на кожен випуск. Authority — PDA випуску, тому
    /// друкувати бонд уміє лише програма, і лише там, де це дозволено станом.
    ///
    /// Authority гука не задається навмисно: без неї `program_id` гука
    /// незмінний назавжди. Freeze authority немає з тієї ж причини — заморожений
    /// бонд не передається, а `FR-017` обіцяє передачу з обліком, а не заборону.
    #[account(
        init,
        payer = issuer,
        mint::decimals = BOND_DECIMALS,
        mint::authority = issue,
        mint::token_program = token_program,
        extensions::transfer_hook::program_id = crate::ID,
    )]
    pub bond_mint: InterfaceAccount<'info, Mint>,

    /// `FR-008`: внески лежать тут і не доступні емітенту до закриття.
    #[account(
        init,
        payer = issuer,
        token::mint = usdc_mint,
        token::authority = issue,
        token::token_program = token_program,
    )]
    pub subscription_vault: InterfaceAccount<'info, TokenAccount>,

    /// `FR-014`: сюди відщеплюється перехоплена частка.
    #[account(
        init,
        payer = issuer,
        token::mint = usdc_mint,
        token::authority = issue,
        token::token_program = token_program,
    )]
    pub escrow_vault: InterfaceAccount<'info, TokenAccount>,

    /// CHECK: TLV-список, а не акаунт Anchor — його розкладку задає
    /// `spl-tlv-account-resolution`, і дискримінатора Anchor у ньому бути не
    /// повинно. Створює його `init` (власник — ця програма, розмір під три
    /// меты), наповнює тіло інструкції, читає Token-2022.
    #[account(
        init,
        payer = issuer,
        space = ExtraAccountMetaList::size_of(EXTRA_ACCOUNT_METAS)?,
        seeds = [EXTRA_METAS_SEED, bond_mint.key().as_ref()],
        bump,
    )]
    pub extra_account_meta_list: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token2022>,

    pub system_program: Program<'info, System>,
}

pub fn create_issue(ctx: Context<CreateIssue>, seq: u64, params: IssueParams) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    params.validate(&ctx.accounts.config, now)?;

    // `FR-006`: два випуски на один потік конкурували б за ті самі гроші без
    // визначеної черговості. Перевірка тут, а не в `intercept`: розщеплювати
    // навпіл уже зібрані кошти нічим.
    require!(
        ctx.accounts.source.active_issue.is_none(),
        ClubError::SourceAlreadyPledged
    );

    let obligation_total = params.obligation_total()?;
    let issue_key = ctx.accounts.issue.key();

    let issue = &mut ctx.accounts.issue;
    issue.source = ctx.accounts.source.key();
    issue.bond_mint = ctx.accounts.bond_mint.key();
    issue.escrow_vault = ctx.accounts.escrow_vault.key();
    issue.subscription_vault = ctx.accounts.subscription_vault.key();
    issue.face = params.face;
    issue.coupon_bps = params.coupon_bps;
    issue.pledge_bps = params.pledge_bps;
    issue.maturity_ts = params.maturity_ts;
    issue.subscription_end_ts = params.subscription_end_ts;
    issue.min_lot = params.min_lot;
    issue.raised = 0;
    issue.obligation_total = obligation_total;
    issue.repaid_total = 0;
    issue.payout_index = 0;
    issue.state = IssueState::Subscribing;
    issue.seq = seq;
    issue.bump = ctx.bumps.issue;

    let source = &mut ctx.accounts.source;
    source.active_issue = Some(issue_key);
    // `FR-030` порівнює темп доходу з тим, що був до випуску, тому зріз
    // знімається саме зараз — потім відрізнити «до» від «після» вже нічим.
    source.observed_before_issue = source.total_observed;

    let metas = hook_account_metas(&issue_key)?;
    let mut data = ctx.accounts.extra_account_meta_list.try_borrow_mut_data()?;
    ExtraAccountMetaList::init::<ExecuteInstruction>(&mut data[..], &metas)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use {super::*, spl_transfer_hook_interface::get_extra_account_metas_address};

    const NOW: i64 = 1_800_000_000;
    const DAY: i64 = 86_400;

    fn config() -> ProtocolConfig {
        ProtocolConfig {
            admin: Pubkey::default(),
            origination_fee_bps: 150,
            trading_fee_bps: 50,
            max_pledge_bps: 3_000,
            min_tenor_secs: 30 * DAY,
            max_tenor_secs: 180 * DAY,
            history_threshold_secs: 7 * DAY,
            usdc_mint: Pubkey::default(),
            fee_vault: Pubkey::default(),
            bump: 255,
        }
    }

    fn params() -> IssueParams {
        IssueParams {
            face: 250_000_000_000,
            coupon_bps: 950,
            pledge_bps: 1_200,
            maturity_ts: NOW + 90 * DAY,
            subscription_end_ts: NOW + 7 * DAY,
            min_lot: 1_000_000,
        }
    }

    /// Seed списку переписаний байтами, бо константа в
    /// `spl-transfer-hook-interface` приватна. Якщо перепис розійдеться,
    /// Token-2022 шукатиме список не там, де ми його створили, — і жоден переказ
    /// бонду не пройде взагалі.
    #[test]
    fn the_metas_seed_is_the_one_token_2022_looks_for() {
        let mint = Pubkey::new_from_array([23u8; 32]);

        assert_eq!(
            Pubkey::find_program_address(&[EXTRA_METAS_SEED, mint.as_ref()], &crate::ID).0,
            get_extra_account_metas_address(&mint, &crate::ID)
        );
    }

    #[test]
    fn valid_terms_pass() {
        assert!(params().validate(&config(), NOW).is_ok());
    }

    #[test]
    fn a_face_that_does_not_split_into_whole_lots_is_refused() {
        // `FR-010` вимагає рівності `raised == face`. Номінал, який не ділиться
        // на лот, лишає хвіст, менший за лот: прийняти його не можна, а не
        // прийняти — значить не зібрати ніколи.
        let odd = IssueParams {
            face: 1_000_001,
            min_lot: 1_000_000,
            ..params()
        };

        assert!(odd.validate(&config(), NOW).is_err());
        assert!(IssueParams { face: 0, ..params() }
            .validate(&config(), NOW)
            .is_err());
    }

    #[test]
    fn a_lot_that_is_zero_or_bigger_than_the_face_is_refused() {
        for min_lot in [0, 250_000_000_001] {
            assert!(IssueParams {
                min_lot,
                ..params()
            }
            .validate(&config(), NOW)
            .is_err());
        }
    }

    /// `FR-003`: діапазон — властивість продукту, а не побажання. Обидва краї
    /// перевіряються включно: рівно 30 і рівно 180 днів мають проходити.
    #[test]
    fn the_term_range_is_closed_at_both_ends() {
        let config = config();

        for offset in [config.min_tenor_secs, config.max_tenor_secs] {
            let at_the_edge = IssueParams {
                maturity_ts: NOW + offset,
                subscription_end_ts: NOW + 1,
                ..params()
            };
            assert!(at_the_edge.validate(&config, NOW).is_ok(), "{offset} с");
        }

        for offset in [config.min_tenor_secs - 1, config.max_tenor_secs + 1] {
            let outside = IssueParams {
                maturity_ts: NOW + offset,
                subscription_end_ts: NOW + 1,
                ..params()
            };
            assert!(outside.validate(&config, NOW).is_err(), "{offset} с");
        }
    }

    #[test]
    fn a_window_that_closes_outside_the_life_of_the_issue_is_refused() {
        for subscription_end_ts in [NOW, NOW + 90 * DAY, NOW + 91 * DAY] {
            assert!(IssueParams {
                subscription_end_ts,
                ..params()
            }
            .validate(&config(), NOW)
            .is_err());
        }
    }

    #[test]
    fn a_share_above_the_protocol_cap_is_refused() {
        let config = config();

        assert!(IssueParams {
            pledge_bps: config.max_pledge_bps,
            ..params()
        }
        .validate(&config, NOW)
        .is_ok());

        assert!(IssueParams {
            pledge_bps: config.max_pledge_bps + 1,
            ..params()
        }
        .validate(&config, NOW)
        .is_err());
    }

    #[test]
    fn the_obligation_is_face_plus_coupon() {
        assert_eq!(params().obligation_total().unwrap(), 273_750_000_000);
    }

    /// Зобов'язання лягає в `u64`, а рахується в `u128`: номінал біля стелі
    /// `u64` із купоном туди вже не вміщується, і мовчки обрізаний залишок
    /// означав би борг, менший за номінал.
    #[test]
    fn an_obligation_that_does_not_fit_the_account_is_an_error() {
        assert!(IssueParams {
            face: u64::MAX,
            coupon_bps: 1,
            ..params()
        }
        .obligation_total()
        .is_err());
    }

    /// Головне, що список мусить уміти: привести Token-2022 рівно до тих
    /// чекпоінтів, які веде програма. Резолв проганяється тут, а не в тесті на
    /// ланцюгу, бо саме тут видно, звідки береться кожен seed.
    #[test]
    fn the_hook_list_resolves_to_the_checkpoints_of_both_sides() {
        let issue = Pubkey::new_from_array([7u8; 32]);
        let sender = Pubkey::new_from_array([8u8; 32]);
        let recipient = Pubkey::new_from_array([9u8; 32]);

        // Токен-акаунт: `mint` (32 байти), далі `owner`. Решта для резолву не
        // потрібна, тому й не заповнюється.
        let mut source_token = [0u8; 72];
        source_token[32..64].copy_from_slice(sender.as_ref());
        let mut destination_token = [0u8; 72];
        destination_token[32..64].copy_from_slice(recipient.as_ref());
        let mint = Pubkey::new_from_array([23u8; 32]);

        let accounts = |index: usize| -> Option<(&Pubkey, Option<&[u8]>)> {
            match index {
                0 => Some((&mint, Some(&source_token[..]))),
                2 => Some((&mint, Some(&destination_token[..]))),
                5 => Some((&issue, None)),
                _ => None,
            }
        };

        let metas = hook_account_metas(&issue).unwrap();

        for (meta, owner) in [(&metas[1], sender), (&metas[2], recipient)] {
            let resolved = meta.resolve(&[], &crate::ID, accounts).unwrap();

            assert_eq!(
                resolved.pubkey,
                Pubkey::find_program_address(
                    &[HOLDER_SEED, issue.as_ref(), owner.as_ref()],
                    &crate::ID
                )
                .0
            );
        }
    }

    #[test]
    fn the_hook_reads_the_owner_out_of_each_token_account() {
        let metas = hook_account_metas(&Pubkey::new_from_array([7u8; 32])).unwrap();

        assert_eq!(metas.len(), EXTRA_ACCOUNT_METAS);
        // Випуск прибитий адресою — дискримінатор 0; чекпоінти дерівуються
        // цією ж програмою — дискримінатор 1.
        assert_eq!(metas[0].discriminator, 0);
        assert_eq!(metas[1].discriminator, 1);
        assert_eq!(metas[2].discriminator, 1);
        // Чекпоінти мусять бути записуваними: гук у них і пише.
        assert!(bool::from(metas[1].is_writable));
        assert!(bool::from(metas[2].is_writable));
        assert!(!bool::from(metas[0].is_signer));
    }
}
