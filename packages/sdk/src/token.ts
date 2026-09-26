/**
 * Асоційовані токен-рахунки. Бонд і USDC протоколу обидва живуть у Token-2022,
 * тому програма тут одна — і вона входить у seeds ATA: той самий власник і мінт
 * під класичним SPL Token дали б іншу адресу.
 *
 * Переписано з `spl-associated-token-account`, а не підключено: заради однієї
 * деривації й однієї інструкції тягнути `@solana/spl-token` у браузер немає за
 * що. Скрипти заміру ходили цими ж функціями на живому вузлі з M1 — звідти вони
 * сюди й переїхали, щоб веб і скрипти не мали двох копій.
 */

import { PublicKey, SystemProgram, TransactionInstruction } from '@solana/web3.js';

/**
 * Token-2022. Бонд і USDC протоколу обидва живуть у ньому, тож `token_program`
 * у наборах ринку й ядра один.
 *
 * Цей файл навмисно без імпортів із решти SDK: скрипти заміру вантажать його
 * під Node напряму, а Node не переписує `./x.js` на `./x.ts`.
 */
export const TOKEN_2022_PROGRAM_ID = new PublicKey('TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb');

/** `spl-associated-token-account`. */
export const ATA_PROGRAM_ID = new PublicKey('ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL');

/** Байт інструкції `CreateIdempotent` програми ATA. */
const CREATE_IDEMPOTENT = 1;

/** Адреса ATA власника для мінта Token-2022. Seeds `[owner, token_program, mint]`. */
export function associatedTokenAddress(owner: PublicKey, mint: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(
    [owner.toBytes(), TOKEN_2022_PROGRAM_ID.toBytes(), mint.toBytes()],
    ATA_PROGRAM_ID,
  )[0];
}

/**
 * Створює ATA, якщо його ще немає, і нічого не робить, якщо є. Саме
 * ідемпотентність дозволяє класти її в транзакцію «про всяк випадок»: платник
 * вносить оренду лише тоді, коли рахунку справді не було.
 *
 * Рахунки бонду створюються тільки так: мінт випуску несе `TransferHook`, тож
 * його рахунки мусять мати розширення `TransferHookAccount`, і розмір під нього
 * рахує сама ATA-програма. Рахунок, зроблений «на 165 байтів», Token-2022 при
 * першому ж переказі відхилив би.
 */
export function createAssociatedTokenAccountIdempotent(
  payer: PublicKey,
  owner: PublicKey,
  mint: PublicKey,
): { address: PublicKey; instruction: TransactionInstruction } {
  const address = associatedTokenAddress(owner, mint);
  return {
    address,
    instruction: new TransactionInstruction({
      programId: ATA_PROGRAM_ID,
      keys: [
        { pubkey: payer, isSigner: true, isWritable: true },
        { pubkey: address, isSigner: false, isWritable: true },
        { pubkey: owner, isSigner: false, isWritable: false },
        { pubkey: mint, isSigner: false, isWritable: false },
        { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
        { pubkey: TOKEN_2022_PROGRAM_ID, isSigner: false, isWritable: false },
      ],
      data: Buffer.from([CREATE_IDEMPOTENT]),
    }),
  };
}

/** Базова частина рахунку SPL/Token-2022; розширення лежать після неї. */
export const TOKEN_ACCOUNT_BASE_LEN = 165;

/** Токен-рахунок так, як його читають екран і замір: чий, якого мінта, скільки. */
export interface TokenAccountView {
  readonly mint: PublicKey;
  readonly owner: PublicKey;
  readonly amount: bigint;
}

/**
 * Читає голову токен-рахунку: `mint`, `owner`, `amount` — перші 72 байти
 * розкладки, однакові для SPL Token і Token-2022. Що рахунок належить
 * Token-2022, перевіряє викликач за власником акаунта: байти цього не кажуть.
 * Коротші за 165 байтів дані — не токен-рахунок, і це названа відмова.
 */
export function decodeTokenAccount(data: unknown): TokenAccountView {
  if (!(data instanceof Uint8Array) || data.length < TOKEN_ACCOUNT_BASE_LEN) {
    throw new RangeError('не токен-рахунок: даних менше за 165 байтів');
  }
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  return {
    mint: new PublicKey(data.subarray(0, 32)),
    owner: new PublicKey(data.subarray(32, 64)),
    amount: view.getBigUint64(64, true),
  };
}
