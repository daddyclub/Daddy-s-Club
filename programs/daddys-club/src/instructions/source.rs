//! Джерело revenue: реєстрація (`FR-004`, `FR-028`).
//!
//! Реєстрація — це підключення перехоплення, а не заявка: з цієї миті джерело
//! накопичує історію (`FR-028`), і саме її тривалість потім вирішує, чи
//! допускається джерело до випуску (`FR-007`). Тому `first_seen_ts` береться з
//! годинника ланцюга, а не приходить аргументом.
//!
//! Тут же записується `authority` — ключ, чий підпис `intercept` прийматиме за
//! доказ, що дохід приніс сам емітент (`FR-004`). Ключ береться таким, яким
//! його дав емітент, і не перевіряється: PDA чужої програми не підписати, тому
//! записати можна будь-що, а скористатись — лише своїм.

use {
    crate::state::{ProtocolConfig, RevenueSource, CONFIG_SEED, SOURCE_SEED},
    anchor_lang::prelude::*,
    anchor_spl::token_interface::{Mint, TokenAccount},
};

#[derive(Accounts)]
#[instruction(seq: u64)]
pub struct RegisterSource<'info> {
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, ProtocolConfig>,

    /// Seeds містять підписанта, тому джерело в чужій namespace не створити.
    #[account(
        init,
        payer = issuer,
        space = 8 + RevenueSource::INIT_SPACE,
        seeds = [SOURCE_SEED, issuer.key().as_ref(), &seq.to_le_bytes()],
        bump,
    )]
    pub source: Account<'info, RevenueSource>,

    #[account(mut)]
    pub issuer: Signer<'info>,

    /// CHECK: сюди подається PDA програми-емітента, яка викликатиме
    /// перехоплення. Довести, що ключ справді належить її програмі, на цьому
    /// боці нічим — доказом є підпис, і його вимагає `intercept` (`FR-004`).
    pub authority: UncheckedAccount<'info>,

    #[account(address = config.usdc_mint)]
    pub usdc_mint: InterfaceAccount<'info, Mint>,

    /// Валюта звіряється при реєстрації, а не при першому розщепленні: рахунок
    /// у чужому мінті виявився б рівно тоді, коли комісія вже мусила пройти.
    #[account(token::mint = usdc_mint)]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    pub system_program: Program<'info, System>,
}

pub fn register_source(ctx: Context<RegisterSource>, seq: u64) -> Result<()> {
    let source = &mut ctx.accounts.source;

    source.issuer = ctx.accounts.issuer.key();
    source.authority = ctx.accounts.authority.key();
    source.vault = ctx.accounts.vault.key();
    source.first_seen_ts = Clock::get()?.unix_timestamp;
    source.total_observed = 0;
    source.observed_before_issue = 0;
    source.active_issue = None;
    source.seq = seq;
    source.bump = ctx.bumps.source;

    Ok(())
}
