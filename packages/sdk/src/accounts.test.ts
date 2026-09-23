/**
 * Декодер звіряється зі `state.rs`, а не з уявленням про нього.
 *
 * Три речі мовчки розходяться між боками і всі три коштують грошей: склад і
 * **порядок** полів, розмір акаунта і порядок варіантів `IssueState`. Тому
 * тест читає сам `programs/daddys-club/src/state.rs` — імена полів, числа з
 * `account_sizes_are_pinned`, варіанти enum — і порівнює з тим, що робить
 * `accounts.ts`. Поле, додане в програмі, тут падає.
 *
 * Решта — байти: акаунт збирається руками з розрізненими значеннями, щоб
 * переставлені поля не збіглися випадково.
 */

import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { PublicKey } from '@solana/web3.js';
import { describe, expect, it } from 'vitest';
import {
  ACCOUNT_SPACE,
  type AccountKind,
  AccountDecodeError,
  DISCRIMINATOR_LEN,
  DISCRIMINATORS,
  ISSUE_STATES,
  decodeHolderCheckpoint,
  decodeIssue,
  decodeOffer,
  decodeProtocolConfig,
  decodeRevenueSource,
  discriminatorFilter,
} from './accounts.js';

const stateRs = readFileSync(
  new URL('../../../programs/daddys-club/src/state.rs', import.meta.url),
  'utf8',
);
const marketStateRs = readFileSync(
  new URL('../../../programs/daddys-market/src/state.rs', import.meta.url),
  'utf8',
);

/**
 * Де оголошено тип. `Offer` переїхав у програму ринку разом із вторинкою: ядро
 * є гуком мінта бонда й не може переказати його зі сховища оферти
 * (`docs/PLAN.md` → Архітектура). Декодер від цього не змінився — акаунт той
 * самий, — але джерело правди для нього тепер інший файл.
 */
const stateOf = (name: string): string => (name === 'Offer' ? marketStateRs : stateRs);

const KINDS: readonly AccountKind[] = [
  'ProtocolConfig',
  'RevenueSource',
  'Issue',
  'HolderCheckpoint',
  'Offer',
];

function must<T>(value: T | undefined, what: string): T {
  if (value === undefined) {
    throw new Error(`${what} не знайдено у state.rs — тест більше нічого не доводить`);
  }
  return value;
}

/** Тіло `pub struct X { ... }` або `pub enum X { ... }` зі `state.rs`. */
function block(keyword: 'struct' | 'enum', name: string): string {
  const source = stateOf(name);
  const start = source.indexOf(`pub ${keyword} ${name} {`);
  const end = source.indexOf('\n}', start);
  if (start < 0 || end < 0) throw new Error(`${keyword} ${name} у state.rs не знайдено`);
  return source.slice(start, end);
}

/** Поля структури в порядку оголошення — саме він і є порядком байтів borsh. */
function fieldsOf(name: string): string[] {
  return [...block('struct', name).matchAll(/^\s*pub (\w+):/gm)].map((found) =>
    must(found[1], `поле ${name}`),
  );
}

const camel = (snake: string): string =>
  snake.replace(/_([a-z0-9])/g, (_, letter: string) => letter.toUpperCase());

/** Числа з `account_sizes_are_pinned` — тими самими рахується rent-exempt. */
function pinnedSpace(name: string): number {
  const found = new RegExp(`${name}::INIT_SPACE, (\\d+)`).exec(stateOf(name));
  return Number(must(found?.[1], `${name}::INIT_SPACE`));
}

// ---- Складання акаунта з байтів --------------------------------------------

function leBytes(value: bigint, size: number): number[] {
  const out: number[] = [];
  let rest = value;
  for (let i = 0; i < size; i += 1) {
    out.push(Number(rest & 0xffn));
    rest >>= 8n;
  }
  return out;
}

