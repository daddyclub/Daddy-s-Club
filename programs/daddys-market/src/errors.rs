//! Іменовані помилки вторинки.
//!
//! Правило те саме, що й у ядрі: кожен варіант несе вимогу, яку стереже, а
//! варіант без вимоги і без ловця — це сміття, а не резерв. Усі три приїхали
//! сюди з `daddys_club::errors` разом із самою вторинкою. `OfferNotActive`
//! серед них не лишилось: викуплена або скасована оферта не має статусу — її
//! акаунт закривається, оренда повертається продавцеві, і про неї каже сам
//! Anchor (`AccountNotInitialized`), а не ми. Резервувати ім'я під відмову,
//! якої не буде, означало б тримати в переписі сміття (`T035`).
//!
//! Порядок оголошення визначає числові коди. Поки програму не задеплоєно,
//! порядок вільний; після першого деплою варіанти можна лише **дописувати в
//! кінець**, інакше в клієнта і в записаних логах старий код почне означати
//! іншу помилку.

use anchor_lang::prelude::*;

#[error_code]
pub enum MarketError {
    /// `FR-024`: ціну і кількість задає продавець; нульових оферт не буває.
    #[msg("Offer amount and price must both be above zero")]
    OfferTermsInvalid,

    /// `FR-024`: продати можна лише те, що є на руках.
    #[msg("Insufficient bond balance")]
    InsufficientBondBalance,

    /// Межа, на якій `Option` арифметики стає помилкою з іменем. Причина та
    /// сама, що й у ядрі: на ланцюгу видно лише код, і «переповнення» мусить
    /// читатись як переповнення.
    #[msg("Arithmetic overflow")]
    MathOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Перепис усіх варіантів. Він не дублює enum «про всяк випадок» — тест
    /// нижче доводить, що перепис повний, і саме це дає право решті тестів
    /// говорити про «усі помилки».
    const ALL: &[MarketError] = &[
        MarketError::OfferTermsInvalid,
        MarketError::InsufficientBondBalance,
        MarketError::MathOverflow,
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

    /// Коди вторинки — власні, від нуля, і з кодами ядра вони не перетинаються
    /// лише тому, що належать різним програмам. На ланцюгу видно `Custom(6000)`
    /// без імені програми, тому тест, який мовчки припускає спільну нумерацію,
    /// одного дня прочитає `OfferTermsInvalid` як `Unauthorized`.
    #[test]
    fn the_first_code_starts_where_anchor_starts_every_program() {
        assert_eq!(u32::from(MarketError::OfferTermsInvalid), 6000);
    }
}
