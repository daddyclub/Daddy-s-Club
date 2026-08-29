//! Референсна інтеграція: мінімальний своп, який утримує комісію і викликає
//! перехоплення в тій самій транзакції (FR-004, FR-014).
//!
//! Реальної AMM-математики, оракулів ціни й захисту від MEV тут немає і не
//! планується — єдине призначення програми виробляти справжній потік комісій.

use anchor_lang::prelude::*;

declare_id!("8wKjGiLvnMTv7oi9PcztmbRv4v63emT2qPPrA8x1fW3z");

#[program]
pub mod demo_issuer {
    use super::*;

    pub fn swap(_ctx: Context<Noop>, _amount_in: u64) -> Result<()> {
        err!(DemoError::NotImplemented)
    }
}

#[derive(Accounts)]
pub struct Noop<'info> {
    pub payer: Signer<'info>,
}

#[error_code]
pub enum DemoError {
    #[msg("Інструкція ще не імплементована")]
    NotImplemented,
}
