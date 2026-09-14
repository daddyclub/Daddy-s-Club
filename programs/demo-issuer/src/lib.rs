//! Референсна інтеграція: мінімальний своп, який утримує комісію і викликає
//! перехоплення в тій самій транзакції (`FR-004`, `FR-014`).
//!
//! Реальної AMM-математики, оракулів ціни й захисту від MEV тут немає і не
//! планується — єдине призначення програми виробляти справжній потік комісій.
//! Курс фіксований: за `amount_in` USDC трейдер отримує стільки ж базового
//! токена, скільки лишилось після комісії, одиниця в одиницю.
//!
//! Ця програма — та половина `FR-004`, яку в реальному світі пише сам емітент:
//! вимога каже, що виклик перехоплення **вбудовується у власну інструкцію, що
//! збирає комісії**, і без другої програми цю механіку неможливо ані показати,
//! ані перевірити. Вона ж доводить `SC-001`: у шляху погашення немає ані
//! другої транзакції, ані зовнішнього виконавця — своп трейдера і є той
//! єдиний підпис, який усе рухає.
//!
//! Порядок дій у `swap` — не стиль, а умова роботи: комісія лягає на рахунок
//! джерела **до** CPI, бо ядро списує частку з уже наповненого рахунку.
//!
//! Автентифікує нас перед ядром **підпис** PDA цієї програми (`pool`).
//! Підписати його не може ніхто інший, тому саме він, а не переданий program
//! id, є доказом походження доходу. Seeds цього PDA заморожені: його адреса
//! записана в `RevenueSource.authority` при реєстрації джерела, і зміна seeds
//! відрізала б від погашення всі вже зареєстровані джерела.

use {
    anchor_lang::prelude::*,
    anchor_spl::{
        token_2022::{transfer_checked, Token2022, TransferChecked},
        token_interface::{Mint, TokenAccount},
    },
    daddys_club::{cpi::accounts::Intercept, math::BPS_DENOM, program::DaddysClub},
};

declare_id!("8wKjGiLvnMTv7oi9PcztmbRv4v63emT2qPPrA8x1fW3z");

/// Seeds єдиного пулу демо-світу. Лічильника в них немає навмисно: другий пул
/// цій програмі не потрібен ані для демо (`SC-006`), ані для заміру
/// обчислювальних одиниць (`SC-005`), а seed без вимоги коштував би дорожче за
/// свою користь — після реєстрації джерела ці байти вже не змінити.
pub const POOL_SEED: &[u8] = b"pool";

/// Комісія свопу — 0.3% від входу. Класична ставка AMM, і тут вона константа, а
/// не параметр: у пулу немає стану, який хтось мусив би створювати й
/// налаштовувати перед показом.
pub const FEE_BPS: u16 = 30;

#[program]
pub mod demo_issuer {
    use super::*;

    /// Своп USDC → базовий токен з утриманням комісії і негайним її
    /// розщепленням (`FR-004`).
    ///
    /// Чотири дії в одній інструкції, і порядок серед них має значення лише
    /// один раз: комісія мусить опинитись на рахунку джерела до виклику
    /// перехоплення, бо ядро бере частку саме звідти (`FR-014`). Решта комісії
    /// нікуди не рухається — рахунок джерела належить цій же програмі.
    ///
    /// Перехоплення викликається **завжди**, навіть коли комісія округлилась у
    /// нуль: гілка «якщо є що ділити» зробила б історію джерела (`FR-028`)
    /// залежною від розміру свопу, а `FR-030` порівнює саме темпи доходу.
    ///
    /// Відмова ядра валить увесь своп — і це правильний бік компромісу для
    /// емітента: він відмовляється від угоди, а не пускає комісію повз
    /// зобов'язання, яке сам на себе взяв.
    pub fn swap(ctx: Context<Swap>, amount_in: u64) -> Result<()> {
        // Уся арифметика — checked_* у u128, округлення вниз: відкинутий
        // залишок лишається трейдеру, а не створюється з повітря.
        let fee = u128::from(amount_in)
            .checked_mul(u128::from(FEE_BPS))
            .and_then(|scaled| scaled.checked_div(BPS_DENOM))
            .ok_or(DemoError::MathOverflow)?;
        let fee = u64::try_from(fee).map_err(|_| error!(DemoError::MathOverflow))?;
        let amount_out = amount_in.checked_sub(fee).ok_or(DemoError::MathOverflow)?;

        let usdc_decimals = ctx.accounts.usdc_mint.decimals;
        let base_decimals = ctx.accounts.base_mint.decimals;

        let bump = [ctx.bumps.pool];
        let pool_signs: &[&[&[u8]]] = &[&[POOL_SEED, &bump]];

        // 1. Комісія — на рахунок джерела. Цей переказ стоїть перед CPI, бо
        //    перехоплення списує частку з рахунку, який уже наповнений.
        transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.trader_usdc.to_account_info(),
                    mint: ctx.accounts.usdc_mint.to_account_info(),
                    to: ctx.accounts.fee_vault.to_account_info(),
                    authority: ctx.accounts.trader.to_account_info(),
                },
            ),
            fee,
            usdc_decimals,
        )?;

        // 2. Решта входу — в резерв пулу.
        transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.trader_usdc.to_account_info(),
                    mint: ctx.accounts.usdc_mint.to_account_info(),
                    to: ctx.accounts.pool_usdc.to_account_info(),
                    authority: ctx.accounts.trader.to_account_info(),
                },
            ),
            amount_out,
            usdc_decimals,
        )?;

        // 3. Вихід — трейдеру, підписом пулу.
        transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.pool_base.to_account_info(),
                    mint: ctx.accounts.base_mint.to_account_info(),
                    to: ctx.accounts.trader_base.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                },
                pool_signs,
            ),
            amount_out,
            base_decimals,
        )?;

        // 4. `FR-004`: перехоплення тут же, у тій самій транзакції. У ядро йде
        //    розмір комісії, а не обсяг свопу: розщеплюється дохід протоколу, а
        //    не гроші трейдера, які лише проходять крізь пул.
        daddys_club::cpi::intercept(
            CpiContext::new_with_signer(
                ctx.accounts.club_program.to_account_info(),
                Intercept {
                    source: ctx.accounts.source.to_account_info(),
                    authority: ctx.accounts.pool.to_account_info(),
                    vault: ctx.accounts.fee_vault.to_account_info(),
                    issue: ctx.accounts.issue.as_ref().map(|a| a.to_account_info()),
                    escrow_vault: ctx
                        .accounts
                        .escrow_vault
                        .as_ref()
                        .map(|a| a.to_account_info()),
                    bond_mint: ctx.accounts.bond_mint.as_ref().map(|a| a.to_account_info()),
                    usdc_mint: ctx.accounts.usdc_mint.to_account_info(),
                    token_program: ctx.accounts.token_program.to_account_info(),
                },
                pool_signs,
            ),
            fee,
        )
    }
}

