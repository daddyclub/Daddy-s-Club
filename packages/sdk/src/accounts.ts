/**
 * Декодери акаунтів протоколу. Дзеркало programs/daddys-club/src/state.rs.
 *
 * Anchor кладе в акаунт 8 байт дискримінатора і далі borsh у порядку
 * оголошення полів. Порядок і розміри тут переписані з `state.rs`, спільного
 * джерела між Rust і TS немає — тому `accounts.test.ts` читає сам `state.rs`
 * і звіряє з ним і склад полів, і їхній порядок, і розміри.
 *
 * Байти з мережі приходять як `unknown` і проходять Zod, перш ніж їх хтось
 * прочитає: акаунт за адресою може виявитись чужим, коротшим або не тим
 * типом, і кожен із цих випадків має бути названою відмовою, а не полем,
 * прочитаним із сусіднього.
 *
 * Числа: усе, що в Rust `u64`, `i64`, `u128`, — тут `bigint`; `u16` і `u8`
 * лишаються `number`. Те саме правило, що й у math.ts, і з тієї ж причини:
 * `number` не тримає ані u64, ані масштаб 1e12.
 */

import { PublicKey } from '@solana/web3.js';
import { z } from 'zod';

/** Акаунти, які вміє читати цей модуль. Імена — типи зі `state.rs`. */
export type AccountKind =
  | 'ProtocolConfig'
  | 'RevenueSource'
  | 'Issue'
  | 'HolderCheckpoint'
  | 'Offer';

/** Дискримінатор Anchor — 8 байт `sha256("account:" + назва типу)`. */
export const DISCRIMINATOR_LEN = 8;

/**
 * Дискримінатори прибиті числами, а не рахуються на льоту: sha256 у браузері
 * лише асинхронний, а тягнути крипто-залежність заради п'яти сталих значень
 * немає за що. Що вони справді ті самі, доводить тест — він рахує sha256 і
 * звіряє.
 */
export const DISCRIMINATORS: Readonly<Record<AccountKind, Uint8Array>> = {
  ProtocolConfig: new Uint8Array([207, 91, 250, 28, 152, 179, 215, 209]),
  RevenueSource: new Uint8Array([177, 81, 146, 115, 113, 186, 73, 207]),
  Issue: new Uint8Array([171, 193, 204, 62, 63, 166, 106, 255]),
  HolderCheckpoint: new Uint8Array([194, 171, 216, 245, 194, 59, 113, 27]),
  Offer: new Uint8Array([215, 88, 60, 71, 170, 162, 73, 229]),
};

/**
 * `INIT_SPACE` кожного акаунта — без дискримінатора. Ті самі числа, що прибиті
 * тестом `account_sizes_are_pinned` у `state.rs`: під них рахується
 * rent-exempt, і поле, додане мимохідь, має читатись як змінена цифра.
 */
export const ACCOUNT_SPACE: Readonly<Record<AccountKind, number>> = {
  ProtocolConfig: 127,
  RevenueSource: 162,
  Issue: 214,
  HolderCheckpoint: 97,
  Offer: 121,
};

/**
 * Життєвий цикл випуску. Порядок — байт у стані на ланцюгу, той самий, що
 * прибитий у `state.rs` тестом `issue_state_bytes_are_pinned`.
 */
export const ISSUE_STATES = [
  'Subscribing',
  'Funded',
  'Repaying',
  'PastDue',
  'Repaid',
  'Failed',
] as const;

export type IssueState = (typeof ISSUE_STATES)[number];

/** Параметри протоколу (`FR-036`). */
export interface ProtocolConfig {
  readonly admin: PublicKey;
  readonly originationFeeBps: number;
  readonly tradingFeeBps: number;
  readonly maxPledgeBps: number;
  readonly minTenorSecs: bigint;
  readonly maxTenorSecs: bigint;
  readonly historyThresholdSecs: bigint;
  readonly usdcMint: PublicKey;
  readonly feeVault: PublicKey;
  readonly bump: number;
}

/** Джерело revenue (`FR-004`, `FR-028`). */
export interface RevenueSource {
  readonly issuer: PublicKey;
  readonly authority: PublicKey;
  readonly vault: PublicKey;
  readonly firstSeenTs: bigint;
  readonly totalObserved: bigint;
  readonly observedBeforeIssue: bigint;
  /** `None` у Rust — це `null`, а не нульовий ключ: вільне джерело (`FR-006`). */
  readonly activeIssue: PublicKey | null;
  readonly seq: bigint;
  readonly bump: number;
}

