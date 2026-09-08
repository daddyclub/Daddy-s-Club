/**
 * Деривації PDA програми. Дзеркало seed-констант із
 * programs/daddys-club/src/state.rs.
 *
 * Seeds тут переписані байт у байт, а не імпортовані — між Rust і TS спільного
 * джерела немає. Тому розбіжність ловить тест: `pda.test.ts` читає `state.rs`
 * і звіряє кожен рядок. Перейменування seed'а в програмі має падати тут, а не
 * на першому виклику, який промахнувся повз акаунт.
 *
 * Кожна деривація віддає ще й `bump`. Програма зберігає його в акаунті саме
 * для того, щоб не платити ≈1500 CU за пошук (`SC-005`), і клієнт, який має
 * акаунт на руках, теж не мусить дерівати повторно.
 */

import { PublicKey } from '@solana/web3.js';

/** Program ID ядра — `declare_id!` у programs/daddys-club/src/lib.rs. */
export const PROGRAM_ID = new PublicKey('7eT5T7mq1uD9piYJ2rMAzma8iYL7C7CZGPgxsB8DckoB');

const utf8 = new TextEncoder();

/** `state.rs` → `CONFIG_SEED`. */
export const CONFIG_SEED = utf8.encode('config');
/** `state.rs` → `SOURCE_SEED`. */
export const SOURCE_SEED = utf8.encode('source');
/** `state.rs` → `ISSUE_SEED`. */
export const ISSUE_SEED = utf8.encode('issue');
/** `state.rs` → `HOLDER_SEED`. */
export const HOLDER_SEED = utf8.encode('holder');
/** `state.rs` → `OFFER_SEED`. */
export const OFFER_SEED = utf8.encode('offer');
/**
 * Список додаткових акаунтів гука. Seed належить не протоколу, а
 * `spl-transfer-hook-interface`: за ним Token-2022 шукає список сам. Тому в
 * `state.rs` його немає, і звіряється він із
 * `programs/daddys-club/src/instructions/issue.rs`.
 */
export const EXTRA_METAS_SEED = utf8.encode('extra-account-metas');

const U64_MAX = (1n << 64n) - 1n;

/** Адреса разом із канонічним bump — те саме, що віддає `find_program_address`. */
export interface Derived {
  readonly address: PublicKey;
  readonly bump: number;
}

/**
 * Лічильник у seeds — little-endian u64, як `seq.to_le_bytes()` у Rust.
 *
 * Вихід за u64 — це помилка виклику, а не інша адреса: `to_le_bytes()` у Rust
 * такого аргументу просто не прийме, і тихо дерівати щось інше тут не можна.
 */
function u64Seed(value: bigint): Uint8Array {
  if (value < 0n || value > U64_MAX) {
    throw new RangeError(`лічильник у seeds не вкладається в u64: ${value}`);
  }
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, value, true);
  return bytes;
}

function derive(seeds: Uint8Array[], programId: PublicKey): Derived {
  const [address, bump] = PublicKey.findProgramAddressSync(seeds, programId);
  return { address, bump };
}

/** Параметри протоколу. Singleton, seeds `["config"]` (`FR-036`). */
export function configPda(programId: PublicKey = PROGRAM_ID): Derived {
  return derive([CONFIG_SEED], programId);
}

/** Джерело revenue. Seeds `["source", issuer, seq]` (`FR-004`). */
export function sourcePda(issuer: PublicKey, seq: bigint, programId: PublicKey = PROGRAM_ID): Derived {
  return derive([SOURCE_SEED, issuer.toBytes(), u64Seed(seq)], programId);
}

/** Випуск. Seeds `["issue", source, seq]` (`FR-001`). */
export function issuePda(source: PublicKey, seq: bigint, programId: PublicKey = PROGRAM_ID): Derived {
  return derive([ISSUE_SEED, source.toBytes(), u64Seed(seq)], programId);
}

/**
 * Облік власника за випуском. Seeds `["holder", issue, owner]`.
 *
 * Адреса дерівається завжди, існування акаунта — ні: саме його відсутність і
 * означає гаманець без відкритого обліку, якому передача бонду відмовляється
 * (`FR-038`). Відрізнити одне від одного — робота читача, не деривації.
 */
export function holderPda(issue: PublicKey, owner: PublicKey, programId: PublicKey = PROGRAM_ID): Derived {
  return derive([HOLDER_SEED, issue.toBytes(), owner.toBytes()], programId);
}

/**
 * Список додаткових акаунтів гука. Seeds `["extra-account-metas", bondMint]`.
 *
 * Дерівається від мінта, а не від випуску: шукає його Token-2022, а в наборі
 * акаунтів переказу з нашого світу є лише мінт. Клієнту він потрібен, щоб
 * зібрати переказ бонду — акаунти в нього дописуються з цього списку
 * (`FR-017`).
 */
export function extraAccountMetasPda(bondMint: PublicKey, programId: PublicKey = PROGRAM_ID): Derived {
  return derive([EXTRA_METAS_SEED, bondMint.toBytes()], programId);
}

/** Оферта вторинного ринку. Seeds `["offer", issue, seller, nonce]` (`FR-024`). */
export function offerPda(
  issue: PublicKey,
  seller: PublicKey,
  nonce: bigint,
  programId: PublicKey = PROGRAM_ID,
): Derived {
  return derive([OFFER_SEED, issue.toBytes(), seller.toBytes(), u64Seed(nonce)], programId);
}
