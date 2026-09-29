/**
 * Which cluster a script is talking to, and the few things that change with it.
 *
 * The cluster is read from the genesis hash, not guessed from the URL: a
 * provider endpoint (Helius and the like) says nothing reliable about the
 * cluster behind it, and a wrong guess here means minting test USDC with the
 * wrong key or trying to airdrop on a cluster that has no faucet.
 */

import { existsSync, readFileSync } from 'node:fs';
import { type Connection, Keypair } from '@solana/web3.js';
import { z } from 'zod';

export type Cluster = 'localnet' | 'devnet' | 'mainnet-beta';

const GENESIS: Readonly<Record<string, Cluster>> = {
  EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG: 'devnet',
  '5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d': 'mainnet-beta',
};

/** Any genesis we do not recognise is a local validator. */
export async function detectCluster(connection: Connection): Promise<Cluster> {
  return GENESIS[await connection.getGenesisHash()] ?? 'localnet';
}

/**
 * Report file for a run. Localnet keeps the name the committed reports already
 * have; any other cluster gets a suffix, so a devnet run never overwrites the
 * localnet evidence it is compared against.
 */
export function reportName(base: string, cluster: Cluster): string {
  return cluster === 'localnet' ? `${base}.json` : `${base}.${cluster}.json`;
}

/**
 * An RPC URL fit for a log line or a committed report: origin only. Provider
 * keys travel either in the query (`?api-key=`) or in the path (`/v2/<key>`),
 * so dropping just the query is not enough.
 */
export function redactUrl(url: string): string {
  try {
    const parsed = new URL(url);
    const hidden = parsed.pathname !== '/' || parsed.search !== '';
    return hidden ? `${parsed.origin}/…` : parsed.origin;
  } catch {
    return '(unparseable URL)';
  }
}

const secretKey = z.array(z.number().int().min(0).max(255)).length(64);

/** A `solana-keygen` file. Missing or malformed is a named failure, not a fresh key. */
export function readKeypair(path: string): Keypair {
  if (!existsSync(path)) throw new Error(`${path}: no such key file`);
  const parsed = secretKey.safeParse(JSON.parse(readFileSync(path, 'utf8')) as unknown);
  if (!parsed.success) throw new Error(`${path}: not a 64-byte solana-keygen key`);
  return Keypair.fromSecretKey(Uint8Array.from(parsed.data));
}