/** Випуск (`FR-001`, `FR-002`). */
export interface Issue {
  readonly source: PublicKey;
  readonly bondMint: PublicKey;
  readonly escrowVault: PublicKey;
  readonly subscriptionVault: PublicKey;
  readonly face: bigint;
  readonly couponBps: number;
  readonly pledgeBps: number;
  readonly maturityTs: bigint;
  readonly subscriptionEndTs: bigint;
  readonly minLot: bigint;
  readonly raised: bigint;
  readonly obligationTotal: bigint;
  readonly repaidTotal: bigint;
  readonly payoutIndex: bigint;
  readonly state: IssueState;
  readonly seq: bigint;
  readonly bump: number;
}

/** Облік власника за випуском (`FR-016`, `FR-038`). */
export interface HolderCheckpoint {
  readonly issue: PublicKey;
  readonly owner: PublicKey;
  readonly indexAtCheckpoint: bigint;
  readonly accrued: bigint;
  readonly claimedTotal: bigint;
  readonly bump: number;
}

/** Оферта вторинного ринку (`FR-024`, `FR-025`). */
export interface Offer {
  readonly seller: PublicKey;
  readonly issue: PublicKey;
  readonly amount: bigint;
  readonly price: bigint;
  readonly tokenEscrow: PublicKey;
  readonly nonce: bigint;
  readonly bump: number;
}

/** Чому байти не стали акаунтом. Розрізняється, бо дії різні. */
export type DecodeFailure =
  /** Прийшло не масивом байтів — межа з RPC не пройдена. */
  | 'not-bytes'
  /** Довжина не та, яку виділяє `init`: акаунт чужий або обрізаний. */
  | 'wrong-size'
  /** Перші вісім байтів належать іншому типу акаунта. */
  | 'wrong-discriminator'
  /** Байт enum поза оголошеними варіантами — стан із новішої версії програми. */
  | 'unknown-variant'
  /** Тег `Option` не 0 і не 1 — акаунт не є тим, за що себе видає. */
  | 'bad-option-tag';

/**
 * Відмова декодера. Названа, бо «акаунт не той» і «акаунт із новішої версії
 * програми» вимагають різних дій, а `catch` без розрізнення зводить їх в одне.
 */
export class AccountDecodeError extends Error {
  readonly kind: AccountKind;
  readonly reason: DecodeFailure;

  constructor(kind: AccountKind, reason: DecodeFailure, detail: string) {
    super(`${kind}: ${detail}`);
    this.name = 'AccountDecodeError';
    this.kind = kind;
    this.reason = reason;
  }
}

/** Межа з мережею: далі йдуть самі байти, і про них уже все відомо. */
const rawData = z.instanceof(Uint8Array);

/**
 * Курсор по байтах акаунта. Читає рівно в порядку оголошення полів — саме тому
 * декодери нижче виглядають як самі структури `state.rs` і звіряються з ними
 * очима.
 */
class Reader {
  private offset = DISCRIMINATOR_LEN;
  private readonly kind: AccountKind;
  private readonly bytes: Uint8Array;
  private readonly view: DataView;

