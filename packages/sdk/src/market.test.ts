/**
 * Збирачі інструкцій звіряються з програмою, а не з уявленням про неї.
 *
 * Набір акаунтів переписаний із `CreateOffer`, `BuyOffer` і `CancelOffer` — тож
 * тест читає programs/daddys-market/src/instructions/market.rs і звіряє імена
 * полів, їхній порядок і прапорці: `mut` або `init` — рахунок, у який пишуть;
 * `Signer<` — підпис. Переставлене в програмі поле падає тут. Що сам порядок
 * у Rust сходиться з байткодом, доводять `create_offer_ix`, `buy_offer_ix` і
 * `cancel_offer_ix` у programs/daddys-club/tests/market.rs.
 *
 * Решта — підстановка: кожен слот має отримати саме ту адресу, яку перевірить
 * `has_one` або `seeds`, тому ключі у фікстурах розрізнені.
 */

import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { PublicKey, SystemProgram, type TransactionInstruction } from '@solana/web3.js';
import { describe, expect, it } from 'vitest';
import { discriminatorFilter, type Issue, type Offer, type ProtocolConfig } from './accounts.js';
import {
  type AccountSlot,
  type AccountsReader,
  BUY_OFFER_ACCOUNTS,
  buyOfferInstruction,
  CANCEL_OFFER_ACCOUNTS,
  CREATE_OFFER_ACCOUNTS,
  cancelOfferInstruction,
  createOfferInstruction,
  findFreeOfferNonce,
  INSTRUCTION_DISCRIMINATORS,
  type MarketInstruction,
  OFFER_ISSUE_OFFSET,
  offerFilters,
  sellerProceeds,
  tradingFee,
} from './market.js';
import {
  configPda,
  extraAccountMetasPda,
  holderPda,
  issuePda,
  MARKET_PROGRAM_ID,
  offerEscrowPda,
  offerPda,
  offerProceedsPda,
  PROGRAM_ID,
} from './pda.js';
import { TOKEN_2022_PROGRAM_ID } from './token.js';

const marketRs = readFileSync(
  new URL('../../../programs/daddys-market/src/instructions/market.rs', import.meta.url),
  'utf8',
);
const marketStateRs = readFileSync(
  new URL('../../../programs/daddys-market/src/state.rs', import.meta.url),
  'utf8',
);
const marketLibRs = readFileSync(
  new URL('../../../programs/daddys-market/src/lib.rs', import.meta.url),
  'utf8',
);

/**
 * Поля `#[derive(Accounts)] pub struct X<'info>` у порядку оголошення.
 * Атрибути поля — усе між попереднім полем і цим, без коментарів: `seeds`
 * містять `)]`, тож шукати кінець `#[account(...)]` дужками не можна.
 */
function rustAccounts(name: string): AccountSlot[] {
  const start = marketRs.indexOf(`pub struct ${name}<'info> {`);
  const end = marketRs.indexOf('\n}', start);
  if (start < 0 || end < 0) throw new Error(`struct ${name} у market.rs не знайдено`);

  const body = marketRs
    .slice(start, end)
    .split('\n')
    .filter((line) => !line.trim().startsWith('//'))
    .join('\n');

  const slots: AccountSlot[] = [];
  let previous = body.indexOf('{') + 1;
  for (const field of body.matchAll(/pub (\w+): ([^\n]+),/g)) {
    const [, fieldName, type] = field;
    if (fieldName === undefined || type === undefined) continue;
    const attributes = body.slice(previous, field.index);
    previous = field.index + field[0].length;
    slots.push({
      name: fieldName,
      writable: /\b(mut|init)\b/.test(attributes),
      signer: type.startsWith('Signer<'),
    });
  }
  return slots;
}

const key = (seed: number): PublicKey => new PublicKey(new Uint8Array(32).fill(seed));

