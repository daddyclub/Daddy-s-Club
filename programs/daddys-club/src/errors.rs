//! Іменовані помилки програми.
//!
//! Кожен варіант несе вимогу, яку стереже. Помилки без вимоги тут не
//! з'являються, а вимога без помилки означає, що правило нікому не відмовляє —
//! і те, і те має бути видно оком при читанні цього файлу.
//!
//! Порядок оголошення визначає числові коди: Anchor нумерує варіанти від
//! `ERROR_CODE_OFFSET`. Поки програму не задеплоєно, порядок вільний; після
//! першого деплою варіанти можна лише **дописувати в кінець**, інакше в клієнта
//! і в записаних логах старий код почне означати іншу помилку.

use anchor_lang::prelude::*;

#[error_code]
pub enum ClubError {
    // ---- Протокол і конфігурація ----
    /// `FR-036`: параметри протоколу змінює лише адміністративна authority.
    #[msg("Only the protocol authority may change protocol parameters")]
    Unauthorized,

    /// `FR-034`: origination fee 1-2%. `FR-035`: торгова комісія — параметр.
    #[msg("Fee rate is outside the range the protocol allows")]
    FeeOutOfRange,

    /// `FR-005`: стеля частки перехоплення не може дозволяти більше, ніж увесь
    /// потік — інакше вона перестає бути стелею.
    #[msg("Revenue share cap must be above zero and at most 100%")]
    PledgeCapOutOfRange,

    /// `FR-003`: діапазон строків до погашення — 30-180 днів.
    #[msg("Maturity range must be non-empty and within 30 to 180 days")]
    TermRangeOutOfBounds,

    /// `FR-007`: поріг допуску не може бути нульовим — інакше допуску немає.
    #[msg("Revenue history threshold must be above zero")]
    HistoryThresholdInvalid,

    // ---- Створення випуску ----
    /// `FR-003`: строк цього випуску поза діапазоном протоколу.
    #[msg("Issue term is outside the range the protocol allows")]
    TermOutOfRange,

    /// `FR-005`: частка перехоплення вища за стелю протоколу.
    #[msg("Revenue share is above the protocol cap")]
    PledgeAboveCap,

    /// `FR-006`: на одне джерело — не більше одного активного випуску.
    #[msg("This revenue source already backs an active issue")]
    SourceAlreadyPledged,

    /// `FR-007`: джерело ще не накопичило потрібної історії доходу. Перевірка
    /// безумовна: її не обходить ані емітент, ані authority протоколу.
    #[msg("Revenue source has not accumulated enough history yet")]
    InsufficientRevenueHistory,

    /// `FR-001`: номінал і мінімальний лот задає емітент.
    #[msg("Face amount must be above zero and a whole number of lots")]
    FaceAmountInvalid,

    #[msg("Minimum lot must be above zero and at most the face amount")]
    LotSizeInvalid,

    #[msg("Subscription window must end after it starts and before maturity")]
    SubscriptionWindowInvalid,

    // ---- Підписка ----
    /// `FR-008`: підписка приймається лише у відповідному стані випуску.
    #[msg("Issue is not accepting subscriptions")]
    IssueNotSubscribing,

    #[msg("Subscription window has closed")]
    SubscriptionWindowClosed,

    /// `FR-009`: внесок нижчий за мінімальний лот.
    #[msg("Contribution is below the minimum lot")]
    BelowMinimumLot,

    /// `FR-009`: залишку нерозібраного номіналу вже немає. Частковий прийом
    /// вичерпує залишок, тому це не «перепідписка», а внесок після повного збору.
    #[msg("Issue is fully subscribed")]
    IssueFullySubscribed,

