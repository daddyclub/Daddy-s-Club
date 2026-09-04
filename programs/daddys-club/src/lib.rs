use anchor_lang::prelude::*;

declare_id!("7eT5T7mq1uD9piYJ2rMAzma8iYL7C7CZGPgxsB8DckoB");

pub mod errors;
pub mod instructions;
pub mod math;
pub mod state;

use instructions::*;

#[program]
pub mod daddys_club {
    use super::*;

    /// Створює параметри протоколу. Singleton: другий виклик упирається в
    /// `init` на тому самому PDA (`FR-036`).
    pub fn init_protocol(ctx: Context<InitProtocol>, params: ConfigParams) -> Result<()> {
        instructions::protocol::init_protocol(ctx, params)
    }

    /// Змінює параметри протоколу. На вже створені випуски зміна не діє —
    /// їхні умови зафіксовані в них самих (`FR-036`, `FR-002`).
    pub fn update_config(ctx: Context<UpdateConfig>, params: ConfigParams) -> Result<()> {
        instructions::protocol::update_config(ctx, params)
    }
}