const SOURCE = key(1);
const SELLER = key(2);
const BUYER = key(3);
const BOND_MINT = key(4);
const ESCROW_VAULT = key(5);
const USDC_MINT = key(6);
const FEE_VAULT = key(7);
const SELLER_BOND = key(8);
const SELLER_USDC = key(9);
const BUYER_USDC = key(10);
const BUYER_BOND = key(11);

const issue: Issue = {
  source: SOURCE,
  bondMint: BOND_MINT,
  escrowVault: ESCROW_VAULT,
  subscriptionVault: key(12),
  face: 250_000_000_000n,
  couponBps: 800,
  pledgeBps: 2_000,
  maturityTs: 1_900_000_000n,
  subscriptionEndTs: 1_800_000_000n,
  minLot: 1_000_000n,
  raised: 250_000_000_000n,
  obligationTotal: 270_000_000_000n,
  repaidTotal: 12_000_000_000n,
  payoutIndex: 48_000_000_000n,
  state: 'Repaying',
  seq: 3n,
  bump: 255,
};
const ISSUE = issuePda(SOURCE, 3n).address;

const config: ProtocolConfig = {
  admin: key(13),
  originationFeeBps: 100,
  tradingFeeBps: 50,
  maxPledgeBps: 5_000,
  minTenorSecs: 1n,
  maxTenorSecs: 2n,
  historyThresholdSecs: 3n,
  usdcMint: USDC_MINT,
  feeVault: FEE_VAULT,
  bump: 254,
};

const NONCE = 2n;
const OFFER = offerPda(ISSUE, SELLER, NONCE).address;
const ESCROW = offerEscrowPda(OFFER).address;

const offer: Offer = {
  seller: SELLER,
  issue: ISSUE,
  amount: 5_000_000_000n,
  price: 4_900_000_000n,
  tokenEscrow: ESCROW,
  nonce: NONCE,
  bump: 253,
};

/** Ключ за іменем слота — щоб підстановку читати іменами з Rust. */
function byName(
  ix: TransactionInstruction,
  layout: readonly AccountSlot[],
): Record<string, PublicKey | undefined> {
  return Object.fromEntries(layout.map((slot, i) => [slot.name, ix.keys[i]?.pubkey]));
}

function flagsOf(ix: TransactionInstruction): { writable: boolean; signer: boolean }[] {
  return ix.keys.map(({ isWritable, isSigner }) => ({ writable: isWritable, signer: isSigner }));
}

const u64le = (value: bigint): number[] => {
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, value, true);
  return [...bytes];
};

describe('дискримінатори інструкцій', () => {
  const NAMES: readonly MarketInstruction[] = ['create_offer', 'buy_offer', 'cancel_offer'];

  it.each(NAMES)('%s — перші 8 байт sha256("global:…")', (name) => {
    const sha = createHash('sha256').update(`global:${name}`).digest();
    expect([...INSTRUCTION_DISCRIMINATORS[name]]).toEqual([...sha.subarray(0, 8)]);
  });

  it.each(NAMES)('%s оголошена в #[program] ринку', (name) => {
    expect(marketLibRs).toContain(`pub fn ${name}(`);
  });
});

describe('набори акаунтів — ті самі, що в market.rs', () => {
  it.each([
    ['CreateOffer', CREATE_OFFER_ACCOUNTS],
    ['BuyOffer', BUY_OFFER_ACCOUNTS],
    ['CancelOffer', CANCEL_OFFER_ACCOUNTS],
  ] as const)('%s: імена, порядок, mut і підпис', (struct, layout) => {
    const rust = rustAccounts(struct);
    expect(rust.length).toBeGreaterThan(10);
    expect(layout).toEqual(rust);
  });

  it('скасування не бачить скарбниці комісій — її немає в наборі (FR-027)', () => {
    expect(CANCEL_OFFER_ACCOUNTS.map((slot) => slot.name)).not.toContain('fee_vault');
    expect(rustAccounts('CancelOffer').map((slot) => slot.name)).not.toContain('fee_vault');
  });
});