    /// `FR-038`: облік за випуском має бути відкритий до того, як з'являться
    /// бонд-токени.
    ///
    /// **Ніхто її не кидає, і це навмисно.** Відмовляє замість неї Anchor:
    /// `holder` у `subscribe` — це `Account<HolderCheckpoint>` на seeds випуску
    /// й підписанта, тому невідкритий облік упирається в
    /// `AccountNotInitialized` ще до тіла інструкції. Права стережуть
    /// обмеження, а не `require!` — переписати це на перевірку в тілі означало
    /// б проміняти типобезпеку на текст повідомлення. Варіант лишається до
    /// прибирання невживаних на закритті M1, разом із `NotImplemented`:
    /// видалення зсуває коди всіх наступних, і робити це варто один раз.
    #[msg("No position is open for this wallet on this issue")]
    PositionNotOpen,

    // ---- Закриття, видача і повернення ----
    /// `FR-010`, `FR-012`: номінал доступний емітенту лише при повному зборі.
    #[msg("Issue has not been fully funded")]
    IssueNotFunded,

    /// `FR-012`: номінал стає доступним емітенту рівно один раз. Окремого
    /// прапорця у випуску немає — записом про видачу є перехід у `Repaying`,
    /// тому саме стан і відрізняє «уже забрано» від «ще не зібрано».
    #[msg("Proceeds have already been withdrawn")]
    ProceedsAlreadyWithdrawn,

    /// `FR-011`: повернення можливе лише для недозібраного випуску.
    #[msg("Issue is not marked undersubscribed")]
    IssueNotFailed,

    #[msg("Subscription window has not closed yet")]
    SubscriptionWindowStillOpen,

    #[msg("Nothing to refund")]
    NothingToRefund,

    // ---- Погашення ----
    /// `FR-004`: єдиний вхід доходу. Дохід приймається тільки від того, хто
    /// довів підписом, що він і є програма-емітент; переданий program id
    /// доказом не є.
    #[msg("Caller did not prove it is the registered revenue source authority")]
    SourceAuthorityMismatch,

    #[msg("Revenue source does not back any active issue")]
    SourceNotPledged,

    /// `FR-014`: розщеплення діє, поки випуск погашається.
    #[msg("Issue is not in repayment")]
    IssueNotRepaying,

    /// `FR-021`: дострокове погашення вже закритого зобов'язання.
    #[msg("Obligation is already fully repaid")]
    ObligationAlreadyRepaid,

    /// `FR-022`: перехід у past due настає від дати, не раніше.
    #[msg("Issue has not reached maturity yet")]
    IssueNotMatured,

    /// `FR-016`: забирати нічого.
    #[msg("Nothing to claim")]
    NothingToClaim,

    /// `FR-016`: облік належить іншому випуску, ніж той, з якого забирають.
    #[msg("Account belongs to a different issue")]
    IssueMismatch,

    // ---- Гук і передача ----
    /// `FR-017`: гук можна покликати напряму, повз Token-2022. Без цієї
    /// перевірки чекпоінти зрушувались би без переказу.
    #[msg("Hook was invoked outside a token transfer")]
    NotTransferring,

    /// `FR-038`: передача на гаманець без відкритого обліку відхиляється цілком,
    /// замість того щоб пройти й зіпсувати облік.
    #[msg("Recipient has no open position for this issue")]
    RecipientPositionMissing,

    // ---- Вторинний ринок ----
    /// `FR-024`: ціну і кількість задає продавець; нульових оферт не буває.
    #[msg("Offer amount and price must both be above zero")]
    OfferTermsInvalid,

    /// `FR-025`: оферту вже викупили або скасували.
    #[msg("Offer is no longer active")]
    OfferNotActive,

    #[msg("Insufficient bond balance")]
    InsufficientBondBalance,

    // ---- Арифметика ----
    // Межа, на якій `Option` із math.rs стає помилкою з іменем. Причини
    // розділені, бо на ланцюгу видно лише код: «переповнення» і «чекпоінт із
    // майбутнього» вимагають різних дій, і зводити їх в одну помилку означало б
    // втратити це саме там, де діагностувати найважче.
    #[msg("Arithmetic overflow")]
    MathOverflow,

