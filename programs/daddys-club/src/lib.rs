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

    /// Підключає джерело revenue: з цієї миті воно накопичує історію
    /// (`FR-028`), а `intercept` знає, чий підпис приймати (`FR-004`).
    pub fn register_source(ctx: Context<RegisterSource>, seq: u64) -> Result<()> {
        instructions::source::register_source(ctx, seq)
    }

    /// Створює випуск під зареєстроване джерело: фіксує умови назавжди
    /// (`FR-001`, `FR-002`), займає джерело (`FR-006`) і випускає власний мінт
    /// бонду з незмінним гуком (`FR-013`).
    pub fn create_issue(ctx: Context<CreateIssue>, seq: u64, params: IssueParams) -> Result<()> {
        instructions::issue::create_issue(ctx, seq, params)
    }

    /// Відкриває облік за випуском. Саме існування цього акаунта й означає, що
    /// гаманцеві можна передати бонд: список акаунтів гука резолвить його при
    /// кожному переказі, і без нього передача відхиляється цілком (`FR-038`).
    pub fn open_position(ctx: Context<OpenPosition>) -> Result<()> {
        instructions::invest::open_position(ctx)
    }

    /// Приймає внесок у випуск і друкує бонд у тій самій транзакції
    /// (`FR-008`, `FR-009`, `FR-013`). `amount` — пропозиція: приймається
    /// стільки, скільки лишилось нерозібраного номіналу, і на повному зборі
    /// випуск стає `Funded` (`FR-010`).
    pub fn subscribe(ctx: Context<Subscribe>, amount: u64) -> Result<()> {
        instructions::invest::subscribe(ctx, amount)
    }

    /// Видає емітенту зібраний номінал за вирахуванням origination fee
    /// (`FR-012`, `FR-034`). Гроші беруться зі сховища підписки, комісія йде у
    /// скарбницю протоколу, а випуск виходить звідси в `Repaying`: з цієї миті
    /// зобов'язання існує, і перехоплення має сенс.
    pub fn withdraw_proceeds(ctx: Context<WithdrawProceeds>) -> Result<()> {
        instructions::issue::withdraw_proceeds(ctx)
    }

    /// Повертає інвесторові внесок із недозібраного випуску: бонд палиться,
    /// гроші йдуть назад зі сховища підписки, origination fee не утримується
    /// (`FR-011`). Перший виклик після закриття вікна й позначає випуск
    /// недозібраним — окремої інструкції на це немає, бо стан на ланцюгу
    /// однаково лишається старим, доки хтось не надішле транзакцію.
    pub fn refund(ctx: Context<Refund>) -> Result<()> {
        instructions::invest::refund(ctx)
    }
}