describe('createOfferInstruction', () => {
  const ix = createOfferInstruction({
    issue,
    seller: SELLER,
    sellerBond: SELLER_BOND,
    nonce: NONCE,
    amount: offer.amount,
    price: offer.price,
  });

  it('іде в програму ринку, а не в ядро', () => {
    expect(ix.programId.equals(MARKET_PROGRAM_ID)).toBe(true);
  });

  it('дані — дискримінатор, потім nonce, amount, price у порядку сигнатури', () => {
    expect(marketLibRs).toMatch(
      /ctx: Context<CreateOffer>,\s*nonce: u64,\s*amount: u64,\s*price: u64,/,
    );
    expect([...ix.data]).toEqual([
      ...INSTRUCTION_DISCRIMINATORS.create_offer,
      ...u64le(NONCE),
      ...u64le(offer.amount),
      ...u64le(offer.price),
    ]);
  });

  it('кожен слот отримує адресу, яку перевірить програма', () => {
    expect(flagsOf(ix)).toEqual(
      CREATE_OFFER_ACCOUNTS.map(({ writable, signer }) => ({ writable, signer })),
    );
    expect(byName(ix, CREATE_OFFER_ACCOUNTS)).toEqual({
      issue: ISSUE,
      seller: SELLER,
      seller_bond: SELLER_BOND,
      bond_mint: BOND_MINT,
      offer: OFFER,
      token_escrow: ESCROW,
      holder_seller: holderPda(ISSUE, SELLER).address,
      holder_escrow: holderPda(ISSUE, OFFER).address,
      extra_account_meta_list: extraAccountMetasPda(BOND_MINT).address,
      club_program: PROGRAM_ID,
      token_program: TOKEN_2022_PROGRAM_ID,
      system_program: SystemProgram.programId,
    });
  });

  it('облік сховища — від адреси оферти, тож той самий nonce веде в той самий облік', () => {
    const again = createOfferInstruction({
      issue,
      seller: SELLER,
      sellerBond: SELLER_BOND,
      nonce: NONCE,
      amount: 1n,
      price: 1n,
    });
    const other = createOfferInstruction({
      issue,
      seller: SELLER,
      sellerBond: SELLER_BOND,
      nonce: NONCE + 1n,
      amount: 1n,
      price: 1n,
    });
    const ledger = (i: TransactionInstruction) => byName(i, CREATE_OFFER_ACCOUNTS).holder_escrow;
    expect(ledger(again)).toEqual(ledger(ix));
    expect(ledger(other)).not.toEqual(ledger(ix));
  });

  it('аргумент поза u64 — помилка виклику, а не обрізані байти', () => {
    const base = {
      issue,
      seller: SELLER,
      sellerBond: SELLER_BOND,
      nonce: 0n,
      amount: 1n,
      price: 1n,
    };
    expect(() => createOfferInstruction({ ...base, amount: -1n })).toThrow(RangeError);
    expect(() => createOfferInstruction({ ...base, price: 1n << 64n })).toThrow(RangeError);
  });
});