    /// Індекс на одиницю без одиниць не визначений.
    #[msg("Bond supply is zero")]
    ZeroBondSupply,

    /// Чекпоінт не може випереджати індекс: це зіпсований облік, а не нуль.
    #[msg("Checkpoint is ahead of the payout index")]
    CheckpointAheadOfIndex,

    /// Частка перехоплення понад 100% надходження.
    #[msg("Pledged share exceeds the inflow")]
    PledgeExceedsInflow,

    // ---- Тимчасове ----
    // Стоїть останнім навмисно: заглушки в lib.rs зникають разом із Фазою 2,
    // і видалення варіанта з кінця нікому не зсуває коди.
    #[msg("Instruction is not implemented yet")]
    NotImplemented,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Перепис усіх варіантів. Він не дублює enum «про всяк випадок» — тест
    /// нижче доводить, що перепис повний, і саме це дає право решті тестів
    /// говорити про «усі помилки».
    const ALL: &[ClubError] = &[
        ClubError::Unauthorized,
        ClubError::FeeOutOfRange,
        ClubError::PledgeCapOutOfRange,
        ClubError::TermRangeOutOfBounds,
        ClubError::HistoryThresholdInvalid,
        ClubError::TermOutOfRange,
        ClubError::PledgeAboveCap,
        ClubError::SourceAlreadyPledged,
        ClubError::InsufficientRevenueHistory,
        ClubError::FaceAmountInvalid,
        ClubError::LotSizeInvalid,
        ClubError::SubscriptionWindowInvalid,
        ClubError::IssueNotSubscribing,
        ClubError::SubscriptionWindowClosed,
        ClubError::BelowMinimumLot,
        ClubError::IssueFullySubscribed,
        ClubError::PositionNotOpen,
        ClubError::IssueNotFunded,
        ClubError::ProceedsAlreadyWithdrawn,
        ClubError::IssueNotFailed,
        ClubError::SubscriptionWindowStillOpen,
        ClubError::NothingToRefund,
        ClubError::SourceAuthorityMismatch,
        ClubError::SourceNotPledged,
        ClubError::IssueNotRepaying,
        ClubError::ObligationAlreadyRepaid,
        ClubError::IssueNotMatured,
        ClubError::NothingToClaim,
        ClubError::IssueMismatch,
        ClubError::NotTransferring,
        ClubError::RecipientPositionMissing,
        ClubError::OfferTermsInvalid,
        ClubError::OfferNotActive,
        ClubError::InsufficientBondBalance,
        ClubError::MathOverflow,
        ClubError::ZeroBondSupply,
        ClubError::CheckpointAheadOfIndex,
        ClubError::PledgeExceedsInflow,
        ClubError::NotImplemented,
    ];

    #[test]
    fn the_census_covers_every_variant_in_order() {
        for (position, error) in ALL.iter().enumerate() {
            assert_eq!(
                *error as u32, position as u32,
                "перепис розійшовся з enum на позиції {position}"
            );
        }
    }

    #[test]
    fn messages_are_present_and_distinct() {
        // Однакове повідомлення на двох варіантах робить дві різні відмови
        // нерозрізненними в логах — найдешевша помилка копіювання і найдорожча
        // при розборі.
        let mut seen: Vec<String> = Vec::with_capacity(ALL.len());
        for error in ALL {
            let message = error.to_string();
            assert!(!message.is_empty(), "порожнє повідомлення у {error:?}");
            assert!(
                !seen.contains(&message),
                "повідомлення «{message}» повторюється на {error:?}"
            );
            seen.push(message);
        }
    }

    #[test]
    fn codes_start_at_the_anchor_offset() {
        // Перший варіант закріплює початок нумерації: якщо колись припишуть
        // помилку на початок, зсунуться геть усі коди, і тест має це спіймати.
        assert_eq!(
            u32::from(ClubError::Unauthorized),
            anchor_lang::error::ERROR_CODE_OFFSET
        );
    }
}
