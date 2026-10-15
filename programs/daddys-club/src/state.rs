//! Акаунти протоколу. Схема — docs/PLAN.md → «Модель даних».
//!
//! Канонічний стан живе тут, а не в базі: будь-яка друга копія правди там, де
//! йдеться про гроші, рано чи пізно розійдеться з ланцюгом.
//!
//! Розміри рахує `InitSpace`; на кожен акаунт зверху йде 8 байт дискримінатора
//! Anchor, тому в `init` виділяється `8 + T::INIT_SPACE`.
//!
//! Кожен PDA зберігає свій `bump`, а кожен акаунт із лічильником у seeds —
//! ще й лічильник. Обґрунтування обох — docs/PLAN.md → «Модель даних».

use anchor_lang::prelude::*;

pub const CONFIG_SEED: &[u8] = b"config";
pub const SOURCE_SEED: &[u8] = b"source";
pub const ISSUE_SEED: &[u8] = b"issue";
pub const HOLDER_SEED: &[u8] = b"holder";

/// Життєвий цикл випуску.
///
/// Порядок варіантів — це байт у стані на ланцюгу. Переставити їх означає
/// мовчки перечитати вже записані випуски: `Repaid` став би `Failed`. Тому
/// варіанти лише дописуються в кінець.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug, InitSpace)]
pub enum IssueState {
    Subscribing,
    Funded,
    Repaying,
    PastDue,
    Repaid,
    Failed,
}

/// Параметри протоколу. Singleton, seeds `["config"]` (`FR-036`).
#[account]
#[derive(InitSpace)]
pub struct ProtocolConfig {
    pub admin: Pubkey,
    /// `FR-034`: 100…200 = 1…2%.
    pub origination_fee_bps: u16,
    /// `FR-035`.
    pub trading_fee_bps: u16,
    /// `FR-005`. Вона ж ставка перехоплення після переходу в past due
    /// (`FR-022`), тому одне поле, а не два.
    pub max_pledge_bps: u16,
    /// `FR-003`: 30…180 днів.
    pub min_tenor_secs: i64,
    pub max_tenor_secs: i64,
    /// `FR-007`: скільки історії доходу джерело мусить накопичити до випуску.
    pub history_threshold_secs: i64,
    pub usdc_mint: Pubkey,
    pub fee_vault: Pubkey,
    pub bump: u8,
}

/// Джерело revenue. Seeds `["source", issuer, seq]`.
#[account]
#[derive(InitSpace)]
pub struct RevenueSource {
    pub issuer: Pubkey,
    /// PDA програми-емітента — єдиний, від кого приймається дохід (`FR-004`).
    /// Саме підпис цього PDA автентифікує `intercept`; переданий program id
    /// доказом не є.
    pub authority: Pubkey,
    pub vault: Pubkey,
    /// `FR-007`, `FR-028`: історія починається з підключення перехоплення, а не
    /// зі створення випуску.
    pub first_seen_ts: i64,
    pub total_observed: u64,
    /// Зріз `total_observed` на момент випуску — база для порівняння темпів
    /// (`FR-030`).
    pub observed_before_issue: u64,
    /// `FR-006`: не більше одного активного випуску на джерело.
    pub active_issue: Option<Pubkey>,
    pub seq: u64,
    pub bump: u8,
}

/// Випуск. Seeds `["issue", source, seq]`.
#[account]
#[derive(InitSpace)]
pub struct Issue {
    pub source: Pubkey,
    pub bond_mint: Pubkey,
    pub escrow_vault: Pubkey,
    pub subscription_vault: Pubkey,
    pub face: u64,
    pub coupon_bps: u16,
    pub pledge_bps: u16,
    pub maturity_ts: i64,
    pub subscription_end_ts: i64,
    pub min_lot: u64,
    pub raised: u64,
    /// Номінал + купон, зафіксовані при створенні (`FR-018`).
    pub obligation_total: u64,
    pub repaid_total: u64,
    /// Кумулятивна виплата на одиницю бонду в масштабі `math::SCALE`
    /// (`FR-015`). `u128`, бо масштаб 1e12 з'їдає u64 на перших же сумах.
    pub payout_index: u128,
    pub state: IssueState,
    pub seq: u64,
    pub bump: u8,
    /// The share intercepted once the issue is past due (`FR-022`): the
    /// protocol cap as it stood at creation. Frozen with the terms, because a
    /// later change of the cap must not reach an existing issue (`FR-036`).
    /// Last in the layout so the offsets clients already read do not move.
    pub past_due_pledge_bps: u16,
}