#[derive(Accounts)]
pub struct Swap<'info> {
    /// CHECK: PDA пулу. Даних не має і не потребує: його роль — підпис. Він
    /// же розпоряджається резервами й рахунком джерела, і саме його адресу
    /// емітент подає в `register_source` як `authority`.
    #[account(seeds = [POOL_SEED], bump)]
    pub pool: UncheckedAccount<'info>,

    /// Єдиний підпис у всьому шляху погашення (`SC-001`).
    pub trader: Signer<'info>,

    #[account(mut, token::mint = usdc_mint, token::authority = trader)]
    pub trader_usdc: InterfaceAccount<'info, TokenAccount>,

    #[account(mut, token::mint = base_mint, token::authority = trader)]
    pub trader_base: InterfaceAccount<'info, TokenAccount>,

    #[account(mut, token::mint = usdc_mint, token::authority = pool)]
    pub pool_usdc: InterfaceAccount<'info, TokenAccount>,

    #[account(mut, token::mint = base_mint, token::authority = pool)]
    pub pool_base: InterfaceAccount<'info, TokenAccount>,

    /// Рахунок джерела — той самий, який записаний у `RevenueSource.vault`.
    /// Сюди лягає комісія, і звідси ядро бере частку. Що це справді рахунок
    /// цього джерела, звіряє саме ядро (`has_one = vault`): дублювати перевірку
    /// тут означало б мати два місця, де вона може розійтись.
    #[account(mut, token::mint = usdc_mint, token::authority = pool)]
    pub fee_vault: InterfaceAccount<'info, TokenAccount>,

    /// CHECK: акаунт ядра, який їде в CPI як є. Розбирає й перевіряє його
    /// ядро — емітент не має ані типів, ані права судити про чужий стан.
    #[account(mut)]
    pub source: UncheckedAccount<'info>,

    /// CHECK: те саме. Трійця `issue`/`escrow_vault`/`bond_mint` подається
    /// або вся, або жодна: без випуску дохід лише спостерігається (`FR-028`).
    #[account(mut)]
    pub issue: Option<UncheckedAccount<'info>>,

    /// CHECK: те саме.
    #[account(mut)]
    pub escrow_vault: Option<UncheckedAccount<'info>>,

    /// CHECK: те саме.
    pub bond_mint: Option<UncheckedAccount<'info>>,

    pub usdc_mint: InterfaceAccount<'info, Mint>,

    /// Другий бік свопу. Протоколу він не цікавий — комісія й перехоплення
    /// живуть у USDC, — але без нього це був би не своп, а збір комісії.
    pub base_mint: InterfaceAccount<'info, Mint>,

    pub token_program: Program<'info, Token2022>,

    pub club_program: Program<'info, DaddysClub>,
}

#[error_code]
pub enum DemoError {
    /// Недосяжна при `FEE_BPS` ≤ 100%, і все одно названа: `checked_*` мусить
    /// мати чим повернутись, а `unwrap()` у програмі не пишеться.
    #[msg("Arithmetic overflow")]
    MathOverflow,
}
