use anchor_lang::prelude::*;

#[error_code]
pub enum ClubError {
    #[msg("Інструкція ще не імплементована")]
    NotImplemented,
}
