/**
 * Sets the protocol's history threshold (`FR-007`) on devnet.
 *
 * `update_config` takes the full parameter set, so every other value is read
 * from the config on chain and written back unchanged — the only thing that
 * moves is `history_threshold_secs`. The threshold is a protocol parameter,
 * not a per-issue one: it applies to every `create_issue` from the next slot on,
 * and nobody can shorten it for a single issue.
 *
 *   HISTORY_THRESHOLD_SECS=120 ADMIN_KEYPAIR=<deployer key file> \
 *     node --env-file=../.env src/update-config.ts
 */

import { Connection } from '@solana/web3.js';
import { decodeProtocolConfig } from '../../packages/sdk/src/accounts.ts';
import { configPda } from '../../packages/sdk/src/pda.ts';
import { detectCluster, readKeypair, redactUrl } from './lib/cluster.ts';
import { Args, CLUB_PROGRAM, instruction, rw, signer } from './lib/encode.ts';
import { DEMO_HISTORY_THRESHOLD_SECS, send } from './lib/world.ts';

const RPC_URL = process.env.RPC_URL ?? 'https://api.devnet.solana.com';

const log = (line: string): void => {
  process.stdout.write(`${line}\n`);
};

async function main(): Promise<void> {
  const keyPath = process.env.ADMIN_KEYPAIR;
  if (keyPath === undefined) throw new Error('ADMIN_KEYPAIR is required: the config admin key');
  const admin = readKeypair(keyPath);
  const threshold = BigInt(process.env.HISTORY_THRESHOLD_SECS ?? DEMO_HISTORY_THRESHOLD_SECS);
  if (threshold <= 0n) throw new Error('HISTORY_THRESHOLD_SECS must be above zero');

  const connection = new Connection(RPC_URL, 'confirmed');
  const cluster = await detectCluster(connection);
  log(`node: ${redactUrl(RPC_URL)} (${cluster})`);
  if (cluster !== 'devnet') throw new Error(`refusing to update ${cluster}: devnet only`);

  const config = configPda(CLUB_PROGRAM).address;
  const read = async () => {
    const account = await connection.getAccountInfo(config, 'confirmed');
    if (account === null) throw new Error('no protocol config — run init:devnet first');
    return decodeProtocolConfig(account.data);
  };

  const before = await read();
  if (!before.admin.equals(admin.publicKey)) {
    throw new Error(`config admin is ${before.admin.toBase58()}, not this key`);
  }
  log(`history threshold: ${before.historyThresholdSecs} s → ${threshold} s`);

  const data = Args.forInstruction('update_config')
    .u16(before.originationFeeBps)
    .u16(before.tradingFeeBps)
    .u16(before.maxPledgeBps)
    .i64(before.minTenorSecs)
    .i64(before.maxTenorSecs)
    .i64(threshold)
    .build();
  const signature = await send(
    connection,
    [instruction(CLUB_PROGRAM, [rw(config), signer(admin.publicKey)], data)],
    [admin],
  );
  log(`tx ${signature}`);

  // Read back: the chain, not the request, says what the threshold is now —
  // and that nothing else moved with it.
  const after = await read();
  const unchanged =
    after.originationFeeBps === before.originationFeeBps &&
    after.tradingFeeBps === before.tradingFeeBps &&
    after.maxPledgeBps === before.maxPledgeBps &&
    after.minTenorSecs === before.minTenorSecs &&
    after.maxTenorSecs === before.maxTenorSecs;
  if (after.historyThresholdSecs !== threshold || !unchanged) {
    throw new Error('config on chain does not match the update');
  }
  log(`history threshold on chain: ${after.historyThresholdSecs} s`);
}

main().catch((error: unknown) => {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
});