describe('buyOfferInstruction', () => {
  const ix = buyOfferInstruction({
    config,
    issue,
    offer,
    buyer: BUYER,
    buyerUsdc: BUYER_USDC,
    buyerBond: BUYER_BOND,
    sellerUsdc: SELLER_USDC,
  });

  it('дані — лише дискримінатор', () => {
    expect([...ix.data]).toEqual([...INSTRUCTION_DISCRIMINATORS.buy_offer]);
  });

  it('скарбниця й валюта — з конфігу, сховища — з випуску й оферти', () => {
    expect(flagsOf(ix)).toEqual(
      BUY_OFFER_ACCOUNTS.map(({ writable, signer }) => ({ writable, signer })),
    );
    expect(byName(ix, BUY_OFFER_ACCOUNTS)).toEqual({
      config: configPda().address,
      issue: ISSUE,
      offer: OFFER,
      token_escrow: ESCROW,
      offer_proceeds: offerProceedsPda(OFFER).address,
      seller: SELLER,
      seller_usdc: SELLER_USDC,
      buyer: BUYER,
      buyer_usdc: BUYER_USDC,
      buyer_bond: BUYER_BOND,
      holder_buyer: holderPda(ISSUE, BUYER).address,
      holder_escrow: holderPda(ISSUE, OFFER).address,
      escrow_vault: ESCROW_VAULT,
      fee_vault: FEE_VAULT,
      bond_mint: BOND_MINT,
      usdc_mint: USDC_MINT,
      extra_account_meta_list: extraAccountMetasPda(BOND_MINT).address,
      club_program: PROGRAM_ID,
      token_program: TOKEN_2022_PROGRAM_ID,
      system_program: SystemProgram.programId,
    });
  });

  it('підписує лише покупець', () => {
    const signers = ix.keys.filter((meta) => meta.isSigner).map((meta) => meta.pubkey);
    expect(signers).toEqual([BUYER]);
  });

  it('випуск не від цієї оферти — відмова до мережі', () => {
    expect(() =>
      buyOfferInstruction({
        config,
        issue: { ...issue, seq: 4n },
        offer,
        buyer: BUYER,
        buyerUsdc: BUYER_USDC,
        buyerBond: BUYER_BOND,
        sellerUsdc: SELLER_USDC,
      }),
    ).toThrow(/належить випуску/);
  });
});

describe('cancelOfferInstruction', () => {
  const ix = cancelOfferInstruction({
    issue,
    offer,
    usdcMint: USDC_MINT,
    sellerBond: SELLER_BOND,
    sellerUsdc: SELLER_USDC,
  });

  it('дані — лише дискримінатор', () => {
    expect([...ix.data]).toEqual([...INSTRUCTION_DISCRIMINATORS.cancel_offer]);
  });

  it('лот і накопичене — на рахунки продавця, підписує він сам', () => {
    expect(flagsOf(ix)).toEqual(
      CANCEL_OFFER_ACCOUNTS.map(({ writable, signer }) => ({ writable, signer })),
    );
    expect(byName(ix, CANCEL_OFFER_ACCOUNTS)).toEqual({
      issue: ISSUE,
      offer: OFFER,
      token_escrow: ESCROW,
      offer_proceeds: offerProceedsPda(OFFER).address,
      seller: SELLER,
      seller_bond: SELLER_BOND,
      seller_usdc: SELLER_USDC,
      holder_seller: holderPda(ISSUE, SELLER).address,
      holder_escrow: holderPda(ISSUE, OFFER).address,
      escrow_vault: ESCROW_VAULT,
      bond_mint: BOND_MINT,
      usdc_mint: USDC_MINT,
      extra_account_meta_list: extraAccountMetasPda(BOND_MINT).address,
      club_program: PROGRAM_ID,
      token_program: TOKEN_2022_PROGRAM_ID,
      system_program: SystemProgram.programId,
    });
    expect(ix.keys.filter((meta) => meta.isSigner).map((meta) => meta.pubkey)).toEqual([SELLER]);
  });
});

