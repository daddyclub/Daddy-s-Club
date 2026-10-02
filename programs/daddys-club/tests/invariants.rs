//! `SC-003` and `SC-004`: does the money still add up after an arbitrary
//! sequence of operations, now that bonds change hands?
//!
//! Both criteria are measured **in one run over one corpus**, because that is
//! how `SPEC.md` writes them: `SC-004` opens with "on the same set". The set is
//! a thousand sequences, each one the whole life of one issue: one to four
//! subscribers with unequal lots, two outside wallets that start with cash and
//! no bonds, and then a random stream of the four operations `SC-003` names —
//! fee arrivals, claims, **transfers** and **sales** — plus early repayment.
//!
//! **What is proven:**
//! - `SC-003`: everything paid out to owners plus what is left in the escrow
//!   equals everything intercepted, to the smallest USDC unit and **after every
//!   step**, not only at the end of a sequence;
//! - `SC-004`: the escrow never takes in more than the obligation still owes,
//!   and no owner ever takes out more than their share — where the share is
//!   now a quantity **in time**: every arrival is divided by the bonds each
//!   wallet held at that moment;
//! - and the other side of the same share, which is what M2 promises ("the
//!   accrued payout is correct for both sides"): once everything is swept, every
//!   owner has received their share **less rounding only**, so nothing that
//!   accrued before a sale went with the tokens to the buyer.
//!
//! **How one sequence is built.** Issue → one to four subscribers, each with a
//! ledger of their own and an unequal lot (the last one offers more than is
//! left, so the corpus also holds a partial fill, `FR-009`) → proceeds are
//! withdrawn and the issue is `Repaying` → 6…18 random operations → **close**:
//! every offer still standing is cancelled → **sweep**: every wallet with a
//! ledger claims → and once more, which must pay nobody anything.
//!
//! The random operations:
//! - a **transfer** is a plain Token-2022 `transfer_checked` between two
//!   wallets, the way a wallet app would send it, past our own client. Its size
//!   is a fraction of what the sender holds, so whole balances, a single unit
//!   and zero all occur. Sending to a wallet that has no ledger must be refused
//!   as a whole (`FR-038`) and leave everything as it was;
//! - a **sale** is three operations, not one: `create_offer` puts part of a
//!   balance into an offer escrow, and the offer then **stands** while
//!   arrivals, claims and transfers go on around it, until `buy_offer` or
//!   `cancel_offer` takes it down. That is the riskiest path of M2: what accrues
//!   on the escrow's ledger while the offer stands is claimed through a CPI and
//!   forwarded to the seller. Buyers are subscribers and outside wallets alike;
//!   an outside wallet's ledger is opened by the purchase itself.
//!
//! The generator does not read the state of the issue, on purpose: an
//! operation arriving at the wrong moment is exactly what the protocol has to
//! survive. Sizes are drawn as fractions and resolved against the balances of
//! the moment; a purchase or cancellation with no offer to act on is recorded
//! as idle rather than sent.
//!
//! **Why the equality is not a tautology of the token program.** Its three
//! numbers are measured from three different sides and none is taken from the
//! issue's state: what was intercepted — how much the **issuer's** accounts
//! shrank; what was paid out — how much the **owners'** accounts (and the
//! protocol treasury) grew; what is left — the escrow balance. The owners' side
//! is measured across every wallet at once, so a purchase, where the buyer's
//! cash goes to the seller and the treasury, contributes exactly the accrual
//! forwarded from the escrow and nothing of the price. The price is a third
//! stream of USDC and it closes on its own, in a separate lock. What the
//! program wrote into its own books (`repaid_total`, `claimed_total`) is
//! compared with the money in another one: books that disagree with the money
//! are the very discrepancy `SC-003` counts.
//!
//! **What the corpus does not prove.** The run is in mollusk: no queues, no
//! contention for state, no transaction size limit — the same demo boundary as
//! `SPEC.md` → Assumptions. Arrivals call `intercept` directly with the pool's
//! signature taken from metadata; that the CPI path from a swap reaches it
//! whole is `tests/swap.rs`. Transfers by a delegate and between two accounts
//! of one wallet are not in the corpus (the latter is `tests/hook.rs`). And
//! the corpus is deterministic: a thousand **different** sequences, not a
//! thousand random numbers that change from run to run. The price is known: the
//! set catches what is in it, and no property "for all inputs" is proven here.

#[path = "harness.rs"]
mod harness;

use {
    anchor_lang::{AccountDeserialize, InstructionData},
    anchor_spl::token_2022::spl_token_2022::{
        extension::StateWithExtensions,
        instruction::TokenInstruction,
        state::{Account as TokenState, Mint as MintState},
    },
    daddys_club::{
        errors::ClubError,
        instructions::issue::IssueParams,
        math::SCALE,
        state::{HolderCheckpoint, Issue, IssueState},
    },
    daddys_market::errors::MarketError,
    harness::*,
    mollusk_svm::{
        account_store::AccountStore,
        result::{InstructionResult, ProgramResult},
        MolluskContext,
    },
    solana_account::Account,
    solana_address::Address as Pubkey,
    solana_instruction::{AccountMeta, Instruction},
    solana_program_error::ProgramError,
    std::{
        collections::{HashMap, HashSet},
        sync::OnceLock,
    },
};

/// Corpus size. The number from `SC-003` ("on a set of ≥1000 random
/// sequences"), not a round "just because".
const SEQUENCES: usize = 1_000;

/// How many subscribers an issue can have. The ceiling is about the time it
/// takes to build the corpus, not realism: what divides between four unequal
/// lots divides between any; what a hundred holders cost is measured in
/// `tests/compute.rs`.
const MAX_HOLDERS: usize = 4;

/// Wallets that hold no bonds at issuance. They enter only by buying on the
/// secondary market, which is also what opens their ledger (`FR-038`); until
/// then every transfer to them must be refused.
const NEWCOMERS: usize = 2;

/// Length of the random part of a sequence. Longer than in `T029`: an offer
/// has to stand for a few steps for anything to accrue on it.
const MIN_OPS: u64 = 6;
const MAX_OPS: u64 = 18;

/// Corpus seed. Fixed: a set that changes from run to run fails one day on its
/// own and takes the failing sequence with it.
const SEED: u64 = 0x0DDD_C1CB_2026_0928;

const ISSUER_USDC: Pubkey = Pubkey::new_from_array([61u8; 32]);

/// What sits on the source account before the first arrival, and what the
/// issuer has for early repayment. Interception cannot take more than the
/// obligation of the largest issue in the corpus (4 000 000 USDC plus coupon),
/// so both accounts are funded four times over.
///
/// Issuance proceeds also land on the issuer's account, but they cannot be
/// relied on: in a sequence that is repaid on its very first step, the
/// remaining obligation exceeds the proceeds by the coupon and the origination
/// fee.
const VAULT_FUNDS: u64 = 20_000_000_000_000;
const PREPAY_FUNDS: u64 = 20_000_000_000_000;

/// Cash every wallet has for the secondary market, as a multiple of face. A
/// price is at most one and a half times the lot and a lot at most the whole
/// face, and a sequence has at most eighteen purchases — so a buyer never runs
/// out of money, and a purchase is never refused by the token program for a
/// reason that is not the protocol's.
const CASH_PER_FACE: u64 = 30;

const MINT_SUPPLY: u64 = 1_000_000_000_000_000;

const BPS: u64 = 10_000;

type Store = HashMap<Pubkey, Account>;

// ---- Generator -------------------------------------------------------------

