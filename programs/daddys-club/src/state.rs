//! Акаунти протоколу. Схема — docs/PLAN.md → «Модель даних».

use anchor_lang::prelude::*;

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum IssueState {
    Subscribing,
    Funded,
    Repaying,
    PastDue,
    Repaid,
    Failed,
}
