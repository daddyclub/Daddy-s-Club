//! Вторинний ринок бондів (`FR-024`…`FR-027`, `FR-035`) — окрема програма.
//!
//! **Чому не в ядрі.** Сховище оферти належить PDA, підписати за нього може
//! лише програма, а мінт бонда має гук — і цей гук є саме ядро. Його CPI в
//! Token-2022 повернувся б у нього ж: `daddys_club → Token-2022 → daddys_club`
//! Solana відхиляє (`ReentrancyNotAllowed`; перевірено пробою на справжньому
//! байткоді 2026-09-23). Звідси й ця програма: у її транзакціях ядро
//! з'являється в стеку рівно один раз — як гук, — і бонд їде зі сховища
//! звичайним `transfer_checked`, лишаючи по собі чекпоінти обох сторін.
//!
//! **Що ринок пише, а що лише читає.** Свої він пише: `Offer` і токен-сховище.
//! В акаунти ядра він не пише нічого — облік рухають гук і `claim`, і ринок їх
//! лише кличе. `Issue` і `ProtocolConfig` читаються, бо ціну ставить продавець
//! (`FR-024`), а торгову комісію — протокол (`FR-035`).

use anchor_lang::prelude::*;

declare_id!("G29gfknNBKtvpfPAjrigefg3tkX7cVXFgnd62NUtcniq");

pub mod errors;
pub mod instructions;
pub mod state;

use instructions::*;

#[program]
pub mod daddys_market {
    use super::*;

    /// Виставляє бонд на продаж: `amount` одиниць номіналу за `price` USDC
    /// цілком (`FR-024`). Токени переїжджають у сховище оферти й лежать там до
    /// купівлі або скасування (`FR-025`).
    pub fn create_offer(
        ctx: Context<CreateOffer>,
        nonce: u64,
        amount: u64,
        price: u64,
    ) -> Result<()> {
        instructions::market::create_offer(ctx, nonce, amount, price)
    }

    /// Викуповує оферту цілком (`FR-026`): продавець отримує USDC за
    /// вирахуванням торгової комісії (`FR-035`), покупець — бонд і відкритий
    /// облік у тій самій транзакції (`FR-038`).
    pub fn buy_offer(ctx: Context<BuyOffer>) -> Result<()> {
        instructions::market::buy_offer(ctx)
    }

    /// Скасовує оферту (`FR-027`): лот повертається продавцеві повністю й без
    /// комісії, разом із тим, що набігло, поки оферта стояла.
    pub fn cancel_offer(ctx: Context<CancelOffer>) -> Result<()> {
        instructions::market::cancel_offer(ctx)
    }
}