/// SplitMix64. Our own rather than from a crate: `proptest` brings a tree of
/// dependencies for a ten-line generator, and the `spl-list-view` pin in
/// `CLAUDE.md` wants a reason for every new branch of the graph. The only
/// reason here is reproducibility, and the seed gives it.
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..bound`. Modulo bias does not matter here: the corpus
    /// has to be varied, not uniform.
    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    /// A number in `low..=high`.
    fn between(&mut self, low: u64, high: u64) -> u64 {
        low + self.below(high - low + 1)
    }
}

/// Offer price. A lot has no price of its own — the seller sets one — so the
/// corpus draws it either around par or as a sum so small that the trading fee
/// rounds to zero and `buy_offer` skips the transfer to the treasury.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Price {
    /// A fixed sum below the smallest price that carries a fee.
    Dust(u64),
    /// A fraction of the lot, in basis points.
    Relative(u64),
}

/// An operation of the random part of a sequence. Wallets are numbered:
/// subscribers first, then the newcomers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Op {
    /// A fee arrival: the issuer's swap, from which the pledged share is split.
    Arrival(u64),
    /// A payout to a wallet.
    Claim(usize),
    /// Early repayment of the remainder (`FR-021`).
    Prepay,
    /// `part` basis points of what `from` holds, straight through Token-2022.
    Transfer { from: usize, to: usize, part: u64 },
    /// `part` basis points of what `seller` holds, into a new offer.
    List {
        seller: usize,
        part: u64,
        price: Price,
    },
    /// A purchase of one of the standing offers not placed by `buyer`.
    Buy { buyer: usize, pick: u64 },
    /// A cancellation of one of the standing offers, by its seller.
    Cancel { pick: u64 },
}

/// A fee arrival. Four sizes, each catching its own case: a penny rounds to
/// zero (12% of 8 is 0, and nothing goes into the escrow), the two middle ones
/// are credited whole, the large one hits the ceiling of the remainder
/// (`FR-020`). Three of the four are measured from face, or else on an issue
/// of tens of USDC every first arrival would repay it whole, and on an issue of
/// millions none would move anything.
fn inflow(rng: &mut Rng, face: u64) -> u64 {
    match rng.below(10) {
        0..=1 => rng.between(1, 8),
        2..=4 => rng.between(1, (face / 1_000).max(1)),
        5..=7 => rng.between(1, (face / 10).max(1)),
        _ => rng.between(1, face.saturating_mul(10)),
    }
}

/// A share of a balance. The whole balance and a single basis point are drawn
/// on purpose: the first empties a wallet — after which it can only claim what
/// accrued before — and the second rounds to zero on any small balance.
fn part(rng: &mut Rng) -> u64 {
    match rng.below(10) {
        0..=1 => BPS,
        2 => 1,
        _ => rng.between(1, BPS - 1),
    }
}

/// The highest price whose trading fee rounds to zero.
fn feeless_ceiling() -> u64 {
    BPS / u64::from(stored_config().trading_fee_bps) - 1
}

fn price(rng: &mut Rng) -> Price {
    match rng.below(10) {
        0 => Price::Dust(rng.between(1, feeless_ceiling())),
        _ => Price::Relative(rng.between(5_000, 15_000)),
    }
}

/// The random part of a sequence.
fn script(rng: &mut Rng, wallets: usize, face: u64) -> Vec<Op> {
    let count = rng.between(MIN_OPS, MAX_OPS);
    let wallet = |rng: &mut Rng| rng.below(wallets as u64) as usize;

    (0..count)
        .map(|_| match rng.below(100) {
            0..=34 => Op::Arrival(inflow(rng, face)),
            35..=49 => Op::Claim(wallet(rng)),
            50..=54 => Op::Prepay,
            55..=69 => {
                let from = wallet(rng);
                let to = (from + 1 + rng.below(wallets as u64 - 1) as usize) % wallets;
                Op::Transfer {
                    from,
                    to,
                    part: part(rng),
                }
            }
            70..=81 => Op::List {
                seller: wallet(rng),
                part: part(rng),
                price: price(rng),
            },
            82..=91 => Op::Buy {
                buyer: wallet(rng),
                pick: rng.next(),
            },
            _ => Op::Cancel { pick: rng.next() },
        })
        .collect()
}

/// Split of face into lots. No equal shares on purpose: dividing the index
/// over unequal lots is what leaves a remainder, and `SC-003` has to hold
/// together with it, not in spite of it.
fn deal(rng: &mut Rng, holders: usize, face: u64) -> Vec<u64> {
    let min_lot = min_lot();
    let mut lots = Vec::with_capacity(holders);
    let mut left = face;

    for index in 0..holders {
        let rest = (holders - index - 1) as u64;
        if rest == 0 {
            lots.push(left);
            break;
        }
        // The ceiling leaves at least one lot to each remaining holder: a
        // subscription below the minimum lot is refused (`FR-008`).
        let lot = rng.between(min_lot, left - rest * min_lot);
        lots.push(lot);
        left -= lot;
    }

    lots
}

fn part_of(balance: u64, part: u64) -> u64 {
    (u128::from(balance) * u128::from(part) / u128::from(BPS)) as u64
}

fn price_of(lot: u64, price: Price) -> u64 {
    match price {
        Price::Dust(sum) => sum,
        Price::Relative(bps) => part_of(lot, bps).max(1),
    }
}

/// The fee at the **protocol's** rate rather than a literal: otherwise the lock
/// would stay green after a change of rate in the config and stop talking
/// about it.
fn fee_of(price: u64) -> u64 {
    (u128::from(price) * u128::from(stored_config().trading_fee_bps) / u128::from(BPS)) as u64
}

// ---- Wallets ---------------------------------------------------------------
//
// The same three roles as in `tests/compute.rs`, derived the same way: the
// addresses could be written out as constants, but then they would have to be
// rewritten together with `MAX_HOLDERS`.

fn keyed(tag: u8, index: usize) -> Pubkey {
    let mut bytes = [0x77u8; 32];
    bytes[0] = tag;
    bytes[1..3].copy_from_slice(&(index as u16).to_le_bytes());

    Pubkey::new_from_array(bytes)
}

fn owner_key(index: usize) -> Pubkey {
    keyed(0xA1, index)
}

fn usdc_key(index: usize) -> Pubkey {
    keyed(0xA2, index)
}

fn bond_key(index: usize) -> Pubkey {
    keyed(0xA3, index)
}

fn ledger_key(owner: Pubkey) -> Pubkey {
    holder_pda(demo_issue(), owner).0
}

// ---- Instructions ----------------------------------------------------------
//
// The metas are written by hand, as in `tests/invest.rs` and
// `tests/market.rs`, rather than built from the structs Anchor generates, as
// in `tests/compute.rs`. The difference has a reason: there the account set
// was the subject of the measurement and pinning it to the program was the
// point. Here the subject is money, and no lock in this file stands on the
// shape of an account set.

/// Issue terms. Face comes from outside because it is the only thing that
/// changes from sequence to sequence; the rest is the M0 card.
fn terms(face: u64) -> IssueParams {
    let issue = stored_issue(IssueState::Subscribing, 0);

    IssueParams {
        face,
        coupon_bps: issue.coupon_bps,
        pledge_bps: issue.pledge_bps,
        maturity_ts: issue.maturity_ts,
        subscription_end_ts: issue.subscription_end_ts,
        min_lot: issue.min_lot,
    }
}

/// Neither the minimum lot nor the pledged share depends on face: both come
/// from the same M0 card.
fn min_lot() -> u64 {
    stored_issue(IssueState::Subscribing, 0).min_lot
}

fn pledge_bps() -> u16 {
    stored_issue(IssueState::Subscribing, 0).pledge_bps
}

/// Face of an issue — from tens of USDC to millions, five orders of magnitude.
/// Not decoration: face equals the bond supply, that is, the denominator of
/// the payout index, and it decides at which scale rounding shows. A thousand
/// sequences with one face would be a thousand measurements of one scale.
/// Built from lots, because `FR-001` wants face to split into whole lots.
fn face(rng: &mut Rng) -> u64 {
    let lots = match rng.below(10) {
        0..=2 => rng.between(8, 50),
        3..=6 => rng.between(1_000, 500_000),
        _ => rng.between(1_000_000, 4_000_000),
    };

    lots * min_lot()
}

fn create_issue_ix(face: u64) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::CreateIssue {
            seq: ISSUE_SEQ,
            params: terms(face),
        }
        .data(),
        vec![
            AccountMeta::new_readonly(config_pda().0, false),
            AccountMeta::new(demo_source(), false),
            AccountMeta::new(demo_issue(), false),
            AccountMeta::new(ISSUER, true),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new(BOND_MINT, true),
            AccountMeta::new(SUBSCRIPTION_VAULT, true),
            AccountMeta::new(ESCROW_VAULT, true),
            AccountMeta::new(extra_metas_pda(BOND_MINT).0, false),
            AccountMeta::new_readonly(token_program().0, false),
            AccountMeta::new_readonly(system_program().0, false),
            // No previous issue: the source is free.
            AccountMeta::new_readonly(club_id(), false),
        ],
    )
}

fn open_ix(index: usize) -> Instruction {
    let owner = owner_key(index);

    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::OpenPosition {}.data(),
        vec![
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(ledger_key(owner), false),
            AccountMeta::new(ISSUER, true),
            AccountMeta::new_readonly(owner, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    )
}

fn subscribe_ix(index: usize, amount: u64) -> Instruction {
    let owner = owner_key(index);

    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Subscribe { amount }.data(),
        vec![
            AccountMeta::new(demo_issue(), false),
            AccountMeta::new_readonly(ledger_key(owner), false),
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new(usdc_key(index), false),
            AccountMeta::new(SUBSCRIPTION_VAULT, false),
            AccountMeta::new(BOND_MINT, false),
            AccountMeta::new(bond_key(index), false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

fn withdraw_ix() -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::WithdrawProceeds {}.data(),
        vec![
            AccountMeta::new_readonly(config_pda().0, false),
            AccountMeta::new(demo_issue(), false),
            AccountMeta::new_readonly(demo_source(), false),
            AccountMeta::new_readonly(ISSUER, true),
            AccountMeta::new(ISSUER_USDC, false),
            AccountMeta::new(SUBSCRIPTION_VAULT, false),
            AccountMeta::new(FEE_VAULT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

/// Interception called directly: mollusk takes the pool PDA's signature from
/// metadata. The path is narrower than the real one — there the issuer program
/// signs from inside a CPI — but the money moves through the same code, and
/// money is what this file is about. That the CPI path reaches it whole is
/// shown by `tests/swap.rs` and `T024`.
fn arrival_ix(amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Intercept { amount }.data(),
        vec![
            AccountMeta::new(demo_source(), false),
            AccountMeta::new_readonly(issuer_authority().0, true),
            AccountMeta::new(SOURCE_VAULT, false),
            AccountMeta::new(demo_issue(), false),
            AccountMeta::new(ESCROW_VAULT, false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

fn claim_ix(index: usize) -> Instruction {
    let owner = owner_key(index);

    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Claim {}.data(),
        vec![
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(ledger_key(owner), false),
            AccountMeta::new_readonly(owner, true),
            AccountMeta::new(usdc_key(index), false),
            AccountMeta::new(ESCROW_VAULT, false),
            AccountMeta::new_readonly(bond_key(index), false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

fn prepay_ix() -> Instruction {
    Instruction::new_with_bytes(
        club_id(),
        &daddys_club::instruction::Prepay {}.data(),
        vec![
            AccountMeta::new(demo_issue(), false),
            AccountMeta::new_readonly(demo_source(), false),
            AccountMeta::new_readonly(ISSUER, true),
            AccountMeta::new(ISSUER_USDC, false),
            AccountMeta::new(ESCROW_VAULT, false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(token_program().0, false),
        ],
    )
}

/// `transfer_checked` with the set a client would assemble from the list in
/// the mint: four mandatory accounts, then the list, the issue, both ledgers
/// and the hook program itself — the same shape as in `tests/hook.rs`. Data is
/// packed by the token program's crate.
fn transfer_ix(from: usize, to: usize, amount: u64) -> Instruction {
    Instruction::new_with_bytes(
        token_program().0,
        &TokenInstruction::TransferChecked {
            amount,
            decimals: BOND_DECIMALS,
        }
        .pack(),
        vec![
            AccountMeta::new(bond_key(from), false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new(bond_key(to), false),
            AccountMeta::new_readonly(owner_key(from), true),
            AccountMeta::new_readonly(extra_metas_pda(BOND_MINT).0, false),
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(ledger_key(owner_key(from)), false),
            AccountMeta::new(ledger_key(owner_key(to)), false),
            AccountMeta::new_readonly(club_id(), false),
        ],
    )
}

/// An offer on the book: where its lot sits and whose it is.
#[derive(Clone, Copy, Debug)]
struct Standing {
    key: Pubkey,
    escrow: Pubkey,
    seller: usize,
    lot: u64,
    price: u64,
}

impl Standing {
    fn new(seller: usize, nonce: u64, lot: u64, price: u64) -> Self {
        let key = offer_pda(demo_issue(), owner_key(seller), nonce).0;

        Self {
            key,
            escrow: offer_escrow_pda(key).0,
            seller,
            lot,
            price,
        }
    }

    fn ledger(&self) -> Pubkey {
        ledger_key(self.key)
    }

    fn proceeds(&self) -> Pubkey {
        offer_proceeds_pda(self.key).0
    }
}

/// Order of the metas — the `CreateOffer` declaration.
fn create_offer_ix(offer: &Standing, nonce: u64) -> Instruction {
    let seller = owner_key(offer.seller);

    Instruction::new_with_bytes(
        market_id(),
        &daddys_market::instruction::CreateOffer {
            nonce,
            amount: offer.lot,
            price: offer.price,
        }
        .data(),
        vec![
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(seller, true),
            AccountMeta::new(bond_key(offer.seller), false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new(offer.key, false),
            AccountMeta::new(offer.escrow, false),
            AccountMeta::new(ledger_key(seller), false),
            AccountMeta::new(offer.ledger(), false),
            AccountMeta::new_readonly(extra_metas_pda(BOND_MINT).0, false),
            AccountMeta::new_readonly(club_id(), false),
            AccountMeta::new_readonly(token_program().0, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    )
}

/// Order of the metas — the `BuyOffer` declaration.
fn buy_offer_ix(offer: &Standing, buyer: usize) -> Instruction {
    let owner = owner_key(buyer);

    Instruction::new_with_bytes(
        market_id(),
        &daddys_market::instruction::BuyOffer {}.data(),
        vec![
            AccountMeta::new_readonly(config_pda().0, false),
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(offer.key, false),
            AccountMeta::new(offer.escrow, false),
            AccountMeta::new(offer.proceeds(), false),
            AccountMeta::new(owner_key(offer.seller), false),
            AccountMeta::new(usdc_key(offer.seller), false),
            AccountMeta::new(owner, true),
            AccountMeta::new(usdc_key(buyer), false),
            AccountMeta::new(bond_key(buyer), false),
            AccountMeta::new(ledger_key(owner), false),
            AccountMeta::new(offer.ledger(), false),
            AccountMeta::new(ESCROW_VAULT, false),
            AccountMeta::new(FEE_VAULT, false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(extra_metas_pda(BOND_MINT).0, false),
            AccountMeta::new_readonly(club_id(), false),
            AccountMeta::new_readonly(token_program().0, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    )
}

/// Order of the metas — the `CancelOffer` declaration.
fn cancel_offer_ix(offer: &Standing) -> Instruction {
    let seller = owner_key(offer.seller);

    Instruction::new_with_bytes(
        market_id(),
        &daddys_market::instruction::CancelOffer {}.data(),
        vec![
            AccountMeta::new_readonly(demo_issue(), false),
            AccountMeta::new(offer.key, false),
            AccountMeta::new(offer.escrow, false),
            AccountMeta::new(offer.proceeds(), false),
            AccountMeta::new(seller, true),
            AccountMeta::new(bond_key(offer.seller), false),
            AccountMeta::new(usdc_key(offer.seller), false),
            AccountMeta::new(ledger_key(seller), false),
            AccountMeta::new(offer.ledger(), false),
            AccountMeta::new(ESCROW_VAULT, false),
            AccountMeta::new_readonly(BOND_MINT, false),
            AccountMeta::new_readonly(USDC_MINT, false),
            AccountMeta::new_readonly(extra_metas_pda(BOND_MINT).0, false),
            AccountMeta::new_readonly(club_id(), false),
            AccountMeta::new_readonly(token_program().0, false),
            AccountMeta::new_readonly(system_program().0, false),
        ],
    )
}

// ---- Bench -----------------------------------------------------------------

/// How a step ended.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Outcome {
    Ok,
    /// A named refusal: a code some program gave itself.
    Refused(u32),
    /// Anything else — a token program refusal, an SVM failure.
    Broke(String),
    /// Nothing to act on: a purchase or cancellation with no matching offer on
    /// the book. Nothing is sent.
    Idle,
}

impl Outcome {
    fn of(result: &InstructionResult) -> Self {
        match &result.program_result {
            ProgramResult::Success => Self::Ok,
            ProgramResult::Failure(ProgramError::Custom(code)) => Self::Refused(*code),
            other => Self::Broke(format!("{other:?}")),
        }
    }
}

fn code(error: ClubError) -> u32 {
    u32::from(error)
}

fn market_code(error: MarketError) -> u32 {
    u32::from(error)
}

fn anchor_code(error: anchor_lang::error::ErrorCode) -> u32 {
    u32::from(error)
}

/// One SVM for the whole corpus: `Mollusk::new` loads the `.so` files from
/// disk every time, and a thousand sequences would be a thousand loads. No
/// state leaks between sequences: the account store is replaced whole, and
/// the context supplies programs and sysvars on every call.
struct Bench {
    context: MolluskContext<Store>,
}

impl Bench {
    fn new() -> Self {
        Self {
            context: setup().with_context(Store::new()),
        }
    }

    fn load(&self, store: Store) {
        *self.context.account_store.borrow_mut() = store;
    }

    fn put(&self, key: Pubkey, account: Account) {
        self.context
            .account_store
            .borrow_mut()
            .store_account(key, account);
    }

    fn send(&self, ix: &Instruction) -> Outcome {
        Outcome::of(&self.context.process_instruction(ix))
    }

    fn account(&self, key: &Pubkey) -> Account {
        self.context
            .account_store
            .borrow()
            .get_account(key)
            .unwrap_or_else(|| panic!("account {key} is in the world"))
    }

    fn state<T: AccountDeserialize>(&self, key: &Pubkey) -> T {
        T::try_deserialize(&mut self.account(key).data.as_slice()).expect("account decodes")
    }

    /// Whether a ledger of ours lies at this address.
    fn has_ledger(&self, key: &Pubkey) -> bool {
        self.context
            .account_store
            .borrow()
            .get_account(key)
            .is_some_and(|account| account.owner == club_id() && !account.data.is_empty())
    }

    fn amount(&self, key: &Pubkey) -> u64 {
        let account = self.account(key);

        StateWithExtensions::<TokenState>::unpack(&account.data)
            .expect("token account unpacks")
            .base
            .amount
    }

    fn balance(&self, key: &Pubkey) -> i128 {
        i128::from(self.amount(key))
    }

    fn supply(&self, key: &Pubkey) -> u64 {
        let account = self.account(key);

        StateWithExtensions::<MintState>::unpack(&account.data)
            .expect("mint unpacks")
            .base
            .supply
    }
}

/// The world before the issue is created: protocol, source, the issuer's
/// wallet and the account the fee arrives on. The demo pool is not raised —
/// there is no swap in this file, and interception needs only its PDA's
/// signature.
fn base_store() -> Store {
    let pool = issuer_authority().0;
    let mut store = Store::new();

    store.insert(config_pda().0, anchor_account(&stored_config()));
    // `FR-007`: an issue is admitted only on a source that has already seen revenue.
    store.insert(
        demo_source(),
        anchor_account(&stored_source(None, OBSERVED_BEFORE)),
    );
    store.insert(ISSUER, wallet());
    store.insert(ISSUER_USDC, usdc_account(ISSUER, PREPAY_FUNDS));
    store.insert(FEE_VAULT, usdc_account(ADMIN, 0));
    store.insert(USDC_MINT, usdc_mint(MINT_SUPPLY));
    store.insert(pool, uninitialized());
    store.insert(SOURCE_VAULT, token_account(USDC_MINT, pool, VAULT_FUNDS));

    store
}

// ---- Trace of a sequence ---------------------------------------------------

/// Where in the sequence a step stands. The random part is the set from
/// `SC-003`; the rest is the measurement after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// The random part.
    Script,
    /// Every offer still standing is cancelled, so that what accrued on it
    /// reaches its seller before the sweep.
    Close,
    /// Every wallet with a ledger claims.
    Sweep,
    /// And once more — nothing may be left to take.
    Rinse,
}

/// A purchase as the book priced it.
#[derive(Clone, Copy, Debug)]
struct Trade {
    buyer: usize,
    seller: usize,
    price: u64,
    fee: u64,
}

#[derive(Clone, Debug)]
struct Step {
    phase: Phase,
    op: Op,
    outcome: Outcome,
    /// Wallets taking part in the step.
    parties: Vec<usize>,
    /// Whether the parties had a ledger before the step, in the same order.
    ledgers: Vec<bool>,
    /// Bonds the step meant to move: the transfer, the listed lot, the lot
    /// bought or cancelled.
    bonds: u64,
    trade: Option<Trade>,
    /// USDC that left the issuer's accounts (an arrival or a repayment).
    from_issuer: i128,
    /// Obligation still owed **before** the step.
    owed_before: i128,
    /// Escrow balance **after** the step, counted from issuance.
    escrow_after: i128,
    /// Both sides of the `SC-003` equality, accumulated up to this step.
    credited: i128,
    paid: i128,
    /// USDC change on each wallet's account, and on the treasury.
    cash: Vec<i128>,
    treasury: i128,
    /// What left the escrow for an owner in this step, and to whom it belongs:
    /// the claimant, or the seller of the offer whose ledger was claimed.
    payout: i128,
    payee: Option<usize>,
    /// Bonds on each wallet's own account before and after the step.
    held_before: Vec<u64>,
    held_after: Vec<u64>,
    /// Bonds in wallets and in standing offers after the step.
    bonds_total: u64,
    /// For each wallet, accumulated after the step: its share of everything
    /// credited so far, times supply, and what it has received from the
    /// escrow.
    entitled: Vec<i128>,
    paid_to: Vec<i128>,
}

/// A ledger at the end of a sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ledger {
    index_at_checkpoint: u128,
    accrued: u64,
    claimed_total: u64,
}

impl From<HolderCheckpoint> for Ledger {
    fn from(holder: HolderCheckpoint) -> Self {
        Self {
            index_at_checkpoint: holder.index_at_checkpoint,
            accrued: holder.accrued,
            claimed_total: holder.claimed_total,
        }
    }
}

#[derive(Clone, Debug)]
struct Sequence {
    seed: u64,
    lots: Vec<u64>,
    wallets: usize,
    script: Vec<Op>,
    steps: Vec<Step>,
    /// A build step that failed in a way not foreseen and cut the sequence.
    broke_at: Option<String>,
    supply: u64,
    obligation: i128,
    state: IssueState,
    /// What the program wrote down for itself.
    repaid_total: i128,
    payout_index: u128,
    ledgers: Vec<Option<Ledger>>,
    offer_ledgers: Vec<Ledger>,
    /// What was measured on the accounts: paid by a claim of the wallet's own,
    /// and forwarded to it from the ledger of its offers.
    direct: Vec<i128>,
    forwarded: Vec<i128>,
    escrow_final: i128,
}

impl Sequence {
    fn holders(&self) -> usize {
        self.lots.len()
    }

    fn last(&self) -> Option<&Step> {
        self.steps.last()
    }

    fn credited(&self) -> i128 {
        self.last().map_or(0, |step| step.credited)
    }

    fn paid(&self) -> i128 {
        self.last().map_or(0, |step| step.paid)
    }

    fn paid_to(&self, wallet: usize) -> i128 {
        self.last().map_or(0, |step| step.paid_to[wallet])
    }

    fn entitled(&self, wallet: usize) -> i128 {
        self.last().map_or(0, |step| step.entitled[wallet])
    }

    fn steps_in(&self, phase: Phase) -> impl Iterator<Item = &Step> + '_ {
        self.steps.iter().filter(move |step| step.phase == phase)
    }
}

/// A sequence that did not reach its end: building the world failed in a way
/// not foreseen. The empty trace is the signal, and
/// `no_step_of_any_sequence_is_refused_for_a_reason_that_was_not_named`
/// catches it.
fn aborted(seed: u64, lots: Vec<u64>, wallets: usize, script: Vec<Op>, at: String) -> Sequence {
    Sequence {
        seed,
        lots,
        wallets,
        script,
        steps: Vec::new(),
        broke_at: Some(at),
        supply: 0,
        obligation: 0,
        state: IssueState::Subscribing,
        repaid_total: 0,
        payout_index: 0,
        ledgers: Vec::new(),
        offer_ledgers: Vec::new(),
        direct: Vec::new(),
        forwarded: Vec::new(),
        escrow_final: 0,
    }
}

/// A sequence in progress, from the moment the issue is `Repaying`.
struct Run<'a> {
    bench: &'a Bench,
    wallets: usize,
    supply: u64,
    escrow_start: i128,
    book: Vec<Standing>,
    nonces: Vec<u64>,
    listed: Vec<Standing>,
    steps: Vec<Step>,
    credited: i128,
    paid: i128,
    entitled: Vec<i128>,
    paid_to: Vec<i128>,
    direct: Vec<i128>,
    forwarded: Vec<i128>,
}

impl Run<'_> {
    fn has_ledger(&self, wallet: usize) -> bool {
        self.bench.has_ledger(&ledger_key(owner_key(wallet)))
    }

    fn held(&self) -> Vec<u64> {
        (0..self.wallets)
            .map(|wallet| self.bench.amount(&bond_key(wallet)))
            .collect()
    }

    fn cash(&self) -> Vec<i128> {
        (0..self.wallets)
            .map(|wallet| self.bench.balance(&usdc_key(wallet)))
            .collect()
    }

    fn issuer(&self) -> i128 {
        self.bench.balance(&SOURCE_VAULT) + self.bench.balance(&ISSUER_USDC)
    }

    /// The bonds each wallet owns economically: on its own account and in its
    /// offers. A lot in an offer escrow still earns for its seller — that is
    /// what the escrow's ledger is for.
    fn owned(&self, held: &[u64]) -> Vec<u64> {
        let mut owned = held.to_vec();
        for offer in &self.book {
            owned[offer.seller] += self.bench.amount(&offer.escrow);
        }

        owned
    }

    /// Offers on the book that `buyer` did not place — a purchase of one's own
    /// offer is not a trade, and the corpus does not send it.
    fn buyable(&self, buyer: usize) -> Vec<usize> {
        self.book
            .iter()
            .enumerate()
            .filter(|(_, offer)| offer.seller != buyer)
            .map(|(position, _)| position)
            .collect()
    }

    fn step(&mut self, phase: Phase, op: Op) -> Outcome {
        let issue: Issue = self.bench.state(&demo_issue());
        let owed_before = i128::from(issue.obligation_total) - i128::from(issue.repaid_total);

        let held_before = self.held();
        let owned = self.owned(&held_before);
        let cash_before = self.cash();
        let treasury_before = self.bench.balance(&FEE_VAULT);
        let issuer_before = self.issuer();

        let mut parties = Vec::new();
        let mut bonds = 0;
        let mut trade = None;
        // A new offer to put on the book, or the position of one to take off.
        let mut placed = None;
        let mut taken = None;

        let ix = match op {
            Op::Arrival(amount) => Some(arrival_ix(amount)),
            Op::Claim(wallet) => {
                parties.push(wallet);
                Some(claim_ix(wallet))
            }
            Op::Prepay => Some(prepay_ix()),
            Op::Transfer { from, to, part } => {
                parties.extend([from, to]);
                bonds = part_of(held_before[from], part);
                Some(transfer_ix(from, to, bonds))
            }
            Op::List {
                seller,
                part,
                price,
            } => {
                parties.push(seller);
                bonds = part_of(held_before[seller], part);
                let nonce = self.nonces[seller];
                self.nonces[seller] += 1;
                let offer = Standing::new(seller, nonce, bonds, price_of(bonds, price));
                placed = Some(offer);
                Some(create_offer_ix(&offer, nonce))
            }
            Op::Buy { buyer, pick } => {
                let buyable = self.buyable(buyer);
                if buyable.is_empty() {
                    parties.push(buyer);
                    None
                } else {
                    let position = buyable[(pick % buyable.len() as u64) as usize];
                    let offer = self.book[position];
                    parties.extend([buyer, offer.seller]);
                    bonds = offer.lot;
                    trade = Some(Trade {
                        buyer,
                        seller: offer.seller,
                        price: offer.price,
                        fee: fee_of(offer.price),
                    });
                    taken = Some(position);
                    Some(buy_offer_ix(&offer, buyer))
                }
            }
            Op::Cancel { pick } => {
                if self.book.is_empty() {
                    None
                } else {
                    let position = (pick % self.book.len() as u64) as usize;
                    let offer = self.book[position];
                    parties.push(offer.seller);
                    bonds = offer.lot;
                    taken = Some(position);
                    Some(cancel_offer_ix(&offer))
                }
            }
        };

        let ledgers = parties
            .iter()
            .map(|wallet| self.has_ledger(*wallet))
            .collect();

        let outcome = match &ix {
            Some(ix) => self.bench.send(ix),
            None => Outcome::Idle,
        };

        if outcome == Outcome::Ok {
            if let Some(offer) = placed {
                self.book.push(offer);
                self.listed.push(offer);
            }
            if let Some(position) = taken {
                self.book.remove(position);
            }
        }

        let held_after = self.held();
        let cash: Vec<i128> = self
            .cash()
            .iter()
            .zip(&cash_before)
            .map(|(after, before)| after - before)
            .collect();
        let treasury = self.bench.balance(&FEE_VAULT) - treasury_before;
        let from_issuer = issuer_before - self.issuer();

        // What reached owners from the escrow, measured on the owners' side:
        // every wallet and the treasury together. In a purchase the buyer's
        // price goes to the seller and the treasury, so the sum is exactly what
        // was forwarded from the escrow's ledger.
        let reached = cash.iter().sum::<i128>() + treasury;

        let (payee, payout) = match (op, trade) {
            (Op::Claim(wallet), _) => (Some(wallet), cash[wallet]),
            (Op::Buy { .. }, Some(trade)) if outcome == Outcome::Ok => (
                Some(trade.seller),
                cash[trade.seller] - i128::from(trade.price - trade.fee),
            ),
            (Op::Cancel { .. }, _) if outcome == Outcome::Ok => {
                let seller = parties[0];
                (Some(seller), cash[seller])
            }
            _ => (None, 0),
        };

        if let Some(wallet) = payee {
            self.paid_to[wallet] += payout;
            match op {
                Op::Claim(_) => self.direct[wallet] += payout,
                _ => self.forwarded[wallet] += payout,
            }
        }

        // The share model. It does not read the index: every unit credited
        // into the escrow is divided by the bonds each wallet owned at that
        // moment. An arrival moves no bonds, so the balances before it are the
        // balances during it.
        if from_issuer > 0 {
            for (wallet, bonds) in owned.iter().enumerate() {
                self.entitled[wallet] += from_issuer * i128::from(*bonds);
            }
        }

        self.credited += from_issuer;
        self.paid += reached;

        let in_offers: u64 = self
            .book
            .iter()
            .map(|offer| self.bench.amount(&offer.escrow))
            .sum();
        let bonds_total = held_after.iter().sum::<u64>() + in_offers;

        self.steps.push(Step {
            phase,
            op,
            outcome: outcome.clone(),
            parties,
            ledgers,
            bonds,
            trade,
            from_issuer,
            owed_before,
            escrow_after: self.bench.balance(&ESCROW_VAULT) - self.escrow_start,
            credited: self.credited,
            paid: self.paid,
            cash,
            treasury,
            payout,
            payee,
            held_before,
            held_after,
            bonds_total,
            entitled: self.entitled.clone(),
            paid_to: self.paid_to.clone(),
        });

        outcome
    }
}

/// One sequence, from the creation of the issue to the last payout.
fn play(bench: &Bench, seed: u64) -> Sequence {
    let mut rng = Rng::new(seed);
    let holders = rng.between(1, MAX_HOLDERS as u64) as usize;
    let wallets = holders + NEWCOMERS;
    let face = face(&mut rng).max(min_lot() * holders as u64);
    let lots = deal(&mut rng, holders, face);
    let script = script(&mut rng, wallets, face);
    // The last subscriber offers more than is left: `FR-009` accepts
    // `min(offered, remaining)`, and the surplus has to stay on their account
    // rather than go into the vault.
    let over_offer = rng.between(1, min_lot() * 10);
    let offer_of = |index: usize| {
        if index + 1 == holders {
            lots[index] + over_offer
        } else {
            lots[index]
        }
    };

    bench.load(base_store());
    for index in 0..wallets {
        let owner = owner_key(index);
        let subscription = if index < holders { offer_of(index) } else { 0 };

        bench.put(owner, wallet());
        bench.put(
            usdc_key(index),
            usdc_account(owner, subscription + face * CASH_PER_FACE),
        );
        bench.put(bond_key(index), bond_account(owner, 0));
    }

    // Building the world is not part of the random set, but it is no reason
    // to panic either: a refusal here means one of the instructions broke, and
    // that is what the test has to say, not a stack trace.
    let outcome = bench.send(&create_issue_ix(face));
    if outcome != Outcome::Ok {
        return aborted(
            seed,
            lots,
            wallets,
            script,
            format!("create_issue: {outcome:?}"),
        );
    }

    for index in 0..holders {
        let outcome = bench.send(&open_ix(index));
        if outcome != Outcome::Ok {
            return aborted(
                seed,
                lots,
                wallets,
                script,
                format!("open_position {index}: {outcome:?}"),
            );
        }

        let outcome = bench.send(&subscribe_ix(index, offer_of(index)));
        if outcome != Outcome::Ok {
            return aborted(
                seed,
                lots,
                wallets,
                script,
                format!("subscribe {index}: {outcome:?}"),
            );
        }
    }

    let outcome = bench.send(&withdraw_ix());
    if outcome != Outcome::Ok {
        return aborted(
            seed,
            lots,
            wallets,
            script,
            format!("withdraw_proceeds: {outcome:?}"),
        );
    }

    // The measurement starts here. Everything before is face, and it does not
    // belong to the `SC-003` equality: that one is about intercepted revenue.
    let mut run = Run {
        bench,
        wallets,
        supply: bench.supply(&BOND_MINT),
        escrow_start: bench.balance(&ESCROW_VAULT),
        book: Vec::new(),
        nonces: vec![0; wallets],
        listed: Vec::new(),
        steps: Vec::new(),
        credited: 0,
        paid: 0,
        entitled: vec![0; wallets],
        paid_to: vec![0; wallets],
        direct: vec![0; wallets],
        forwarded: vec![0; wallets],
    };

    for op in &script {
        run.step(Phase::Script, *op);
    }

    // Close. The loop stops on a failed cancellation instead of spinning on
    // it; the failure itself is in the trace.
    while !run.book.is_empty() {
        if run.step(Phase::Close, Op::Cancel { pick: 0 }) != Outcome::Ok {
            break;
        }
    }

    let sweep: Vec<usize> = (0..wallets).filter(|w| run.has_ledger(*w)).collect();
    for wallet in &sweep {
        run.step(Phase::Sweep, Op::Claim(*wallet));
    }
    for wallet in &sweep {
        run.step(Phase::Rinse, Op::Claim(*wallet));
    }

    let issue: Issue = bench.state(&demo_issue());
    let ledgers = (0..wallets)
        .map(|wallet| {
            let key = ledger_key(owner_key(wallet));
            bench
                .has_ledger(&key)
                .then(|| Ledger::from(bench.state::<HolderCheckpoint>(&key)))
        })
        .collect();
    let offer_ledgers = run
        .listed
        .iter()
        .map(|offer| Ledger::from(bench.state::<HolderCheckpoint>(&offer.ledger())))
        .collect();

    Sequence {
        seed,
        lots,
        wallets,
        script,
        steps: run.steps,
        broke_at: None,
        supply: run.supply,
        obligation: i128::from(issue.obligation_total),
        state: issue.state,
        repaid_total: i128::from(issue.repaid_total),
        payout_index: issue.payout_index,
        ledgers,
        offer_ledgers,
        direct: run.direct,
        forwarded: run.forwarded,
        escrow_final: bench.balance(&ESCROW_VAULT) - run.escrow_start,
    }
}

/// The corpus is built once per binary: a thousand sequences are tens of
/// thousands of real instructions, and cloning their trace is free.
fn corpus() -> &'static Vec<Sequence> {
    static CORPUS: OnceLock<Vec<Sequence>> = OnceLock::new();

    CORPUS.get_or_init(|| {
        let bench = Bench::new();
        let mut master = Rng::new(SEED);

        (0..SEQUENCES)
            .map(|_| play(&bench, master.next()))
            .collect()
    })
}

// ---- 1. Corpus -------------------------------------------------------------

/// The prop under every other test. "A thousand random sequences" is not a
/// variable name: there have to be a thousand, they have to differ, and what
/// the set is run for has to happen in them. Without this test the rest of
/// the file could prove zero discrepancies over a thousand identical runs in
/// which nothing happens.
#[test]
fn a_thousand_sequences_are_a_thousand_different_sequences() {
    let corpus = corpus();
    assert_eq!(corpus.len(), SEQUENCES, "corpus of the wrong size");

    let shapes: HashSet<Vec<Op>> = corpus.iter().map(|run| run.script.clone()).collect();
    assert_eq!(
        shapes.len(),
        SEQUENCES,
        "two sequences in the corpus are the same — the generator repeats itself"
    );

    // The shape of the corpus is pinned by numbers, not inequalities: it does
    // not depend on the program — the generator builds it from the seed alone
    // — so any change to the generator has to show as a changed digit, not as
    // a quietly different set.
    let mut with_holders = [0usize; MAX_HOLDERS + 1];
    for run in corpus {
        with_holders[run.holders()] += 1;
    }
    assert_eq!(with_holders, [0, 263, 254, 253, 230], "split by holders");

    // Faces have to spread over orders of magnitude, or the corpus measures
    // one scale.
    let smallest = corpus.iter().map(|run| run.supply).min().unwrap_or(0);
    let largest = corpus.iter().map(|run| run.supply).max().unwrap_or(0);
    assert!(
        smallest > 0 && largest / smallest >= 1_000,
        "faces in the corpus did not spread: from {smallest} to {largest}"
    );

    let ops = || corpus.iter().flat_map(|run| &run.script);
    let count = |kind: fn(&Op) -> bool| ops().filter(|&op| kind(op)).count();
    let shape = [
        count(|op| matches!(op, Op::Arrival(_))),
        count(|op| matches!(op, Op::Claim(_))),
        count(|op| matches!(op, Op::Prepay)),
        count(|op| matches!(op, Op::Transfer { .. })),
        count(|op| matches!(op, Op::List { .. })),
        count(|op| matches!(op, Op::Buy { .. })),
        count(|op| matches!(op, Op::Cancel { .. })),
    ];
    assert_eq!(
        shape,
        [4_182, 1_801, 572, 1_829, 1_424, 1_144, 951],
        "arrivals, claims, prepayments, transfers, listings, purchases, cancellations"
    );

    // Penny arrivals are counted from the shape too: 12% of less than nine is
    // zero, and nothing goes into the escrow from such an arrival.
    let dust = ops()
        .filter(|op| match op {
            Op::Arrival(amount) => i128::from(*amount) * i128::from(pledge_bps()) / 10_000 == 0,
            _ => false,
        })
        .count();
    assert_eq!(dust, 819, "penny arrivals in the corpus");

    // From here on it is not the shape but what came of it. These numbers
    // depend on the program, so they are bounds: the test has to go red on an
    // empty corpus, not because a payout moved by one unit. Each counter is a
    // case the corpus exists for.
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut note = |case: &'static str| *seen.entry(case).or_default() += 1;

    for run in corpus {
        for step in &run.steps {
            let ok = step.outcome == Outcome::Ok;
            match (step.phase, step.op) {
                (Phase::Script, Op::Arrival(amount)) => {
                    let share = i128::from(amount) * i128::from(pledge_bps()) / 10_000;
                    if step.owed_before > 0 && share > step.owed_before {
                        note("an arrival capped at the remainder (FR-020)");
                    }
                    if step.owed_before == 0 && share > 0 {
                        note("an arrival on a repaid issue (FR-019)");
                    }
                }
                (_, Op::Claim(wallet)) => {
                    if step.payout > 0 {
                        note("a claim that paid something");
                        if step.held_before[wallet] == 0 {
                            note("a claim with no bonds left, paid from what accrued before (FR-017)");
                        }
                    } else if step.ledgers[0] {
                        note("a claim with nothing owed");
                    } else {
                        note("a claim by a wallet with no ledger");
                    }
                }
                (Phase::Script, Op::Prepay) => {
                    if ok {
                        note("an early repayment");
                    } else {
                        note("a repayment of a repaid issue");
                    }
                }
                (_, Op::Transfer { .. }) => {
                    if ok && step.bonds > 0 {
                        note("a transfer");
                    }
                    if ok && step.bonds == 0 {
                        note("a transfer of nothing");
                    }
                    if !step.ledgers[1] {
                        note("a transfer to a wallet with no ledger (FR-038)");
                    }
                }
                (_, Op::List { .. }) => {
                    if ok {
                        note("a listing");
                    } else {
                        note("a listing of nothing");
                    }
                }
                (_, Op::Buy { .. }) => {
                    if step.outcome == Outcome::Idle {
                        note("a purchase with nothing to buy");
                    }
                    if !ok {
                        continue;
                    }
                    note("a purchase");
                    if !step.ledgers[0] {
                        note("a purchase that opened the buyer's ledger");
                    }
                    if step.payout > 0 {
                        note("a purchase that forwarded what accrued on the offer");
                    }
                    if step.trade.is_some_and(|trade| trade.fee == 0) {
                        note("a purchase too small to carry a fee");
                    }
                }
                (Phase::Script, Op::Cancel { .. }) => {
                    if ok {
                        note("a cancellation");
                    }
                    if step.outcome == Outcome::Idle {
                        note("a cancellation with nothing to cancel");
                    }
                    if step.payout > 0 {
                        note("a cancellation that forwarded what accrued on the offer");
                    }
                }
                (Phase::Close, Op::Cancel { .. }) => note("an offer still standing at the end"),
                _ => {}
            }
        }

        let newcomer_holds = (run.holders()..run.wallets)
            .any(|wallet| run.last().is_some_and(|step| step.held_after[wallet] > 0));
        if newcomer_holds {
            note("a sequence that ends with bonds on a newcomer's account");
        }
        if run.state == IssueState::Repaid {
            note("an issue repaid in full");
        } else {
            note("an issue with a live obligation");
        }
        if run.escrow_final > 0 && run.credited() > 0 {
            // The remainder of division: what `CLAUDE.md` demands rounding
            // down for. After the sweep there is nothing left to take, and
            // something lies in the escrow.
            note("a remainder left in the escrow");
        }
    }

    let cases = [
        "an arrival capped at the remainder (FR-020)",
        "an arrival on a repaid issue (FR-019)",
        "a claim that paid something",
        "a claim with no bonds left, paid from what accrued before (FR-017)",
        "a claim with nothing owed",
        "a claim by a wallet with no ledger",
        "an early repayment",
        "a repayment of a repaid issue",
        "a transfer",
        "a transfer of nothing",
        "a transfer to a wallet with no ledger (FR-038)",
        "a listing",
        "a listing of nothing",
        "a purchase",
        "a purchase with nothing to buy",
        "a purchase that opened the buyer's ledger",
        "a purchase that forwarded what accrued on the offer",
        "a purchase too small to carry a fee",
        "a cancellation",
        "a cancellation with nothing to cancel",
        "a cancellation that forwarded what accrued on the offer",
        "an offer still standing at the end",
        "a sequence that ends with bonds on a newcomer's account",
        "an issue repaid in full",
        "an issue with a live obligation",
        "a remainder left in the escrow",
    ];
    let missing: Vec<&str> = cases
        .iter()
        .copied()
        .filter(|case| !seen.contains_key(case))
        .collect();
    assert!(missing.is_empty(), "the corpus never has: {missing:#?}");

    // Steps outnumber operations: after the random part every open offer is
    // closed and every wallet with a ledger is swept twice.
    let steps: usize = corpus.iter().map(|run| run.steps.len()).sum();
    assert_eq!(steps, 17_377, "corpus of the wrong length");
}

// ---- 2. `SC-003` -----------------------------------------------------------

/// `SC-003`: "the sum of everything paid out to owners and the remainder in
/// the escrow equals the sum of everything intercepted, to the smallest unit
/// of USDC".
///
/// The three numbers are measured from three sides and none is taken from the
/// issue's state: intercepted — how much the issuer's accounts shrank; paid —
/// how much every wallet and the treasury grew, all together; left — the
/// escrow balance. Checked **after every step**: a set in which the equality
/// held only at the end would miss a hole that the next operation closed.
///
/// **What this lock stands on.** In `T029`, none of eleven mutations of the
/// set turned it red — its neighbours went red instead — because the escrow
/// had exactly three counterparties and all three were measured. With the
/// market there are more ways out of the escrow (a purchase and a cancellation
/// claim through a CPI into an offer's own USDC account and forward from
/// there), and the equality now also says that nothing stays behind on that
/// intermediate account: it is not measured here, so anything left on it
/// would show as money missing on the owners' side.
#[test]
fn nothing_is_created_and_nothing_is_lost_at_any_step_of_any_sequence() {
    let mut checked = 0usize;

    for run in corpus() {
        for (position, step) in run.steps.iter().enumerate() {
            assert_eq!(
                step.credited,
                step.paid + step.escrow_after,
                "seed {:#x}, step {position} ({:?}): intercepted {}, paid {}, in escrow {}",
                run.seed,
                step.op,
                step.credited,
                step.paid,
                step.escrow_after
            );
            checked += 1;
        }

        assert_eq!(
            run.credited(),
            run.paid() + run.escrow_final,
            "seed {:#x}: the equality does not hold at the end of the sequence",
            run.seed
        );
    }

    assert!(
        checked >= SEQUENCES * (MIN_OPS as usize + 2),
        "too few steps checked: {checked}"
    );
}

// ---- 3. `SC-004`, first half -----------------------------------------------

/// `SC-004`: "0 cases where more than the remaining obligation went into the
/// escrow".
///
/// Measured on each step separately, because that is where it breaks: an
/// arrival larger than the remainder must put exactly the remainder into the
/// escrow and not one unit more (`FR-020`), and on a repaid issue nothing
/// (`FR-019`). No other operation may take anything from the issuer at all.
#[test]
fn the_escrow_never_takes_more_than_the_obligation_still_owes() {
    for run in corpus() {
        for (position, step) in run.steps.iter().enumerate() {
            assert!(
                step.from_issuer >= 0,
                "seed {:#x}, step {position}: money came into the issuer's accounts instead of leaving",
                run.seed
            );
            assert!(
                step.from_issuer <= step.owed_before,
                "seed {:#x}, step {position} ({:?}): {} went into the escrow while {} was owed",
                run.seed,
                step.op,
                step.from_issuer,
                step.owed_before
            );
            if !matches!(step.op, Op::Arrival(_) | Op::Prepay) {
                assert_eq!(
                    step.from_issuer, 0,
                    "seed {:#x}, step {position} ({:?}) took money from the issuer",
                    run.seed, step.op
                );
            }
        }

        assert!(
            run.credited() <= run.obligation,
            "seed {:#x}: intercepted {} against an obligation of {}",
            run.seed,
            run.credited(),
            run.obligation
        );

        // `FR-019`: a repaid issue closes exactly on the obligation, not
        // "somewhere near".
        if run.state == IssueState::Repaid {
            assert_eq!(
                run.credited(),
                run.obligation,
                "seed {:#x}: the issue is Repaid, but not all of the obligation was intercepted",
                run.seed
            );
        } else {
            assert!(
                run.credited() < run.obligation,
                "seed {:#x}: the obligation is paid, but the issue is not Repaid",
                run.seed
            );
        }
    }
}

// ---- 4. `SC-004`, second half ----------------------------------------------

/// `SC-004`: "0 cases where an owner took more than their share".
///
/// The share is not what the index computed: measuring the index by the index
/// would check it against itself. It is built from numbers the index does not
/// own: every unit credited into the escrow, divided by the bonds each wallet
/// owned **at the moment it was credited** — on its account or in its offers,
/// because a lot in an escrow still earns for its seller. With transfers the
/// share is piecewise constant in time, and this is where `SC-004` has to
/// survive the break: a buyer who received what accrued before the purchase
/// would exceed their share on the very next claim.
///
/// Checked after every step, in integers: the paid sum times supply against
/// the accumulated product of credit and bonds. Rounding down (`CLAUDE.md`)
/// makes the inequality non-strict in one direction only: taking less than
/// one's share is always possible, more — never.
#[test]
fn no_owner_ever_takes_more_than_the_bonds_they_held_earned() {
    for run in corpus() {
        assert_eq!(
            run.lots.iter().sum::<u64>(),
            run.supply,
            "seed {:#x}: the lots do not add up to the bond supply",
            run.seed
        );
        let supply = i128::from(run.supply);

        for (position, step) in run.steps.iter().enumerate() {
            for wallet in 0..run.wallets {
                assert!(
                    step.paid_to[wallet] >= 0,
                    "seed {:#x}, step {position}: wallet {wallet} gave money back to the escrow",
                    run.seed
                );
                assert!(
                    step.paid_to[wallet] * supply <= step.entitled[wallet],
                    "seed {:#x}, step {position} ({:?}): wallet {wallet} took {}, its bonds earned {}/{supply}",
                    run.seed,
                    step.op,
                    step.paid_to[wallet],
                    step.entitled[wallet]
                );
            }
        }

        assert_eq!(
            (0..run.wallets).map(|w| run.paid_to(w)).sum::<i128>(),
            run.paid(),
            "seed {:#x}: payouts attributed to wallets do not add up to what reached the owners",
            run.seed
        );
    }
}

// ---- 5. And no less ---------------------------------------------------------

/// The other side of the same share — what M2 promises: "the accrued payout is
/// correct for both sides".
///
/// `SC-004` only forbids taking more. It would stay green if a sale lost what
/// accrued before it: the seller would get less, and the money would sit in
/// the escrow where `SC-003` still counts it. So after the sweep every wallet
/// must have received its share **less rounding only**, and the rounding is
/// bounded by what the program actually does:
/// - each credit moves the index down to a whole unit, which costs a holder
///   less than `bonds / SCALE`, that is, at most `supply / SCALE + 1` units per
///   credit;
/// - each settlement of a ledger — a claim, either side of a transfer, a
///   listing, a purchase or cancellation of one's offer — rounds down once,
///   less than one unit each.
///
/// A seller losing the accrual of a whole holding period would be off by far
/// more than that.
#[test]
fn every_owner_receives_the_share_their_bonds_earned_less_rounding_only() {
    for run in corpus() {
        let supply = i128::from(run.supply);
        let credits = run.steps.iter().filter(|step| step.from_issuer > 0).count() as i128;
        let per_credit = supply / SCALE as i128 + 1;

        for wallet in 0..run.wallets {
            let settlements = run
                .steps
                .iter()
                .filter(|step| step.parties.contains(&wallet))
                .count() as i128;
            let slack = credits * per_credit + settlements + 1;
            let short = run.entitled(wallet) - run.paid_to(wallet) * supply;

            assert!(
                short < slack * supply,
                "seed {:#x}: wallet {wallet} earned {}/{supply} and received {} — short by more than rounding ({slack} units)",
                run.seed,
                run.entitled(wallet),
                run.paid_to(wallet)
            );
        }
    }
}

// ---- 6. Books against money ------------------------------------------------

/// What keeps the `SC-003` equality from being a tautology of the token
/// program.
///
/// On their own, the three measured numbers would add up even in a protocol
/// that keeps its books at random: what leaves one account arrives on another
/// — that is Token-2022's property, not ours. What is proven here is that the
/// **program's books** tell the same story: `repaid_total` is what the
/// remaining obligation is computed from, a wallet's `claimed_total` is what
/// its owner sees, and an offer's ledger records what was forwarded to the
/// seller. After the sweep no ledger holds anything accrued, and every ledger
/// stands on the current index.
#[test]
fn the_books_the_program_keeps_agree_with_the_money_that_moved() {
    for run in corpus() {
        if run.broke_at.is_some() {
            continue;
        }

        assert_eq!(
            run.repaid_total,
            run.credited(),
            "seed {:#x}: the issue recorded {} as repaid, the issuer's accounts lost {}",
            run.seed,
            run.repaid_total,
            run.credited()
        );

        for (wallet, ledger) in run.ledgers.iter().enumerate() {
            match ledger {
                Some(ledger) => {
                    assert_eq!(
                        i128::from(ledger.claimed_total),
                        run.direct[wallet],
                        "seed {:#x}: wallet {wallet}'s ledger says {}, its account — {}",
                        run.seed,
                        ledger.claimed_total,
                        run.direct[wallet]
                    );
                    assert_eq!(
                        ledger.accrued, 0,
                        "seed {:#x}: wallet {wallet} still has something accrued after the sweep",
                        run.seed
                    );
                    // A claim with nothing owed is refused and writes nothing,
                    // so a ledger may keep an old checkpoint — on an empty
                    // wallet, or where what the balance earned since rounds
                    // down to zero. Either way nothing may be left owed on it.
                    let holds = run.last().map_or(0, |step| step.held_after[wallet]);
                    let unclaimed =
                        (run.payout_index - ledger.index_at_checkpoint) * u128::from(holds) / SCALE;
                    assert_eq!(
                        unclaimed, 0,
                        "seed {:#x}: wallet {wallet} holds {holds} and is still owed {unclaimed} after the sweep",
                        run.seed
                    );
                }
                None => assert_eq!(
                    run.paid_to(wallet),
                    0,
                    "seed {:#x}: wallet {wallet} has no ledger and was paid anyway",
                    run.seed
                ),
            }
        }

        let forwarded: i128 = run.forwarded.iter().sum();
        let recorded: i128 = run
            .offer_ledgers
            .iter()
            .map(|ledger| i128::from(ledger.claimed_total))
            .sum();
        assert_eq!(
            recorded, forwarded,
            "seed {:#x}: offer ledgers record {recorded} claimed, sellers received {forwarded}",
            run.seed
        );
        for ledger in &run.offer_ledgers {
            assert_eq!(
                ledger.accrued, 0,
                "seed {:#x}: an offer is gone, and something accrued on it is left behind",
                run.seed
            );
        }
    }
}

// ---- 7. Bonds --------------------------------------------------------------

/// Every bond is where the operation put it.
///
/// The share model above is only as good as the balances it divides by, so
/// they are checked here: a transfer moves exactly the amount from sender to
/// recipient, a listing moves the lot off the seller's account and a purchase
/// onto the buyer's, a cancellation brings it back. Nothing else moves any
/// bond, a refused operation moves nothing at all, and no bond is created or
/// lost — wallets and standing offers together always hold the whole supply.
#[test]
fn every_bond_is_where_the_operation_put_it() {
    for run in corpus() {
        for (position, step) in run.steps.iter().enumerate() {
            let at = format!("seed {:#x}, step {position} ({:?})", run.seed, step.op);

            assert_eq!(
                step.bonds_total, run.supply,
                "{at}: bonds were created or lost"
            );

            let mut expected = step.held_before.clone();
            if step.outcome == Outcome::Ok {
                match step.op {
                    Op::Transfer { from, to, .. } => {
                        expected[from] -= step.bonds;
                        expected[to] += step.bonds;
                    }
                    Op::List { seller, .. } => expected[seller] -= step.bonds,
                    Op::Buy { buyer, .. } => expected[buyer] += step.bonds,
                    Op::Cancel { .. } => expected[step.parties[0]] += step.bonds,
                    _ => {}
                }
            }
            assert_eq!(
                step.held_after, expected,
                "{at}: bonds are not where they should be"
            );
        }
    }
}

// ---- 8. The secondary market's money ---------------------------------------

/// The price is a third stream of USDC and it closes on its own.
///
/// In a purchase the buyer pays exactly the price, the treasury receives
/// exactly the trading fee at the protocol's rate (`FR-035`), and the seller
/// receives the rest plus whatever accrued on the offer — never less. A
/// cancellation charges nothing: the treasury is not in its account set.
/// And no other operation moves anyone's cash but the claimant's.
#[test]
fn the_secondary_market_money_closes_on_its_own() {
    for run in corpus() {
        for (position, step) in run.steps.iter().enumerate() {
            let at = format!("seed {:#x}, step {position} ({:?})", run.seed, step.op);
            let untouched = |except: &[usize]| {
                for (wallet, delta) in step.cash.iter().enumerate() {
                    if !except.contains(&wallet) {
                        assert_eq!(*delta, 0, "{at}: wallet {wallet}'s cash moved");
                    }
                }
            };

            match (step.op, step.trade) {
                (Op::Buy { .. }, Some(trade)) if step.outcome == Outcome::Ok => {
                    assert_eq!(
                        step.cash[trade.buyer],
                        -i128::from(trade.price),
                        "{at}: the buyer did not pay the price"
                    );
                    assert_eq!(
                        step.treasury,
                        i128::from(trade.fee),
                        "{at}: the treasury did not get the fee"
                    );
                    assert!(
                        step.payout >= 0,
                        "{at}: the seller got less than the price net of the fee"
                    );
                    untouched(&[trade.buyer, trade.seller]);
                }
                (Op::Claim(wallet), _) => {
                    assert_eq!(step.treasury, 0, "{at}: a claim paid the treasury");
                    untouched(&[wallet]);
                }
                (Op::Cancel { .. }, _) if step.outcome == Outcome::Ok => {
                    assert_eq!(step.treasury, 0, "{at}: a cancellation paid the treasury");
                    untouched(&[step.parties[0]]);
                }
                _ => {
                    assert_eq!(step.treasury, 0, "{at}: the treasury moved");
                    untouched(&[]);
                }
            }
        }
    }
}

// ---- 9. Refusals -----------------------------------------------------------

/// No operation is refused for a reason not named here.
///
/// In a random set an operation arrives at the wrong moment all the time: a
/// claim with nothing owed, a repayment of a repaid issue, a listing of a
/// share that rounds to nothing, bonds sent to a wallet with no ledger. These
/// are normal refusals and each has a name — and a condition: a claim may be
/// refused as `AccountNotInitialized` only by a wallet that has no ledger, and
/// a transfer only if one of its sides has none. Everything else — a token
/// program refusal, an SVM failure — means the protocol stumbled over the
/// order of operations rather than refused it.
#[test]
fn no_step_of_any_sequence_is_refused_for_a_reason_that_was_not_named() {
    let no_ledger = anchor_code(anchor_lang::error::ErrorCode::AccountNotInitialized);

    for run in corpus() {
        assert!(
            run.broke_at.is_none(),
            "seed {:#x}: the sequence was cut — {}",
            run.seed,
            run.broke_at.clone().unwrap_or_default()
        );

        for (position, step) in run.steps.iter().enumerate() {
            let everyone_has_a_ledger = step.ledgers.iter().all(|has| *has);
            let allowed: Vec<u32> = match step.op {
                // Interception is a filter, not a gate: it never refuses
                // (session 11), whatever state the issue is in.
                Op::Arrival(_) => vec![],
                Op::Claim(_) if everyone_has_a_ledger => vec![code(ClubError::NothingToClaim)],
                Op::Claim(_) => vec![no_ledger],
                Op::Prepay => vec![code(ClubError::ObligationAlreadyRepaid)],
                Op::Transfer { .. } if everyone_has_a_ledger => vec![],
                Op::Transfer { .. } => vec![no_ledger],
                Op::List { .. } if step.bonds == 0 => {
                    vec![market_code(MarketError::OfferTermsInvalid)]
                }
                Op::List { .. } | Op::Buy { .. } | Op::Cancel { .. } => vec![],
            };
            let idle_allowed = step.phase == Phase::Script
                && matches!(step.op, Op::Buy { .. } | Op::Cancel { .. });

            match &step.outcome {
                Outcome::Ok => {}
                Outcome::Refused(actual) if allowed.contains(actual) => {}
                Outcome::Idle if idle_allowed => {}
                other => panic!(
                    "seed {:#x}, step {position} ({:?}, {:?}): {other:?} — expected {allowed:?}",
                    run.seed, step.phase, step.op
                ),
            }
        }
    }
}

// ---- 10. Twice the same ----------------------------------------------------

/// The same money is not claimed twice.
///
/// The sweep leaves every ledger on the current index, so a second sweep must
/// give nobody anything — and refuse everyone the same way, as
/// `NothingToClaim`. It is the shortest way to `SC-004`: a checkpoint that did
/// not move shows here at once, before the escrow runs dry.
#[test]
fn the_same_money_is_never_claimed_twice() {
    for run in corpus() {
        if run.broke_at.is_some() {
            continue;
        }

        for step in run.steps_in(Phase::Rinse) {
            assert_eq!(
                step.payout, 0,
                "seed {:#x}: {:?} took again what it had already taken",
                run.seed, step.op
            );
            assert_eq!(
                step.outcome,
                Outcome::Refused(code(ClubError::NothingToClaim)),
                "seed {:#x}: after the sweep {:?} was not refused",
                run.seed,
                step.op
            );
        }

        // And the mirror: every wallet with a ledger was swept, and no offer
        // survived the close.
        let swept: HashSet<usize> = run
            .steps_in(Phase::Sweep)
            .filter_map(|step| step.payee)
            .collect();
        let ledgered: HashSet<usize> = (0..run.wallets)
            .filter(|wallet| run.ledgers[*wallet].is_some())
            .collect();
        assert_eq!(
            swept, ledgered,
            "seed {:#x}: the sweep missed a ledger",
            run.seed
        );
        assert!(
            run.steps_in(Phase::Close)
                .all(|step| step.outcome == Outcome::Ok),
            "seed {:#x}: an offer could not be closed",
            run.seed
        );
    }
}

// ---- 11. Reproducibility ---------------------------------------------------

/// A run replays from its seed.
///
/// A corpus that does not replay is no proof: a discrepancy found on the
/// thousandth run has to stay in the same place next time, or a failure report
/// points nowhere. The test takes a sequence from the corpus and plays it
/// again from a clean world.
#[test]
fn the_same_seed_replays_the_same_sequence() {
    let corpus = corpus();
    let bench = Bench::new();

    for run in [&corpus[0], &corpus[SEQUENCES / 2], &corpus[SEQUENCES - 1]] {
        let replay = play(&bench, run.seed);
        let outcomes = |run: &Sequence| -> Vec<Outcome> {
            run.steps.iter().map(|step| step.outcome.clone()).collect()
        };

        assert_eq!(
            replay.script, run.script,
            "seed {:#x}: another shape",
            run.seed
        );
        assert_eq!(replay.lots, run.lots, "seed {:#x}: other lots", run.seed);
        assert_eq!(
            outcomes(&replay),
            outcomes(run),
            "seed {:#x}: other outcomes",
            run.seed
        );
        assert_eq!(
            replay.credited(),
            run.credited(),
            "seed {:#x}: other interception",
            run.seed
        );
        assert_eq!(
            replay.last().map(|step| step.paid_to.clone()),
            run.last().map(|step| step.paid_to.clone()),
            "seed {:#x}: other payouts",
            run.seed
        );
        assert_eq!(
            replay.escrow_final, run.escrow_final,
            "seed {:#x}: another remainder",
            run.seed
        );
    }
}
