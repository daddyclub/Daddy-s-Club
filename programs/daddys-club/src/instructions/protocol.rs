//! Параметри протоколу: створення і зміна (`FR-036`).
//!
//! Обидві інструкції приймають один і той самий `ConfigParams` і перевіряють
//! його однією функцією. Це не економія рядків: набір, який не можна створити,
//! не повинен ставати доступним через зміну, а дві копії правил розходяться
//! саме в той бік, який ніхто не ганяє тестом.
//!
//! **Зміна не застосовується до вже створених випусків** (`FR-036`). Тут це
//! видно з набору акаунтів: `update_config` не бачить жодного `Issue`, тому
//! дотягнутись до нього не може навіть помилково. Умови випуску фіксуються в
//! ньому самому при створенні (`FR-002`).
//!
//! Чого тут навмисно немає — передачі адміністративних прав, зміни розрахункової
//! валюти і скарбниці комісій. `FR-036` перелічує рівно чотири групи параметрів
//! (ставки комісій, стеля частки перехоплення, діапазон строків, поріг допуску),
//! а вимоги на решту немає.

use {
    crate::{
        errors::ClubError,
        math::BPS_DENOM,
        state::{ProtocolConfig, CONFIG_SEED},
    },
    anchor_lang::prelude::*,
    anchor_spl::token_interface::{Mint, TokenAccount},
};

/// `FR-034`: origination fee — 1…2%.
pub const MIN_ORIGINATION_FEE_BPS: u16 = 100;
pub const MAX_ORIGINATION_FEE_BPS: u16 = 200;

/// `FR-003`: діапазон строків протоколу мусить сам уміститись у 30…180 днів.
pub const MIN_TENOR_SECS: i64 = 30 * 86_400;
pub const MAX_TENOR_SECS: i64 = 180 * 86_400;

/// Параметри, які адміністратор виставляє й потім змінює (`FR-036`).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigParams {
    /// `FR-034`.
    pub origination_fee_bps: u16,
    /// `FR-035`.
    pub trading_fee_bps: u16,
    /// `FR-005`, він же ставка перехоплення в past due (`FR-022`).
    pub max_pledge_bps: u16,
    /// `FR-003`.
    pub min_tenor_secs: i64,
    pub max_tenor_secs: i64,
    /// `FR-007`.
    pub history_threshold_secs: i64,
}

impl ConfigParams {
    fn validate(&self) -> Result<()> {
        require!(
            (MIN_ORIGINATION_FEE_BPS..=MAX_ORIGINATION_FEE_BPS)
                .contains(&self.origination_fee_bps),
            ClubError::FeeOutOfRange
        );

        // Діапазону торговій комісії спека не задає — `FR-035` каже лише, що
        // вона параметр. Стеля тут не політика, а арифметика: комісія понад
        // усю ціну не лишає продавцю чого віддати, і угода стає невиконанною
        // ще до того, як хтось її підпише.
        require!(
            u128::from(self.trading_fee_bps) <= BPS_DENOM,
            ClubError::FeeOutOfRange
        );

        // `FR-005`: нульова стеля забороняє будь-який випуск, стеля понад увесь
        // потік перестає бути стелею. Ані те, ані те не є параметром.
        require!(
            self.max_pledge_bps > 0 && u128::from(self.max_pledge_bps) <= BPS_DENOM,
            ClubError::PledgeCapOutOfRange
        );

        require!(
            self.min_tenor_secs >= MIN_TENOR_SECS
                && self.max_tenor_secs <= MAX_TENOR_SECS
                && self.min_tenor_secs <= self.max_tenor_secs,
            ClubError::TermRangeOutOfBounds
        );

        // `FR-007`: нульовий поріг означає, що допуску немає взагалі, а
        // перевірка історії має бути безумовною.
        require!(
            self.history_threshold_secs > 0,
            ClubError::HistoryThresholdInvalid
        );

        Ok(())
    }

    fn apply_to(&self, config: &mut ProtocolConfig) {
        config.origination_fee_bps = self.origination_fee_bps;
        config.trading_fee_bps = self.trading_fee_bps;
        config.max_pledge_bps = self.max_pledge_bps;
        config.min_tenor_secs = self.min_tenor_secs;
        config.max_tenor_secs = self.max_tenor_secs;
        config.history_threshold_secs = self.history_threshold_secs;
    }
}

#[derive(Accounts)]
pub struct InitProtocol<'info> {
    #[account(
        init,
        payer = admin,
        space = 8 + ProtocolConfig::INIT_SPACE,
        seeds = [CONFIG_SEED],
        bump,
    )]
    pub config: Account<'info, ProtocolConfig>,

    /// Той, хто створює конфіг, ним і розпоряджається далі.
    #[account(mut)]
    pub admin: Signer<'info>,

    /// Розрахункова валюта протоколу — одна на все.
    pub usdc_mint: InterfaceAccount<'info, Mint>,

    /// Куди йдуть origination fee (`FR-034`) і торгова комісія (`FR-035`).
    /// Валюта звіряється тут, а не при виплаті: скарбниця в чужому мінті
    /// виявилася б лише в момент, коли перший переказ уже мусив пройти.
    #[account(token::mint = usdc_mint)]
    pub fee_vault: InterfaceAccount<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    /// `has_one` і є перевіркою прав: чужий підпис не доходить до тіла
    /// інструкції взагалі.
    #[account(
        mut,
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = admin @ ClubError::Unauthorized,
    )]
    pub config: Account<'info, ProtocolConfig>,

    pub admin: Signer<'info>,
}

pub fn init_protocol(ctx: Context<InitProtocol>, params: ConfigParams) -> Result<()> {
    params.validate()?;

    let config = &mut ctx.accounts.config;
    config.admin = ctx.accounts.admin.key();
    config.usdc_mint = ctx.accounts.usdc_mint.key();
    config.fee_vault = ctx.accounts.fee_vault.key();
    config.bump = ctx.bumps.config;
    params.apply_to(config);

    Ok(())
}

pub fn update_config(ctx: Context<UpdateConfig>, params: ConfigParams) -> Result<()> {
    params.validate()?;
    params.apply_to(&mut ctx.accounts.config);

    Ok(())
}