const u8 = (value: number): number[] => [value];
const u16 = (value: number): number[] => leBytes(BigInt(value), 2);
const u64 = (value: bigint): number[] => leBytes(value, 8);
/** Знакове число пишеться доповняльним кодом — так само, як його пише Rust. */
const i64 = (value: bigint): number[] => leBytes(BigInt.asUintN(64, value), 8);
const u128 = (value: bigint): number[] => leBytes(value, 16);
const pk = (value: PublicKey): number[] => [...value.toBytes()];
const option = (value: PublicKey | null): number[] => (value === null ? [0] : [1, ...pk(value)]);

/**
 * Акаунт рівно того розміру, який виділяє `init`. Хвіст лишається нулями — так
 * само, як на ланцюгу лежить `RevenueSource` із `None`.
 */
function account(kind: AccountKind, ...body: number[][]): Uint8Array {
  const flat = body.flat();
  if (flat.length > ACCOUNT_SPACE[kind]) {
    throw new Error(`${kind}: тіло на ${flat.length} байтів не влазить у ${ACCOUNT_SPACE[kind]}`);
  }
  const bytes = new Uint8Array(DISCRIMINATOR_LEN + ACCOUNT_SPACE[kind]);
  bytes.set(DISCRIMINATORS[kind], 0);
  bytes.set(flat, DISCRIMINATOR_LEN);
  return bytes;
}

/** Ключі-пустушки: значення має лише їхня різність. */
const key = (fill: number): PublicKey => new PublicKey(new Uint8Array(32).fill(fill));

const CONFIG_BYTES = account(
  'ProtocolConfig',
  pk(key(1)),
  u16(150),
  u16(25),
  u16(3000),
  i64(2_592_000n),
  i64(15_552_000n),
  i64(604_800n),
  pk(key(2)),
  pk(key(3)),
  u8(254),
);

const sourceBytes = (activeIssue: PublicKey | null): Uint8Array =>
  account(
    'RevenueSource',
    pk(key(4)),
    pk(key(5)),
    pk(key(6)),
    i64(1_800_000_000n),
    u64(123_456n),
    u64(654_321n),
    option(activeIssue),
    u64(9n),
    u8(253),
  );

const ISSUE_BYTES = account(
  'Issue',
  pk(key(8)),
  pk(key(9)),
  pk(key(10)),
  pk(key(11)),
  u64(1_000_000_000n),
  u16(800),
  u16(1500),
  i64(1_830_000_000n),
  i64(1_805_000_000n),
  u64(100n),
  u64(250n),
  u64(1_080_000_000n),
  u64(42n),
  u128((1n << 80n) + 7n),
  u8(2),
  u64(3n),
  u8(252),
);

const HOLDER_BYTES = account(
  'HolderCheckpoint',
  pk(key(12)),
  pk(key(13)),
  u128((1n << 70n) + 5n),
  u64(11n),
  u64(22n),
  u8(251),
);

const OFFER_BYTES = account(
  'Offer',
  pk(key(14)),
  pk(key(15)),
  u64(7n),
  u64(8n),
  pk(key(16)),
  u64(9n),
  u8(250),
);

function failure(decode: () => unknown): AccountDecodeError {
  try {
    decode();
  } catch (error) {
    if (error instanceof AccountDecodeError) return error;
    throw error;
  }
  throw new Error('декодер прийняв те, що мусив відхилити');
}

// ---- Дзеркало state.rs -----------------------------------------------------

