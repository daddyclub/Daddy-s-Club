/**
 * Кодування інструкцій руками: Anchor і Token-2022.
 *
 * IDL-клієнта тут немає навмисно, і причина та сама, з якої `packages/sdk`
 * переписує розкладку акаунтів замість того, щоб її генерувати: нова гілка
 * графа залежностей вимагає причини (`CLAUDE.md`). Кодувальників усього сім, і
 * кожен — це рівно ті байти, які читає програма.
 *
 * Що ці байти справді ті самі, доводить не тест, а сам вузол: інструкція з
 * хибним дискримінатором або зсунутим аргументом не проходить взагалі. Замір
 * `SC-002` починається з того, що весь ланцюжок M1 пройшов на живому вузлі.
 */

import { createHash } from 'node:crypto';
import {
  type AccountMeta,
  PublicKey,
  SystemProgram,
  TransactionInstruction,
} from '@solana/web3.js';

/** Program ID ядра — `declare_id!` у `programs/daddys-club/src/lib.rs`. */
export const CLUB_PROGRAM = new PublicKey('7eT5T7mq1uD9piYJ2rMAzma8iYL7C7CZGPgxsB8DckoB');
/** Program ID референсного емітента — `programs/demo-issuer/src/lib.rs`. */
export const DEMO_PROGRAM = new PublicKey('8wKjGiLvnMTv7oi9PcztmbRv4v63emT2qPPrA8x1fW3z');
export const TOKEN_2022 = new PublicKey('TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb');
export const ATA_PROGRAM = new PublicKey('ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL');
export const SYSTEM_PROGRAM = SystemProgram.programId;

/** Дискримінатор інструкції Anchor — 8 байт `sha256("global:" + ім'я)`. */
export function instructionDiscriminator(name: string): Buffer {
  return createHash('sha256').update(`global:${name}`).digest().subarray(0, 8);
}

/** Складач borsh-аргументів. Порядок викликів і є порядок полів у Rust. */
export class Args {
  private readonly parts: Buffer[] = [];

  static forInstruction(name: string): Args {
    const args = new Args();
    args.parts.push(instructionDiscriminator(name));
    return args;
  }

  u8(value: number): this {
    this.parts.push(Buffer.from([value]));
    return this;
  }

  u16(value: number): this {
    const buffer = Buffer.alloc(2);
    buffer.writeUInt16LE(value);
    this.parts.push(buffer);
    return this;
  }

  u64(value: bigint): this {
    const buffer = Buffer.alloc(8);
    buffer.writeBigUInt64LE(value);
    this.parts.push(buffer);
    return this;
  }

  i64(value: bigint): this {
    const buffer = Buffer.alloc(8);
    buffer.writeBigInt64LE(value);
    this.parts.push(buffer);
    return this;
  }

  pubkey(value: PublicKey): this {
    this.parts.push(Buffer.from(value.toBytes()));
    return this;
  }

  build(): Buffer {
    return Buffer.concat(this.parts);
  }
}

/** Мета акаунта. Три літери замість чотирьох полів — набори тут довгі. */
export const ro = (pubkey: PublicKey): AccountMeta => ({
  pubkey,
  isSigner: false,
  isWritable: false,
});
export const rw = (pubkey: PublicKey): AccountMeta => ({
  pubkey,
  isSigner: false,
  isWritable: true,
});
export const signer = (pubkey: PublicKey): AccountMeta => ({
  pubkey,
  isSigner: true,
  isWritable: false,
});
export const signerRw = (pubkey: PublicKey): AccountMeta => ({
  pubkey,
  isSigner: true,
  isWritable: true,
});

export function instruction(
  programId: PublicKey,
  keys: AccountMeta[],
  data: Buffer,
): TransactionInstruction {
  return new TransactionInstruction({ programId, keys, data });
}

// ── Token-2022 ────────────────────────────────────────────────────────────────
//
// Індекси інструкцій — ті самі, що в SPL Token: Token-2022 успадкував базовий
// набір і дописав розширення далі.

const INITIALIZE_MINT_2 = 20;
const INITIALIZE_ACCOUNT_3 = 18;
const MINT_TO = 7;

/** Розмір мінта без розширень і токен-акаунта без розширень. */
export const MINT_SIZE = 82;
export const TOKEN_ACCOUNT_SIZE = 165;

export function initializeMint2(
  mint: PublicKey,
  decimals: number,
  mintAuthority: PublicKey,
): TransactionInstruction {
  const data = new Args()
    .u8(INITIALIZE_MINT_2)
    .u8(decimals)
    .pubkey(mintAuthority)
    .u8(0) // freeze authority: None
    .build();
  return instruction(TOKEN_2022, [rw(mint)], data);
}

export function initializeAccount3(
  account: PublicKey,
  mint: PublicKey,
  owner: PublicKey,
): TransactionInstruction {
  const data = new Args().u8(INITIALIZE_ACCOUNT_3).pubkey(owner).build();
  return instruction(TOKEN_2022, [rw(account), ro(mint)], data);
}

export function mintTo(
  mint: PublicKey,
  destination: PublicKey,
  authority: PublicKey,
  amount: bigint,
): TransactionInstruction {
  const data = new Args().u8(MINT_TO).u64(amount).build();
  return instruction(TOKEN_2022, [rw(mint), rw(destination), signer(authority)], data);
}

/** Адреса асоційованого рахунка. Власник може бути PDA — це не заборонено. */
export function associatedTokenAddress(owner: PublicKey, mint: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(
    [owner.toBytes(), TOKEN_2022.toBytes(), mint.toBytes()],
    ATA_PROGRAM,
  )[0];
}

/**
 * Створення асоційованого рахунка, ідемпотентне.
 *
 * Саме ним створюються рахунки бонду: мінт випуску несе `TransferHook`, тому
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
    instruction: instruction(
      ATA_PROGRAM,
      [signerRw(payer), rw(address), ro(owner), ro(mint), ro(SYSTEM_PROGRAM), ro(TOKEN_2022)],
      Buffer.from([1]),
    ),
  };
}
