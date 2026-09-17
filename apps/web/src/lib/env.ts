/**
 * Межа з конфігурацією збірки: усе, що Vite вкладає в `import.meta.env`.
 *
 * `import.meta.env` типізований індексною сигнатурою `any` — тобто рівно тим,
 * чого `CLAUDE.md` не дозволяє пускати далі. Тому значення читаються як
 * `unknown` і проходять Zod: хибний `VITE_RPC_URL` має бути названою відмовою
 * на старті, а не запитом, який відмовить вузол.
 *
 * Значення за замовчуванням — ті самі, що в `.env.example`: локальний вузол і
 * program ID з `Anchor.toml`. Це не приховує помилку конфігурації, а описує
 * єдиний кластер, де M1 узагалі має сенс, — localnet.
 */

import { PROGRAM_ID } from '@daddys-club/sdk';
import { PublicKey } from '@solana/web3.js';
import { z } from 'zod';

/** Кластери, для яких зібраний застосунок. Той самий перелік, що в `.env.example`. */
export const CLUSTERS = ['localnet', 'devnet', 'mainnet-beta'] as const;

export type Cluster = (typeof CLUSTERS)[number];

/** Вузол за замовчуванням — той, що піднімає `solana-test-validator`. */
export const DEFAULT_RPC_URL = 'http://127.0.0.1:8899';

/**
 * Порожній рядок дорівнює відсутності: саме так виглядає незаповнений ключ у
 * `.env.example`, і вважати його адресою вузла означало б піти в мережу за `''`.
 */
const optionalText = z
  .unknown()
  .transform((value) => (typeof value === 'string' && value !== '' ? value : undefined));

const schema = z.object({
  VITE_RPC_URL: optionalText,
  VITE_CLUSTER: optionalText,
  VITE_PROGRAM_ID: optionalText,
});

export interface WebEnv {
  readonly rpcUrl: string;
  readonly cluster: Cluster;
  readonly programId: PublicKey;
}

/** Помилка конфігурації збірки. Названа, бо лагодиться вона в `.env`, а не в коді. */
export class EnvError extends Error {
  readonly key: string;

  constructor(key: string, detail: string) {
    super(`${key}: ${detail}`);
    this.name = 'EnvError';
    this.key = key;
  }
}

function requireUrl(key: string, value: string | undefined, fallback: string): string {
  if (value === undefined) return fallback;
  const parsed = z.url().safeParse(value);
  if (!parsed.success) throw new EnvError(key, `не URL: ${value}`);
  return parsed.data;
}

function requireCluster(key: string, value: string | undefined): Cluster {
  if (value === undefined) return 'localnet';
  const parsed = z.enum(CLUSTERS).safeParse(value);
  if (!parsed.success) throw new EnvError(key, `не з переліку кластерів: ${value}`);
  return parsed.data;
}

function requirePublicKey(key: string, value: string | undefined, fallback: PublicKey): PublicKey {
  if (value === undefined) return fallback;
  try {
    return new PublicKey(value);
  } catch {
    throw new EnvError(key, `не адреса Solana: ${value}`);
  }
}

/** Розбирає конфігурацію збірки. Виділена з `webEnv`, щоб її можна було ганяти тестом. */
export function parseEnv(raw: unknown): WebEnv {
  const source = schema.safeParse(raw);
  const values = source.success
    ? source.data
    : { VITE_RPC_URL: undefined, VITE_CLUSTER: undefined, VITE_PROGRAM_ID: undefined };

  return {
    rpcUrl: requireUrl('VITE_RPC_URL', values.VITE_RPC_URL, DEFAULT_RPC_URL),
    cluster: requireCluster('VITE_CLUSTER', values.VITE_CLUSTER),
    programId: requirePublicKey('VITE_PROGRAM_ID', values.VITE_PROGRAM_ID, PROGRAM_ID),
  };
}

let cached: WebEnv | null = null;

/** Конфігурація цієї збірки. Розбирається один раз і не змінюється. */
export function webEnv(): WebEnv {
  cached ??= parseEnv(import.meta.env);
  return cached;
}
