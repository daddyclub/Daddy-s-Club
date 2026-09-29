import { describe, expect, it } from 'vitest';
import { DEFAULT_RPC_URL, EnvError, parseEnv } from './env';

const DEVNET = 'https://api.devnet.solana.com';

describe('parseEnv', () => {
  it('falls back to localnet when nothing is set', () => {
    const env = parseEnv({});
    expect(env.rpcUrl).toBe(DEFAULT_RPC_URL);
    expect(env.cluster).toBe('localnet');
  });

  // A key that is absent, not merely empty, used to fail the whole object in
  // Zod 4 and silently send every value back to localnet.
  it('keeps the values that are set when another key is absent', () => {
    const env = parseEnv({ VITE_CLUSTER: 'devnet', VITE_RPC_URL: DEVNET });
    expect(env.rpcUrl).toBe(DEVNET);
    expect(env.cluster).toBe('devnet');
  });

  it('reads what Vite puts next to the variables', () => {
    const env = parseEnv({
      BASE_URL: '/Daddy-s-Club/',
      DEV: false,
      MODE: 'production',
      PROD: true,
      SSR: false,
      VITE_CLUSTER: 'devnet',
      VITE_RPC_URL: DEVNET,
    });
    expect(env.cluster).toBe('devnet');
    expect(env.rpcUrl).toBe(DEVNET);
  });

  // Unset repository variables reach the Pages build as empty strings.
  it('treats an empty value as unset', () => {
    const env = parseEnv({ VITE_CLUSTER: 'devnet', VITE_RPC_URL: DEVNET, VITE_PROGRAM_ID: '' });
    expect(env.cluster).toBe('devnet');
    expect(env.programId.toBase58()).toBe('7eT5T7mq1uD9piYJ2rMAzma8iYL7C7CZGPgxsB8DckoB');
  });

  it('names a malformed value instead of falling back', () => {
    expect(() => parseEnv({ VITE_CLUSTER: 'testnet' })).toThrow(EnvError);
    expect(() => parseEnv({ VITE_RPC_URL: 'not a url' })).toThrow(EnvError);
  });
});
