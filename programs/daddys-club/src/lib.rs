use anchor_lang::prelude::*;

declare_id!("7eT5T7mq1uD9piYJ2rMAzma8iYL7C7CZGPgxsB8DckoB");

pub mod errors;
pub mod math;
pub mod state;

#[program]
pub mod daddys_club {
    use super::*;

    pub fn init_protocol(_ctx: Context<Noop>) -> Result<()> {
        err!(errors::ClubError::NotImplemented)
    }
}

#[derive(Accounts)]
pub struct Noop<'info> {
    pub payer: Signer<'info>,
}
