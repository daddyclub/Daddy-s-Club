/**
 * Читання стану з вузла — дзеркало `encode.ts`: там байти складаються, тут
 * розбираються.
 *
 * Усе, що прийшло з RPC, лишається `unknown` доти, доки його не пропустив Zod
 * (`CLAUDE.md`). Декодери акаунтів протоколу беруться з SDK: переписувати
 * розкладку `state.rs` удруге немає за що, а розбіжність між Rust і TS там уже
 * стереже тест. Своє тут рівно одне — баланс токен-акаунта, якого SDK не знає
 * і знати не мусить: це акаунт Token-2022, а не наш.
 *
 * Відсутність акаунта тут — **названа відмова**, а не нуль. Те саме правило, що
 * `FR-037` ставить публічному читачеві, і та сама причина, з якої картка випуску
 * не малює нуль на місці невідомого: у демо, яке рахує гроші, «рахунка немає» і
 * «на рахунку нуль» — різні новини.
 */

import type { Connection, PublicKey } from '@solana/web3.js';
import { z } from 'zod';
import type { HolderCheckpoint, Issue } from '../../../packages/sdk/src/accounts.ts';
import { decodeHolderCheckpoint, decodeIssue } from '../../../packages/sdk/src/accounts.ts';
import { TOKEN_2022 } from './encode.ts';

/** Чому за адресою не виявилось того, що очікували. */
export class ChainReadError extends Error {
  readonly address: string;

  constructor(address: PublicKey, detail: string) {
    super(`${address.toBase58()}: ${detail}`);
    this.name = 'ChainReadError';
    this.address = address.toBase58();
  }
}

/** Межа з мережею. Далі — самі байти, і про них уже все відомо. */
const rawData = z.instanceof(Uint8Array);

/**
 * Розкладка токен-акаунта Token-2022: `mint(32) owner(32) amount(u64)`.
 *
 * Довжина звіряється **не на рівність**, і це навмисно: рахунок бонду несе
 * розширення `TransferHookAccount` і тому довший за базові 165 байтів. Спільне
 * в обох — початок, і саме він тут читається.
 */
const TOKEN_ACCOUNT_MIN_LEN = 165;
const AMOUNT_OFFSET = 64;

async function fetchData(connection: Connection, address: PublicKey): Promise<unknown> {
  const account = await connection.getAccountInfo(address, 'confirmed');
  if (account === null) throw new ChainReadError(address, 'акаунта за адресою немає');
  return account.data;
}

/** Баланс токен-акаунта в найменших одиницях. */
export async function readTokenAmount(connection: Connection, address: PublicKey): Promise<bigint> {
  const account = await connection.getAccountInfo(address, 'confirmed');
  if (account === null) throw new ChainReadError(address, 'токен-акаунта за адресою немає');
  if (!account.owner.equals(TOKEN_2022)) {
    throw new ChainReadError(address, `власник ${account.owner.toBase58()}, а не Token-2022`);
  }

  const parsed = rawData.safeParse(account.data);
  if (!parsed.success) throw new ChainReadError(address, 'дані акаунта — не масив байтів');
  const bytes = parsed.data;
  if (bytes.length < TOKEN_ACCOUNT_MIN_LEN) {
    throw new ChainReadError(address, `${bytes.length} байтів — коротше за токен-акаунт`);
  }

  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  return view.getBigUint64(AMOUNT_OFFSET, true);
}

/** Випуск за адресою. Декодер — той самий, що читає картка (`FR-023`). */
export async function readIssue(connection: Connection, address: PublicKey): Promise<Issue> {
  return decodeIssue(await fetchData(connection, address));
}

/** Облік власника за випуском (`FR-016`). */
export async function readHolder(
  connection: Connection,
  address: PublicKey,
): Promise<HolderCheckpoint> {
  return decodeHolderCheckpoint(await fetchData(connection, address));
}