describe('findFreeOfferNonce — найменший вільний слот', () => {
  const taken = { owner: MARKET_PROGRAM_ID, data: new Uint8Array(129) };

  /** Мережа, де зайняті рівно ці `nonce`, і лічильник ходок. */
  function chain(occupied: ReadonlyMap<bigint, unknown>) {
    const byAddress = new Map(
      [...occupied].map(([nonce, info]) => [
        offerPda(ISSUE, SELLER, nonce).address.toBase58(),
        info,
      ]),
    );
    const calls: number[] = [];
    const reader: AccountsReader = {
      getMultipleAccountsInfo: async (keys) => {
        calls.push(keys.length);
        return keys.map((k) => byAddress.get(k.toBase58()) ?? null);
      },
    };
    return { reader, calls };
  }

  it('порожній продавець — нуль', async () => {
    expect(await findFreeOfferNonce(chain(new Map()).reader, ISSUE, SELLER)).toBe(0n);
  });

  it('дірка між зайнятими — береться дірка, а не наступний за найбільшим', async () => {
    const { reader } = chain(
      new Map([
        [0n, taken],
        [1n, taken],
        [3n, taken],
      ]),
    );
    expect(await findFreeOfferNonce(reader, ISSUE, SELLER)).toBe(2n);
  });

  it('зайнята ціла сторінка — питає наступну', async () => {
    const occupied = new Map(Array.from({ length: 11 }, (_, i) => [BigInt(i), taken] as const));
    const { reader, calls } = chain(occupied);
    expect(await findFreeOfferNonce(reader, ISSUE, SELLER)).toBe(11n);
    expect(calls.length).toBe(2);
  });

  it('лампорти на порожній системній адресі слот не займають — init їх приймає', async () => {
    const dusted = { owner: SystemProgram.programId, data: new Uint8Array(0) };
    const { reader } = chain(new Map([[0n, dusted]]));
    expect(await findFreeOfferNonce(reader, ISSUE, SELLER)).toBe(0n);
  });

  it('відповідь не того вигляду — відмова, а не «вільно»', async () => {
    const short: AccountsReader = { getMultipleAccountsInfo: async () => [null] };
    const junk: AccountsReader = {
      getMultipleAccountsInfo: async (keys) => keys.map(() => ({ owner: 'x', data: [] })),
    };
    await expect(findFreeOfferNonce(short, ISSUE, SELLER)).rejects.toThrow();
    await expect(findFreeOfferNonce(junk, ISSUE, SELLER)).rejects.toThrow();
  });
});

describe('tradingFee — дзеркало trading_fee у daddys-market', () => {
  it('формула в програмі — та сама: price × bps / BPS_DENOM', () => {
    expect(marketRs).toMatch(
      /fn trading_fee\(price: u128, fee_bps: u16\)[\s\S]*?checked_mul\(u128::from\(fee_bps\)\)[\s\S]*?checked_div\(math::BPS_DENOM\)/,
    );
  });

  it('округлення вниз: залишок лишається продавцеві', () => {
    expect(tradingFee(4_900_000_000n, 50)).toBe(24_500_000n);
    expect(tradingFee(199n, 50)).toBe(0n);
    expect(tradingFee(200n, 50)).toBe(1n);
    expect(sellerProceeds(4_900_000_000n, 50)).toBe(4_875_500_000n);
  });

  it('поза u64 чи u16 — null, а не число', () => {
    expect(tradingFee(1n << 64n, 50)).toBeNull();
    expect(tradingFee(-1n, 50)).toBeNull();
    expect(tradingFee(1n, 70_000)).toBeNull();
    expect(sellerProceeds(1n << 64n, 50)).toBeNull();
  });
});

describe('offerFilters — стакан випуску без індексатора', () => {
  it('issue лежить одразу після seller, як оголошено в state.rs', () => {
    const offerStruct = marketStateRs.slice(marketStateRs.indexOf('pub struct Offer {'));
    const fields = [...offerStruct.matchAll(/pub (\w+): (\w+),/g)].map((m) => `${m[1]}: ${m[2]}`);
    expect(fields.slice(0, 2)).toEqual(['seller: Pubkey', 'issue: Pubkey']);
    expect(OFFER_ISSUE_OFFSET).toBe(8 + 32);
  });

  it('тип за дискримінатором, випуск за полем', () => {
    const [byKind, byIssue] = offerFilters(ISSUE);
    expect(byKind).toEqual(discriminatorFilter('Offer'));
    expect(byIssue).toEqual({ memcmp: { offset: OFFER_ISSUE_OFFSET, bytes: ISSUE.toBase58() } });
  });
});