impl Issue {
    /// Whether the issue has let go of its source (`FR-006`): "at most one
    /// **active** issue" means a finished one holds no place.
    ///
    /// Repaid and undersubscribed do — but undersubscribed is not only what
    /// has been written down. `Failed` is written by the first `refund`, and an
    /// issue nobody subscribed to never gets one: there is no one to refund. So
    /// a closed window with an incomplete raise counts the same as a recorded
    /// `Failed` — by the very rule `refund` opens on.
    ///
    /// `Funded` holds investors' money until the payout and `PastDue` can only
    /// be repaid (`FR-022`) — neither lets go. The `match` is exhaustive on
    /// purpose: a new state must not quietly become "free".
    pub fn releases_source(&self, now: i64) -> bool {
        match self.state {
            IssueState::Repaid | IssueState::Failed => true,
            IssueState::Subscribing => now >= self.subscription_end_ts && self.raised < self.face,
            IssueState::Funded | IssueState::Repaying | IssueState::PastDue => false,
        }
    }

    /// `FR-022`: an obligation still open on its maturity date is past due.
    /// On the date itself, not a second later: by then it had to be closed.
    /// Only `Repaying` falls due — before the payout there is no obligation,
    /// and nothing leads back, so past due ends only in `Repaid`.
    pub fn fall_due(&mut self, now: i64) {
        if self.state == IssueState::Repaying && now >= self.maturity_ts {
            self.state = IssueState::PastDue;
        }
    }

    /// The share the intercept takes from a fee, or `None` when this state
    /// does not split the flow.
    pub fn intercepted_bps(&self) -> Option<u16> {
        match self.state {
            IssueState::Repaying => Some(self.pledge_bps),
            IssueState::PastDue => Some(self.past_due_pledge_bps),
            IssueState::Subscribing
            | IssueState::Funded
            | IssueState::Repaid
            | IssueState::Failed => None,
        }
    }
}

/// Облік власника за випуском. Seeds `["holder", issue, owner]`.
///
/// Існування цього акаунта — і є «відкритий облік» із `FR-038`: передача
/// бонд-токена на гаманець без нього відхиляється цілком.
#[account]
#[derive(InitSpace)]
pub struct HolderCheckpoint {
    pub issue: Pubkey,
    pub owner: Pubkey,
    /// Звідки рахувати наступну претензію (`FR-016`).
    pub index_at_checkpoint: u128,
    /// Нараховане гуком при передачах, ще не забране (`FR-017`).
    pub accrued: u64,
    pub claimed_total: u64,
    pub bump: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_state_bytes_are_pinned() {
        // Це байт у записаному стані, а не деталь реалізації: якщо варіанти
        // переставити, вже створені випуски мовчки поміняють стан.
        for (expected_byte, state) in [
            IssueState::Subscribing,
            IssueState::Funded,
            IssueState::Repaying,
            IssueState::PastDue,
            IssueState::Repaid,
            IssueState::Failed,
        ]
        .into_iter()
        .enumerate()
        {
            let encoded = state.try_to_vec().expect("стан не серіалізувався");
            assert_eq!(
                encoded,
                vec![expected_byte as u8],
                "{state:?} записується не тим байтом"
            );
        }
    }

    #[test]
    fn a_pledged_source_fills_the_space_reserved_for_it() {
        // Borsh пише `None` одним байтом, а `Some` — тридцятьма трьома, і
        // виділяється завжди більше. Тобто зайняте джерело має вкластися рівно
        // в резерв: якщо не вкладається, `active_issue` не запишеться після
        // того, як випуск з'явиться.
        let pledged = RevenueSource {
            issuer: Pubkey::default(),
            authority: Pubkey::default(),
            vault: Pubkey::default(),
            first_seen_ts: 0,
            total_observed: 0,
            observed_before_issue: 0,
            active_issue: Some(Pubkey::default()),
            seq: 0,
            bump: 0,
        };
        let free = RevenueSource {
            active_issue: None,
            ..pledged
        };

        let pledged_len = pledged.try_to_vec().expect("не серіалізувалось").len();
        let free_len = free.try_to_vec().expect("не серіалізувалось").len();

        assert_eq!(pledged_len, RevenueSource::INIT_SPACE);
        assert_eq!(free_len, RevenueSource::INIT_SPACE - 32);
    }

    #[test]
    fn account_sizes_are_pinned() {
        // PLAN рахує розміри під rent-exempt. Поле, додане мимохідь, має бути
        // видно як зміну цифри, а не тільки як зрослу оренду.
        assert_eq!(ProtocolConfig::INIT_SPACE, 127);
        assert_eq!(RevenueSource::INIT_SPACE, 162);
        assert_eq!(Issue::INIT_SPACE, 216);
        assert_eq!(HolderCheckpoint::INIT_SPACE, 97);
    }
}
