/**
 * A standing issue on devnet for the showcase: in repayment, with fees already
 * split into it and one lot listed on the secondary market.
 *
 * Unlike `demo` and `measure:sc009`, this run keeps its wallets. The issue it
 * leaves behind is what the landing page links to, and moving it later — more
 * swaps, cancelling or refilling the offer — needs the issuer, trader and
 * holder keys. They are written to `scripts/.secrets/devnet-world.json` (git
 * ignores `.secrets/`) as soon as they exist, so a run that fails halfway does
 * not strand SOL on keys nobody kept.
 *
 * Visitors can watch this issue but not trade it: the settlement mint is ours
 * and there is no faucet for it.
 *
 *   RPC_URL=<devnet endpoint> ADMIN_KEYPAIR=<deployer key file> \
 *     pnpm --filter @daddys-club/scripts seed:devnet
 */

import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ComputeBudgetProgram, Connection, type Keypair, type PublicKey } from '@solana/web3.js';
import {
  extraAccountMetasPda,
  holderPda,
  offerEscrowPda,
  offerPda,
} from '../../packages/sdk/src/pda.ts';
import { detectCluster, readKeypair, redactUrl } from './lib/cluster.ts';
import {
  Args,
  CLUB_PROGRAM,
  instruction,
  MARKET_PROGRAM,
  ro,
  rw,
  SYSTEM_PROGRAM,
  signerRw,
  TOKEN_2022,
} from './lib/encode.ts';
import { readIssue } from './lib/read.ts';
import {
  formatUsdc,
  type HolderHandle,
  type IssueHandle,
  issueProceeds,
  joinIssue,
  openIssue,
  prepareWorld,
  send,
  swapInstruction,
  USDC,
  type WorldStage,
} from './lib/world.ts';

const RPC_URL = process.env.RPC_URL ?? 'https://api.devnet.solana.com';
const WEB_URL = process.env.WEB_URL ?? 'https://daddyclub.github.io/Daddy-s-Club';
const SWAPS = Number(process.env.SEED_SWAPS ?? 5);

/** Two unequal lots, as in `demo`: the second one is accepted only in part (`FR-009`). */
const OFFER = 150_000n * USDC;
/** The standing secondary-market lot: 25 000 face for 24 500 USDC. */
const LOT = 25_000n * USDC;
const PRICE = 24_500n * USDC;

const log = (line: string): void => {
  process.stdout.write(`${line}\n`);
};

const here = dirname(fileURLToPath(import.meta.url));
const worldFile = resolve(here, '../.secrets/devnet-world.json');

/** Everything needed to act on the seeded issue later. Secret keys included. */
function saveWorld(stage: WorldStage, extra: Record<string, string>): void {
  const secret = (keypair: Keypair) => ({
    address: keypair.publicKey.toBase58(),
    secretKey: [...keypair.secretKey],
  });
  mkdirSync(dirname(worldFile), { recursive: true });
  writeFileSync(
    worldFile,
    `${JSON.stringify(
      {
        cluster: stage.cluster,
        admin: stage.admin.publicKey.toBase58(),
        config: stage.config.toBase58(),
        usdcMint: stage.usdcMint.toBase58(),
        source: stage.source.toBase58(),
        issuer: secret(stage.issuer),
        trader: secret(stage.trader),
        investors: stage.investors.map((investor) => secret(investor.keypair)),
        ...extra,
        savedAt: new Date().toISOString(),
      },
      null,
      2,
    )}\n`,
    'utf8',
  );
}

/** `create_offer` of the market program; the account order is `CREATE_OFFER_ACCOUNTS` in the SDK. */
async function listLot(
  stage: WorldStage,
  issue: IssueHandle,
  holder: HolderHandle,
): Promise<PublicKey> {
  const seller = holder.investor.keypair;
  const nonce = 0n;
  const offer = offerPda(issue.issue, seller.publicKey, nonce, MARKET_PROGRAM).address;

  await send(
    stage.connection,
    [
      ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
      instruction(
        MARKET_PROGRAM,
        [
          ro(issue.issue),
          signerRw(seller.publicKey),
          rw(holder.bond),
          ro(issue.bondMint),
          rw(offer),
          rw(offerEscrowPda(offer, MARKET_PROGRAM).address),
          rw(holderPda(issue.issue, seller.publicKey, CLUB_PROGRAM).address),
          rw(holderPda(issue.issue, offer, CLUB_PROGRAM).address),
          ro(extraAccountMetasPda(issue.bondMint, CLUB_PROGRAM).address),
          ro(CLUB_PROGRAM),
          ro(TOKEN_2022),
          ro(SYSTEM_PROGRAM),
        ],
        Args.forInstruction('create_offer').u64(nonce).u64(LOT).u64(PRICE).build(),
      ),
    ],
    [seller],
  );
  return offer;
}

async function main(): Promise<void> {
  const keyPath = process.env.ADMIN_KEYPAIR;
  if (keyPath === undefined) throw new Error('ADMIN_KEYPAIR is required: the deployer key file');
  const admin = readKeypair(keyPath);

  const connection = new Connection(RPC_URL, 'confirmed');
  const cluster = await detectCluster(connection);
  log(`node: ${redactUrl(RPC_URL)} (${cluster})`);
  if (cluster !== 'devnet') throw new Error(`refusing to seed ${cluster}: devnet only`);

  const stage = await prepareWorld(connection, admin, [OFFER, OFFER], log);
  saveWorld(stage, {});
  log(`keys saved: ${worldFile}`);

  const [walletA, walletB] = stage.investors;
  if (walletA === undefined || walletB === undefined) throw new Error('two wallets expected');

  const issue = await openIssue(stage, log);
  saveWorld(stage, { issue: issue.issue.toBase58(), bondMint: issue.bondMint.toBase58() });
  const holderA = await joinIssue(stage, issue, walletA, OFFER, log);
  await joinIssue(stage, issue, walletB, OFFER, log);
  await issueProceeds(stage, issue, log);

  for (let index = 0; index < SWAPS; index += 1) {
    await send(connection, [swapInstruction(stage, issue)], [stage.trader]);
  }
  log(`${SWAPS} swaps done`);

  const offer = await listLot(stage, issue, holderA);
  log(
    `lot listed: ${formatUsdc(LOT)} face for ${formatUsdc(PRICE)} USDC — offer ${offer.toBase58()}`,
  );
  saveWorld(stage, {
    issue: issue.issue.toBase58(),
    bondMint: issue.bondMint.toBase58(),
    offer: offer.toBase58(),
  });

  const state = await readIssue(connection, issue.issue);
  log('');
  log(
    `issue ${issue.issue.toBase58()}: ${state.state}, repaid ${formatUsdc(state.repaidTotal)} USDC`,
  );
  log(`card:   ${WEB_URL}/live/issue/${issue.issue.toBase58()}`);
  log(`offers: ${WEB_URL}/live/issue/${issue.issue.toBase58()}/offers`);
}

main().catch((error: unknown) => {
  process.stderr.write(
    `${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