describe('state.rs — джерело правди', () => {
  it('дискримінатори ті самі, що рахує Anchor', () => {
    for (const kind of KINDS) {
      const sha = createHash('sha256').update(`account:${kind}`).digest();
      expect([...DISCRIMINATORS[kind]]).toEqual([...sha.subarray(0, DISCRIMINATOR_LEN)]);
    }
  });

  it('розміри збігаються з прибитими у state.rs', () => {
    for (const kind of KINDS) {
      expect(ACCOUNT_SPACE[kind]).toBe(pinnedSpace(kind));
    }
  });

  it('порядок варіантів IssueState той самий', () => {
    const variants = [...block('enum', 'IssueState').matchAll(/^\s{4}(\w+),/gm)].map((found) =>
      must(found[1], 'варіант IssueState'),
    );
    expect(variants).toEqual([...ISSUE_STATES]);
  });

  it.each([
    ['ProtocolConfig', decodeProtocolConfig(CONFIG_BYTES)],
    ['RevenueSource', decodeRevenueSource(sourceBytes(key(7)))],
    ['Issue', decodeIssue(ISSUE_BYTES)],
    ['HolderCheckpoint', decodeHolderCheckpoint(HOLDER_BYTES)],
    ['Offer', decodeOffer(OFFER_BYTES)],
  ])('%s: склад і порядок полів', (name, decoded) => {
    // Об'єкт складається в порядку читання байтів, тому порядок його ключів —
    // це і є розкладка, яку декодер вважає правильною.
    expect(Object.keys(decoded)).toEqual(fieldsOf(name).map(camel));
  });
});

// ---- Декодування -----------------------------------------------------------

describe('ProtocolConfig (FR-036)', () => {
  it('читається полями, а не зсувами навмання', () => {
    expect(decodeProtocolConfig(CONFIG_BYTES)).toEqual({
      admin: key(1),
      originationFeeBps: 150,
      tradingFeeBps: 25,
      maxPledgeBps: 3000,
      minTenorSecs: 2_592_000n,
      maxTenorSecs: 15_552_000n,
      historyThresholdSecs: 604_800n,
      usdcMint: key(2),
      feeVault: key(3),
      bump: 254,
    });
  });
});

describe('RevenueSource (FR-004, FR-006)', () => {
  it('зайняте джерело віддає адресу випуску', () => {
    expect(decodeRevenueSource(sourceBytes(key(7)))).toEqual({
      issuer: key(4),
      authority: key(5),
      vault: key(6),
      firstSeenTs: 1_800_000_000n,
      totalObserved: 123_456n,
      observedBeforeIssue: 654_321n,
      activeIssue: key(7),
      seq: 9n,
      bump: 253,
    });
  });

  it('вільне джерело віддає null, а не нульовий ключ', () => {
    // `None` займає один байт замість тридцяти трьох, тому все після нього
    // зсувається. Якби декодер читав за сталими зсувами, `seq` і `bump`
    // прочиталися б із хвоста нулів — і джерело виглядало б першим.
    const decoded = decodeRevenueSource(sourceBytes(null));
    expect(decoded.activeIssue).toBeNull();
    expect(decoded.seq).toBe(9n);
    expect(decoded.bump).toBe(253);
  });

  it('час до першого спостереження — знакове число', () => {
    // `first_seen_ts` має тип `i64`. Прочитаний як беззнаковий, він дав би
    // 1.8e19 і зробив би історію джерела вічною (`FR-007`).
    const bytes = sourceBytes(null);
    bytes.set(i64(-86_400n), DISCRIMINATOR_LEN + 96);
    expect(decodeRevenueSource(bytes).firstSeenTs).toBe(-86_400n);
  });
});

describe('Issue (FR-001, FR-015)', () => {
  it('читається полями', () => {
    expect(decodeIssue(ISSUE_BYTES)).toEqual({
      source: key(8),
      bondMint: key(9),
      escrowVault: key(10),
      subscriptionVault: key(11),
      face: 1_000_000_000n,
      couponBps: 800,
      pledgeBps: 1500,
      maturityTs: 1_830_000_000n,
      subscriptionEndTs: 1_805_000_000n,
      minLot: 100n,
      raised: 250n,
      obligationTotal: 1_080_000_000n,
      repaidTotal: 42n,
      payoutIndex: (1n << 80n) + 7n,
      state: 'Repaying',
      seq: 3n,
      bump: 252,
    });
  });

  it('payout_index не обрізається до u64', () => {
    // Масштаб 1e12 виносить індекс за u64 на перших же сумах; обрізаний
    // старший регістр дав би тиху недоплату кожному власнику (`FR-016`).
    expect(decodeIssue(ISSUE_BYTES).payoutIndex).toBeGreaterThan((1n << 64n) - 1n);
  });

  it.each(ISSUE_STATES.map((state, byte) => [byte, state] as const))(
    'байт %i — це %s',
    (byte, state) => {
      const bytes = Uint8Array.from(ISSUE_BYTES);
      bytes[DISCRIMINATOR_LEN + 204] = byte;
      expect(decodeIssue(bytes).state).toBe(state);
    },
  );
});

