# Daddy's Club

A marketplace for revenue-based financing of onchain protocols on Solana.

A protocol that already earns fees issues a short-term bond with a fixed face
value and coupon. The bond repays itself: an agreed share of the protocol's fees
is split off at the moment a fee arises and distributed to bondholders until
face plus coupon has been paid out. After that the interception stops on its
own. Bonds trade until maturity.

No token dilution, no user funds as collateral, no credit committee.

## Status — v0.1.0, the first working bond

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
- **A read-only web app**: marketplace, issue detail and the issuer dashboard,
  with the repayment counter updating live from the chain.
- **A demo script** that walks the whole cycle on a local validator.

What does not exist yet:

- a secondary market — offers, bond transfers with checkpoints, the transfer
  hook's `execute` path; the bond mint already carries the hook;
- an admission threshold — in this version anyone can create an issue;
- signing transactions from the web app.

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
programs/demo-issuer   reference integration: a swap that hands over a share of its fee
packages/sdk           types, PDA derivations, account decoders, mirror of the arithmetic
apps/web               marketplace, issue detail, issuer dashboard
scripts                the full-cycle demo and the SC-002 measurement on a live node
fixtures               shared arithmetic fixtures for Rust ↔ TypeScript
```

## Development

```bash
pnpm install
pnpm gate                    # lint + typecheck + TypeScript tests
anchor build                 # both programs
cargo test -p daddys-club    # program tests on mollusk-svm; needs anchor build first
```

The program tests load the freshly built bytecode of both programs, so
`anchor build` has to run before `cargo test`.

Copy `.env.example` to `.env` and set the RPC endpoint for your cluster; the
defaults point at a local validator. The web app is started with `pnpm dev`.

To see the whole cycle end to end — a local validator with both programs, the
demo issuer generating fees, holders getting paid — see
[scripts/README.md](scripts/README.md).

## License

MIT
