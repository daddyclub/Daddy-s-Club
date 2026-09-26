/**
 * ATA — деривація і інструкція створення. Справжній доказ адреси — живий
 * вузол: програма ATA відмовить `CreateIdempotent` із чужою адресою, і саме цим
 * шляхом скрипти заміру створюють рахунки з M1. Тут — те, що ламається тихо:
 * програма в seeds, порядок акаунтів і байт інструкції.
 */

import { PublicKey, SystemProgram } from '@solana/web3.js';
import { describe, expect, it } from 'vitest';
import {
  ATA_PROGRAM_ID,
  associatedTokenAddress,
  createAssociatedTokenAccountIdempotent,
  decodeTokenAccount,
  TOKEN_2022_PROGRAM_ID,
} from './token.js';

const key = (seed: number): PublicKey => new PublicKey(new Uint8Array(32).fill(seed));
const OWNER = key(1);
const MINT = key(2);
const PAYER = key(3);

describe('associatedTokenAddress', () => {
  it('seeds — власник, Token-2022, мінт; програма — ATA', () => {
    const [expected] = PublicKey.findProgramAddressSync(
      [OWNER.toBytes(), TOKEN_2022_PROGRAM_ID.toBytes(), MINT.toBytes()],
      ATA_PROGRAM_ID,
    );
    expect(associatedTokenAddress(OWNER, MINT).equals(expected)).toBe(true);
  });

  it('класичний SPL Token дав би іншу адресу — програма в seeds не декоративна', () => {
    const classic = new PublicKey('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA');
    const [other] = PublicKey.findProgramAddressSync(
      [OWNER.toBytes(), classic.toBytes(), MINT.toBytes()],
      ATA_PROGRAM_ID,
    );
    expect(associatedTokenAddress(OWNER, MINT).equals(other)).toBe(false);
  });

  it('пін адреси: переставлені seeds падають тут', () => {
    expect(associatedTokenAddress(OWNER, MINT).toBase58()).toMatchInlineSnapshot(
      `"DyaUQ3JTcmWApDibKtBvxLBhUPjvA4KEM99t45qz3bfh"`,
    );
  });
});

describe('createAssociatedTokenAccountIdempotent', () => {
  const { address, instruction } = createAssociatedTokenAccountIdempotent(PAYER, OWNER, MINT);

  it('байт 1 — CreateIdempotent, а не Create (0), який падає на наявному рахунку', () => {
    expect([...instruction.data]).toEqual([1]);
    expect(instruction.programId.equals(ATA_PROGRAM_ID)).toBe(true);
  });

  it('акаунти в порядку програми ATA, платить і підписує лише платник', () => {
    expect(
      instruction.keys.map(({ pubkey, isSigner, isWritable }) => [
        pubkey.toBase58(),
        isSigner,
        isWritable,
      ]),
    ).toEqual([
      [PAYER.toBase58(), true, true],
      [address.toBase58(), false, true],
      [OWNER.toBase58(), false, false],
      [MINT.toBase58(), false, false],
      [SystemProgram.programId.toBase58(), false, false],
      [TOKEN_2022_PROGRAM_ID.toBase58(), false, false],
    ]);
  });
});

describe('decodeTokenAccount', () => {
  it('mint, owner, amount — з перших 72 байтів; розширення після 165 не заважають', () => {
    const data = new Uint8Array(182);
    data.set(MINT.toBytes(), 0);
    data.set(OWNER.toBytes(), 32);
    new DataView(data.buffer).setBigUint64(64, 4_875_500_000n, true);
    const view = decodeTokenAccount(data);
    expect(view.mint.equals(MINT)).toBe(true);
    expect(view.owner.equals(OWNER)).toBe(true);
    expect(view.amount).toBe(4_875_500_000n);
  });

  it('коротші дані й не байти — відмова, а не нуль', () => {
    expect(() => decodeTokenAccount(new Uint8Array(164))).toThrow(RangeError);
    expect(() => decodeTokenAccount(null)).toThrow(RangeError);
  });
});
