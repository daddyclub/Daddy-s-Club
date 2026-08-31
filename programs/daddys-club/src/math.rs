//! Арифметика погашення. Уся — checked_* у u128, округлення вниз.
//!
//! Дзеркалиться у packages/sdk/src/math.ts і покривається спільними фікстурами
//! з fixtures/. Розбіжність між цими двома файлами має бути червоним тестом.
//!
//! Кожна функція повертає `None` замість того, щоб панікувати: переповнення,
//! ділення на нуль і чекпоінт із майбутнього — це не «неможливо», а помилки,
//! які інструкція мусить назвати іменованою помилкою на своїй межі.

/// Масштаб кумулятивного індексу виплати на одиницю бонду.
pub const SCALE: u128 = 1_000_000_000_000;

pub const BPS_DENOM: u128 = 10_000;

/// Як розійшлося одне надходження: скільки пішло в ескроу погашення і скільки
/// лишилося емітенту.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split {
    pub to_escrow: u128,
    pub to_issuer: u128,
}

/// Повне зобов'язання випуску: номінал + купон (`FR-018`).
///
/// Рахується один раз при створенні випуску і більше не змінюється — купон не
/// залежить від того, як швидко надходить revenue.
pub fn obligation_total(face: u128, coupon_bps: u16) -> Option<u128> {
    let coupon = face
        .checked_mul(u128::from(coupon_bps))?
        .checked_div(BPS_DENOM)?;
    face.checked_add(coupon)
}

/// Узгоджена частка від надходження, до врахування залишку зобов'язання.
pub fn pledged_share(amount: u128, pledge_bps: u16) -> Option<u128> {
    amount
        .checked_mul(u128::from(pledge_bps))?
        .checked_div(BPS_DENOM)
}

/// Розщеплення надходження в момент його виникнення (`FR-014`, `FR-019`,
/// `FR-020`).
///
/// У сховище йде частка, але не більше за залишок зобов'язання: надлишок
/// повертається емітенту в тій самій транзакції. Коли залишок нульовий,
/// перехоплення припиняється саме собою — емітент отримує все.
pub fn split_intercept(amount: u128, pledge_bps: u16, remaining: u128) -> Option<Split> {
    let to_escrow = pledged_share(amount, pledge_bps)?.min(remaining);
    let to_issuer = amount.checked_sub(to_escrow)?;
    Some(Split {
        to_escrow,
        to_issuer,
    })
}

/// Просування кумулятивного індексу виплати на одиницю бонду (`FR-015`).
///
/// Вартість цієї операції не залежить від кількості власників — у цьому й є
/// сенс індексу. Ділення вниз лишає залишок у сховищі: він дістанеться
/// наступним claim'ам, а не зникне.
pub fn advance_index(index: u128, amount: u128, bond_supply: u128) -> Option<u128> {
    if bond_supply == 0 {
        return None;
    }
    let per_unit = amount.checked_mul(SCALE)?.checked_div(bond_supply)?;
    index.checked_add(per_unit)
}

