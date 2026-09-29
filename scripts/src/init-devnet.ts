/**
 * One-time protocol setup on devnet, run right after the core program lands.
 *
 * `init_protocol` is open to anyone, and whoever signs it first becomes the
 * config's admin for good — along with the settlement mint and the fee vault
 * it pins. On a public cluster that makes deploy-then-init a race, so this
 * script runs immediately after `solana program deploy` and then reads the
 * config back: an admin other than ours means someone got there first, and the
 * deployment has to be redone under fresh program IDs. Closing the window in
 * the program itself is a separate task, due before mainnet.
 *
 * The settlement currency is a test mint of our own (6 decimals, authority =
 * the admin), not Circle's devnet USDC: the demo world needs millions of units,
 * which only a mint we control can provide.
 *
 *   RPC_URL=https://api.devnet.solana.com ADMIN_KEYPAIR=<deployer key file> \
 *     pnpm --filter @daddys-club/scripts init:devnet
 */

import { Connection, type PublicKey } from '@solana/web3.js';
import { decodeProtocolConfig } from '../../packages/sdk/src/accounts.ts';
import { detectCluster, readKeypair, redactUrl } from './lib/cluster.ts';
import { CLUB_PROGRAM, DEMO_PROGRAM, MARKET_PROGRAM } from './lib/encode.ts';
import { ensureConfig } from './lib/world.ts';

const RPC_URL = process.env.RPC_URL ?? 'https://api.devnet.solana.com';

const log = (line: string): void => {
  process.stdout.write(`${line}\n`);
};

async function executable(connection: Connection, program: PublicKey): Promise<boolean> {
  const account = await connection.getAccountInfo(program, 'confirmed');
  return account?.executable === true;
}

async function main(): Promise<void> {
  const keyPath = process.env.ADMIN_KEYPAIR;
  if (keyPath === undefined) throw new Error('ADMIN_KEYPAIR is required: the deployer key file');
  const admin = readKeypair(keyPath);

  const connection = new Connection(RPC_URL, 'confirmed');
  const cluster = await detectCluster(connection);
  log(`node: ${redactUrl(RPC_URL)} (${cluster})`);
  if (cluster !== 'devnet') throw new Error(`refusing to initialise ${cluster}: devnet only`);

  for (const [name, program] of [
    ['daddys_club', CLUB_PROGRAM],
    ['daddys_market', MARKET_PROGRAM],
    ['demo_issuer', DEMO_PROGRAM],
  ] as const) {
    const live = await executable(connection, program);
    log(`${name.padEnd(14)} ${program.toBase58()} ${live ? 'deployed' : 'MISSING'}`);
    if (program === CLUB_PROGRAM && !live) throw new Error('the core program is not deployed');
  }

  const { config, fresh } = await ensureConfig(connection, admin, cluster, true);

  // Read back rather than trust `fresh`: the point is to see who the chain
  // says the admin is, whichever transaction got there first.
  const account = await connection.getAccountInfo(config, 'confirmed');
  if (account === null) throw new Error('config vanished right after creation');
  const decoded = decodeProtocolConfig(account.data);
  if (!decoded.admin.equals(admin.publicKey)) {
    throw new Error(
      `config admin is ${decoded.admin.toBase58()} — someone initialised first; redeploy under new IDs`,
    );
  }

  log(fresh ? 'config created' : 'config already there, admin is ours');
  log(`config           ${config.toBase58()}`);
  log(`admin            ${decoded.admin.toBase58()}`);
  log(`settlement mint  ${decoded.usdcMint.toBase58()}`);
  log(`fee vault        ${decoded.feeVault.toBase58()}`);
  log(
    `fees             origination ${decoded.originationFeeBps} bps, trading ${decoded.tradingFeeBps} bps`,
  );
}

main().catch((error: unknown) => {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
});