describe('HolderCheckpoint (FR-016, FR-038)', () => {
  it('читається полями', () => {
    expect(decodeHolderCheckpoint(HOLDER_BYTES)).toEqual({
      issue: key(12),
      owner: key(13),
      indexAtCheckpoint: (1n << 70n) + 5n,
      accrued: 11n,
      claimedTotal: 22n,
      bump: 251,
    });
  });
});

describe('Offer (FR-024)', () => {
  it('читається полями', () => {
    expect(decodeOffer(OFFER_BYTES)).toEqual({
      seller: key(14),
      issue: key(15),
      amount: 7n,
      price: 8n,
      tokenEscrow: key(16),
      nonce: 9n,
      bump: 250,
    });
  });
});

// ---- Відмови ---------------------------------------------------------------

describe('декодер відмовляє названо', () => {
  it('не байти — межа з RPC не пройдена', () => {
    expect(failure(() => decodeIssue('AQIDBA==')).reason).toBe('not-bytes');
    expect(failure(() => decodeIssue(null)).reason).toBe('not-bytes');
    expect(failure(() => decodeIssue([1, 2, 3])).reason).toBe('not-bytes');
  });

  it('не той розмір — за адресою лежить не цей акаунт', () => {
    expect(failure(() => decodeIssue(new Uint8Array(0))).reason).toBe('wrong-size');
    expect(failure(() => decodeIssue(ISSUE_BYTES.subarray(0, ISSUE_BYTES.length - 1))).reason).toBe(
      'wrong-size',
    );
    // Розміри всіх п'яти акаунтів різні, тому чужий тип впирається сюди ще до
    // дискримінатора — і це нормально: обидві відмови означають «не той».
    expect(failure(() => decodeOffer(ISSUE_BYTES)).reason).toBe('wrong-size');
  });

  it('чужий дискримінатор — тип не той', () => {
    const bytes = Uint8Array.from(ISSUE_BYTES);
    bytes[0] = (must(bytes[0], 'дискримінатор') + 1) % 256;
    expect(failure(() => decodeIssue(bytes)).reason).toBe('wrong-discriminator');
  });

  it('невідомий варіант стану — випуск із новішої версії програми', () => {
    const bytes = Uint8Array.from(ISSUE_BYTES);
    bytes[DISCRIMINATOR_LEN + 204] = ISSUE_STATES.length;
    expect(failure(() => decodeIssue(bytes)).reason).toBe('unknown-variant');
  });

  it('тег Option не 0 і не 1', () => {
    const bytes = sourceBytes(null);
    bytes[DISCRIMINATOR_LEN + 120] = 2;
    expect(failure(() => decodeRevenueSource(bytes)).reason).toBe('bad-option-tag');
  });

  it('відмова називає акаунт, який читали', () => {
    const error = failure(() => decodeOffer(new Uint8Array(1)));
    expect(error.kind).toBe('Offer');
    expect(error.message).toContain('Offer');
    expect(error).toBeInstanceOf(Error);
  });
});

// ---- Вибірка за дискримінатором --------------------------------------------

describe('discriminatorFilter (FR-031)', () => {
  it('віддає ті самі вісім байтів із нульового зсуву', () => {
    const filter = discriminatorFilter('Issue');
    expect(filter.memcmp.offset).toBe(0);
    expect(filter.memcmp.encoding).toBe('base64');
    const decoded = Uint8Array.from(atob(filter.memcmp.bytes), (char) => char.charCodeAt(0));
    expect([...decoded]).toEqual([...DISCRIMINATORS.Issue]);
  });
});