/// Скільки власник може забрати зараз (`FR-016`).
///
/// Різниця індексу з моменту його чекпоінта, помножена на баланс, плюс те, що
/// вже нараховано гуком при передачах.
pub fn claimable(index: u128, checkpoint: u128, balance: u128, accrued: u128) -> Option<u128> {
    let delta = index.checked_sub(checkpoint)?;
    let earned = delta.checked_mul(balance)?.checked_div(SCALE)?;
    earned.checked_add(accrued)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- obligation_total (FR-018) ----

    #[test]
    fn obligation_adds_coupon_to_face() {
        // 250 000 USDC під 9.5% — цифри з картки випуску на M0.
        assert_eq!(obligation_total(250_000_000_000, 950), Some(273_750_000_000));
    }

    #[test]
    fn obligation_without_coupon_is_face() {
        assert_eq!(obligation_total(100, 0), Some(100));
    }

    #[test]
    fn obligation_rounds_the_coupon_down() {
        // 1 × 9.5% = 0.095 → купон 0, зобов'язання лишається номіналом.
        assert_eq!(obligation_total(1, 950), Some(1));
        // 10 001 × 0.01% = 1.0001 → 1, не 2.
        assert_eq!(obligation_total(10_001, 1), Some(10_002));
    }

    #[test]
    fn obligation_overflow_is_none() {
        assert_eq!(obligation_total(u128::MAX, 1), None);
    }

    // ---- pledged_share ----

    #[test]
    fn share_is_bps_of_amount() {
        assert_eq!(pledged_share(1_070_000_000, 1_200), Some(128_400_000));
    }

    #[test]
    fn share_rounds_down() {
        // 7 × 12% = 0.84 → 0. Відкинуте лишається емітенту, а не створюється.
        assert_eq!(pledged_share(7, 1_200), Some(0));
    }

    #[test]
    fn share_overflow_is_none() {
        assert_eq!(pledged_share(u128::MAX, 10_000), None);
    }

    // ---- split_intercept (FR-014, FR-019, FR-020) ----

    #[test]
    fn split_sends_the_share_and_keeps_the_rest() {
        let split = split_intercept(1_000, 1_200, u128::MAX).unwrap();
        assert_eq!(split.to_escrow, 120);
        assert_eq!(split.to_issuer, 880);
    }

    #[test]
    fn split_never_exceeds_the_remaining_obligation() {
        // FR-020: у сховище йде рівно залишок, надлишок — назад емітенту.
        let split = split_intercept(1_000, 5_000, 50).unwrap();
        assert_eq!(split.to_escrow, 50);
        assert_eq!(split.to_issuer, 950);
    }

    #[test]
    fn split_stops_when_nothing_is_owed() {
        // FR-019: погашено — наступні комісії йдуть емітенту повністю.
        let split = split_intercept(1_000, 5_000, 0).unwrap();
        assert_eq!(split.to_escrow, 0);
        assert_eq!(split.to_issuer, 1_000);
    }

    #[test]
    fn split_of_the_exact_remainder_leaves_nothing_over() {
        let split = split_intercept(1_000, 10_000, 1_000).unwrap();
        assert_eq!(split.to_escrow, 1_000);
        assert_eq!(split.to_issuer, 0);
    }

    #[test]
    fn split_always_conserves_the_amount() {
        for amount in [0_u128, 1, 7, 999, 1_000_000] {
            for bps in [0_u16, 1, 1_200, 5_000, 10_000] {
                for remaining in [0_u128, 1, 500, u128::MAX] {
                    let split = split_intercept(amount, bps, remaining).unwrap();
                    assert_eq!(
                        split.to_escrow + split.to_issuer,
                        amount,
                        "amount={amount} bps={bps} remaining={remaining}"
                    );
                    assert!(split.to_escrow <= remaining);
                }
            }
        }
    }

    // ---- advance_index (FR-015) ----

    #[test]
    fn index_advances_by_amount_per_unit() {
        // 120 у сховище на 1 000 одиниць = 0.12 на одиницю.
        assert_eq!(advance_index(0, 120, 1_000), Some(SCALE * 12 / 100));
    }

    #[test]
    fn index_accumulates() {
        let after_first = advance_index(0, 120, 1_000).unwrap();
        let after_second = advance_index(after_first, 120, 1_000).unwrap();
        assert_eq!(after_second, SCALE * 24 / 100);
    }

    #[test]
    fn index_rounds_down_and_leaves_the_dust_in_escrow() {
        // 1 на 3 одиниці: 333333333333, не 333333333333.33 — і не 334.
        assert_eq!(advance_index(0, 1, 3), Some(333_333_333_333));
        // Три власники по одній одиниці заберуть менше, ніж прийшло.
        let index = advance_index(0, 1, 3).unwrap();
        let paid_out: u128 = (0..3).map(|_| claimable(index, 0, 1, 0).unwrap()).sum();
        assert!(paid_out < 1, "виплачено {paid_out}, а прийшла лише 1");
    }

    #[test]
    fn index_without_supply_is_none() {
        assert_eq!(advance_index(0, 120, 0), None);
    }

    #[test]
    fn index_overflow_is_none() {
        assert_eq!(advance_index(0, u128::MAX, 1), None);
    }

    // ---- claimable (FR-016) ----

    #[test]
    fn claim_is_the_index_delta_times_balance() {
        let index = advance_index(0, 120, 1_000).unwrap();
        // Власник 250 одиниць із 1 000 забирає чверть надходження.
        assert_eq!(claimable(index, 0, 250, 0), Some(30));
    }

    #[test]
    fn claim_counts_only_what_came_after_the_checkpoint() {
        let first = advance_index(0, 120, 1_000).unwrap();
        let second = advance_index(first, 120, 1_000).unwrap();
        assert_eq!(claimable(second, first, 250, 0), Some(30));
    }

    #[test]
    fn claim_right_after_a_claim_is_zero() {
        let index = advance_index(0, 120, 1_000).unwrap();
        assert_eq!(claimable(index, index, 250, 0), Some(0));
    }

    #[test]
    fn claim_adds_what_the_hook_already_accrued() {
        let index = advance_index(0, 120, 1_000).unwrap();
        assert_eq!(claimable(index, index, 250, 17), Some(17));
    }

    #[test]
    fn claim_without_balance_is_only_the_accrued() {
        let index = advance_index(0, 120, 1_000).unwrap();
        assert_eq!(claimable(index, 0, 0, 5), Some(5));
    }

    #[test]
    fn claim_rounds_down() {
        // Індекс 0.12 на одиницю, баланс 1 → 0.12 → 0.
        let index = advance_index(0, 120, 1_000).unwrap();
        assert_eq!(claimable(index, 0, 1, 0), Some(0));
    }

    #[test]
    fn claim_from_a_checkpoint_in_the_future_is_none() {
        // Чекпоінт не може випереджати індекс: це зіпсований облік, не нуль.
        assert_eq!(claimable(10, 11, 1, 0), None);
    }

    #[test]
    fn claim_overflow_is_none() {
        assert_eq!(claimable(u128::MAX, 0, u128::MAX, 0), None);
    }

    // ---- інваріант, який тримає SC-003 ----

    #[test]
    fn holders_never_take_more_than_came_in() {
        // Один власник на всю пропозицію не може забрати більше за надходження,
        // скільки б надходжень не було: округлення завжди працює проти нього.
        let supply = 7_u128;
        let mut index = 0_u128;
        let mut received = 0_u128;
        for amount in [1_u128, 2, 3, 5, 8, 13, 21, 34, 55, 89] {
            index = advance_index(index, amount, supply).unwrap();
            received += amount;
        }
        let paid_out: u128 = (0..supply)
            .map(|_| claimable(index, 0, 1, 0).unwrap())
            .sum();
        assert!(
            paid_out <= received,
            "виплачено {paid_out} при надходженнях {received}"
        );
    }
}