  constructor(kind: AccountKind, bytes: Uint8Array) {
    this.kind = kind;
    this.bytes = bytes;
    this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  u8(): number {
    const value = this.view.getUint8(this.offset);
    this.offset += 1;
    return value;
  }

  u16(): number {
    const value = this.view.getUint16(this.offset, true);
    this.offset += 2;
    return value;
  }

  u64(): bigint {
    const value = this.view.getBigUint64(this.offset, true);
    this.offset += 8;
    return value;
  }

  i64(): bigint {
    const value = this.view.getBigInt64(this.offset, true);
    this.offset += 8;
    return value;
  }

  u128(): bigint {
    const lo = this.u64();
    const hi = this.u64();
    return (hi << 64n) | lo;
  }

  pubkey(): PublicKey {
    const value = new PublicKey(this.bytes.subarray(this.offset, this.offset + 32));
    this.offset += 32;
    return value;
  }

  /** `Option<Pubkey>`: спершу тег, і лише за ним ключ. `None` — один байт. */
  optionPubkey(): PublicKey | null {
    const tag = this.u8();
    if (tag === 0) return null;
    if (tag === 1) return this.pubkey();
    throw new AccountDecodeError(this.kind, 'bad-option-tag', `тег Option = ${tag}, не 0 і не 1`);
  }

  issueState(): IssueState {
    const byte = this.u8();
    const state = ISSUE_STATES[byte];
    if (state === undefined) {
      throw new AccountDecodeError(this.kind, 'unknown-variant', `стан випуску = ${byte}`);
    }
    return state;
  }
}

/**
 * Спільна перевірка межі: тип, розмір, дискримінатор. Розмір звіряється
 * точно — `init` виділяє рівно `8 + INIT_SPACE`, тож інша довжина означає, що
 * за адресою лежить не цей акаунт, і читати його як цей не можна.
 */
function reader(kind: AccountKind, data: unknown): Reader {
  const parsed = rawData.safeParse(data);
  if (!parsed.success) {
    throw new AccountDecodeError(kind, 'not-bytes', 'дані акаунта — не масив байтів');
  }
  const bytes = parsed.data;

  const expected = DISCRIMINATOR_LEN + ACCOUNT_SPACE[kind];
  if (bytes.length !== expected) {
    throw new AccountDecodeError(kind, 'wrong-size', `${bytes.length} байтів замість ${expected}`);
  }

  const want = DISCRIMINATORS[kind];
  for (let i = 0; i < DISCRIMINATOR_LEN; i += 1) {
    if (bytes[i] !== want[i]) {
      throw new AccountDecodeError(kind, 'wrong-discriminator', 'дискримінатор іншого акаунта');
    }
  }

  return new Reader(kind, bytes);
}

export function decodeProtocolConfig(data: unknown): ProtocolConfig {
  const r = reader('ProtocolConfig', data);
  return {
    admin: r.pubkey(),
    originationFeeBps: r.u16(),
    tradingFeeBps: r.u16(),
    maxPledgeBps: r.u16(),
    minTenorSecs: r.i64(),
    maxTenorSecs: r.i64(),
    historyThresholdSecs: r.i64(),
    usdcMint: r.pubkey(),
    feeVault: r.pubkey(),
    bump: r.u8(),
  };
}

export function decodeRevenueSource(data: unknown): RevenueSource {
  const r = reader('RevenueSource', data);
  return {
    issuer: r.pubkey(),
    authority: r.pubkey(),
    vault: r.pubkey(),
    firstSeenTs: r.i64(),
    totalObserved: r.u64(),
    observedBeforeIssue: r.u64(),
    activeIssue: r.optionPubkey(),
    seq: r.u64(),
    bump: r.u8(),
  };
}

export function decodeIssue(data: unknown): Issue {
  const r = reader('Issue', data);
  return {
    source: r.pubkey(),
    bondMint: r.pubkey(),
    escrowVault: r.pubkey(),
    subscriptionVault: r.pubkey(),
    face: r.u64(),
    couponBps: r.u16(),
    pledgeBps: r.u16(),
    maturityTs: r.i64(),
    subscriptionEndTs: r.i64(),
    minLot: r.u64(),
    raised: r.u64(),
    obligationTotal: r.u64(),
    repaidTotal: r.u64(),
    payoutIndex: r.u128(),
    state: r.issueState(),
    seq: r.u64(),
    bump: r.u8(),
  };
}

export function decodeHolderCheckpoint(data: unknown): HolderCheckpoint {
  const r = reader('HolderCheckpoint', data);
  return {
    issue: r.pubkey(),
    owner: r.pubkey(),
    indexAtCheckpoint: r.u128(),
    accrued: r.u64(),
    claimedTotal: r.u64(),
    bump: r.u8(),
  };
}

export function decodeOffer(data: unknown): Offer {
  const r = reader('Offer', data);
  return {
    seller: r.pubkey(),
    issue: r.pubkey(),
    amount: r.u64(),
    price: r.u64(),
    tokenEscrow: r.pubkey(),
    nonce: r.u64(),
    bump: r.u8(),
  };
}

/**
 * Фільтр `getProgramAccounts` за дискримінатором — те, чим береться список
 * випусків для маркетплейсу (`FR-031`).
 *
 * `bytes` віддається в base64, а не base58: base58-кодера в залежностях немає,
 * а RPC приймає обидва. Форма — `MemcmpFilter` із @solana/web3.js.
 */
export function discriminatorFilter(kind: AccountKind): {
  memcmp: { offset: number; encoding: 'base64'; bytes: string };
} {
  const bytes = DISCRIMINATORS[kind];
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return { memcmp: { offset: 0, encoding: 'base64', bytes: btoa(binary) } };
}
