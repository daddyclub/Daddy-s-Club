# Daddy's Club

A marketplace for revenue-based financing of onchain protocols on Solana.

A protocol that already earns fees issues a short-term bond with a fixed face
value and coupon. The bond repays itself: an agreed share of the protocol's fees
is split off at the moment a fee arises and distributed to bondholders until
face plus coupon has been paid out. After that the interception stops on its
own. Bonds trade until maturity.

No token dilution, no user funds as collateral, no credit committee.

## Status — v0.2.0, a way out before maturity

What ships:

- **Issuance.** An issuer registers a revenue source and creates an issue;
  investors open a position and subscribe; the face less the origination fee
  is paid out to the issuer, or refunded to investors if the issue is
  undersubscribed.
- **Repayment in the same transaction as the fee.** The issuer's own fee
  instruction calls the interception by CPI, so the split happens where the
  fee arises — no extra transactions and no external executors.
- **Payout by a cumulative index.** Holders claim their share whenever they
  like, and the cost of a fee arrival does not grow with the number of
  holders.
- **Repayment ends by itself** once face plus coupon is covered; the issuer
  can also buy the flow back early in one payment.
- **Bonds change hands without losing what they earned.** The transfer hook
  settles both sides at the moment before every transfer — including one
  sent from any wallet app, past this interface — so what accrued before a
  sale stays with the seller and the buyer earns from the purchase on. Bonds
  can only land on a wallet whose position is open.
- **A secondary market** in its own program: a holder lists part of a
  position at a fixed price into an offer escrow, another wallet buys it for
  USDC in one instruction (the protocol takes a trading fee), or the seller
  cancels and gets the whole lot back. What accrues while an offer stands is
  forwarded to the seller.
- **A web app**: marketplace, issue detail and the issuer dashboard, with the
  repayment counter updating live from the chain, and a screen for offers
  where a Wallet Standard wallet lists, buys and cancels.
- **Scripts** that walk the whole cycle on a local validator and measure it.

Measured: the money adds up to the smallest USDC unit after every step of
1,000 random sequences of fee arrivals, claims, transfers and sales, and no
holder ever takes more than their bonds earned; from listing to USDC on the
seller's screen takes 1.3 s on a local validator and 3.0 s on devnet; the
whole issuance-to-payout cycle runs in 16.4 s of machine time on devnet.

What does not exist yet:

- a catalogue of issues and risk metrics for the issuer — buying a bond is
  possible, judging whose it is is not;
- an admission threshold — in this version anyone can create an issue.

One issuer, and it is our own demo issuer.

## Stack

Anchor 0.32.1 · Rust 1.97.1 · Agave 4.2.0 · Token-2022 with Transfer Hook ·
mollusk-svm 0.15.0 · pnpm 9 workspaces · TypeScript 5.9 strict · Biome · Vitest ·
React 18 + Vite 5 · Node ≥ 22.

There is no database and no backend on purpose: the canonical state lives in
Solana accounts, and the interface reads them straight over RPC.

## Layout

```
programs/daddys-club   core: protocol config, revenue sources, issuance, escrows,
                       interception, payout; the bond mint carries the transfer hook
programs/daddys-market secondary market: offers, purchase and cancellation
programs/demo-issuer   reference integration: a swap that hands over a share of its fee
packages/sdk           types, PDA derivations, account decoders, mirror of the arithmetic
apps/web               marketplace, issue detail, issuer dashboard, offers
scripts                the full-cycle demo and the SC-002 / SC-009 measurements on a live node
fixtures               shared arithmetic fixtures for Rust ↔ TypeScript
```

## Development

```bash
pnpm install
pnpm gate                    # lint + typecheck + TypeScript tests
anchor build                 # all three programs
cargo test -p daddys-club    # program tests on mollusk-svm; needs anchor build first
```

The program tests load the freshly built bytecode of all three programs, so
`anchor build` has to run before `cargo test`.

Copy `.env.example` to `.env` and set the RPC endpoint for your cluster; the
defaults point at a local validator. The web app is started with `pnpm dev`.

To see the whole cycle end to end — a local validator with the programs, the
demo issuer generating fees, holders getting paid — see
[scripts/README.md](scripts/README.md).

## Deployment

The web app is published to GitHub Pages by
[`.github/workflows/pages.yml`](.github/workflows/pages.yml) on every push to
`main`. One-time setup for the repository owner: **Settings → Pages → Source:
GitHub Actions**. The bundle is built with `--base=/<repo>/`, and the router
takes the same prefix from `import.meta.env.BASE_URL`, so the site works at
`https://<owner>.github.io/<repo>/`.

The demo screens run on built-in figures and need no configuration. The live
screen (`/live/issue`) reads the cluster set in repository variables
`VITE_RPC_URL`, `VITE_CLUSTER` and `VITE_PROGRAM_ID` (Settings → Secrets and
variables → Actions → Variables). They end up in a public bundle, so an RPC
key placed there must be restricted to the site's origin.

### Devnet

All three programs are deployed to devnet under the same IDs as in
[`Anchor.toml`](Anchor.toml):

| | Address |
|---|---|
| `daddys_club` | `7eT5T7mq1uD9piYJ2rMAzma8iYL7C7CZGPgxsB8DckoB` |
| `daddys_market` | `G29gfknNBKtvpfPAjrigefg3tkX7cVXFgnd62NUtcniq` |
| `demo_issuer` | `8wKjGiLvnMTv7oi9PcztmbRv4v63emT2qPPrA8x1fW3z` |
| protocol config | `4HLq2cCPFRgHurFywRv24eLXxKURyz3To7FF2eGoyeg1` |
| settlement mint (test USDC, 6 decimals) | `A2BL4NnYRAjAdDyoyVXmpM55CC1EcsX6ELsS4uHqdoYX` |
| standing demo issue | `DQ3VtusT8kxeByCmWHyejcaSsDpbLfHFFqNjoErGXUMD` |

The settlement currency is a test mint of our own, not Circle's devnet USDC:
the demo needs amounts no faucet hands out. Visitors can watch the standing
issue and its offer book, but have no way to get this USDC, so trading it is
for the scripts. The repository variables for this deployment:

```
VITE_CLUSTER=devnet
VITE_RPC_URL=https://api.devnet.solana.com
```

`VITE_PROGRAM_ID` stays unset: the IDs are the same as on localnet.

`init_protocol` is open to whoever calls it first, so on devnet it was run
straight after the deploy and the admin read back from the chain. Restricting
it to the upgrade authority is due before mainnet.

## License

MIT
