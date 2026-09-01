/**
 * Дзеркало math.rs звіряється з тими самими фікстурами, що й сам math.rs
 * (`mod fixtures` там). Файл один — fixtures/math.json — тому будь-яка
 * розбіжність між реалізаціями стає червоним тестом з обох боків одночасно.
 *
 * JSON тут — така сама зовнішня межа, як RPC: числа приходять рядками, бо u128
 * не влазить ані в JSON-число, ані в number. Тому файл проходить Zod, а не
 * читається на віру: схема ловить і зіпсоване значення, і головну тиху
 * поразку — групу, яка спорожніла й перестала щось доводити.
 */

import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import raw from '../../../fixtures/math.json';
import {
  BPS_DENOM,
  SCALE,
  advanceIndex,
  claimable,
  obligationTotal,
  pledgedShare,
  splitIntercept,
} from './math.js';

/** Групи, які цей бік уміє прогнати. Нова група у файлі має впасти, а не мовчки пропаститись. */
const GROUPS = [
  'obligationTotal',
  'pledgedShare',
  'splitIntercept',
  'advanceIndex',
  'claimable',
] as const;

const u128 = z.string().regex(/^\d+$/, 'u128 записується десятковим рядком');
const bps = z.number().int().min(0).max(65_535);
const outcome = u128.nullable();
const label = { name: z.string().min(1), why: z.string().min(1) };

/** Порожня група — це не «нема що перевіряти», а мовчазна втрата покриття. */
const cases = <T extends z.ZodType>(shape: T) => z.array(shape).min(1);

const schema = z.object({
  scale: u128,
  bpsDenom: u128,
  obligationTotal: cases(z.object({ ...label, face: u128, couponBps: bps, expected: outcome })),
  pledgedShare: cases(z.object({ ...label, amount: u128, pledgeBps: bps, expected: outcome })),
  splitIntercept: cases(
    z.object({
      ...label,
      amount: u128,
      pledgeBps: bps,
      remaining: u128,
      expected: z.object({ toEscrow: u128, toIssuer: u128 }).nullable(),
    }),
  ),
  advanceIndex: cases(
    z.object({ ...label, index: u128, amount: u128, bondSupply: u128, expected: outcome }),
  ),
  claimable: cases(
    z.object({
      ...label,
      index: u128,
      checkpoint: u128,
      balance: u128,
      accrued: u128,
      expected: outcome,
    }),
  ),
});

const fixtures = schema.parse(raw);

/** `null` у фікстурах — це `None` у Rust. */
const want = (expected: string | null): bigint | null => (expected === null ? null : BigInt(expected));

describe('фікстури', () => {
  it('усі групи з файлу справді ганяються', () => {
    const inFile = Object.entries(raw)
      .filter(([, value]) => Array.isArray(value))
      .map(([key]) => key);
    expect(inFile.sort()).toEqual([...GROUPS].sort());
  });

  it('константи збігаються з дзеркалом', () => {
    expect(SCALE).toBe(BigInt(fixtures.scale));
    expect(BPS_DENOM).toBe(BigInt(fixtures.bpsDenom));
  });
});

describe('obligationTotal (FR-018)', () => {
  for (const c of fixtures.obligationTotal) {
    it(c.name, () => {
      expect(obligationTotal(BigInt(c.face), c.couponBps)).toBe(want(c.expected));
    });
  }
});

describe('pledgedShare', () => {
  for (const c of fixtures.pledgedShare) {
    it(c.name, () => {
      expect(pledgedShare(BigInt(c.amount), c.pledgeBps)).toBe(want(c.expected));
    });
  }
});

describe('splitIntercept (FR-014, FR-019, FR-020)', () => {
  for (const c of fixtures.splitIntercept) {
    it(c.name, () => {
      const split = splitIntercept(BigInt(c.amount), c.pledgeBps, BigInt(c.remaining));
      if (c.expected === null) {
        expect(split).toBeNull();
        return;
      }
      expect(split).toEqual({
        toEscrow: BigInt(c.expected.toEscrow),
        toIssuer: BigInt(c.expected.toIssuer),
      });
    });
  }

  it('надходження зберігається і стеля залишку не пробивається', () => {
    for (const c of fixtures.splitIntercept) {
      const split = splitIntercept(BigInt(c.amount), c.pledgeBps, BigInt(c.remaining));
      if (split === null) continue;
      expect(split.toEscrow + split.toIssuer, c.name).toBe(BigInt(c.amount));
      expect(split.toEscrow <= BigInt(c.remaining), c.name).toBe(true);
    }
  });
});

describe('advanceIndex (FR-015)', () => {
  for (const c of fixtures.advanceIndex) {
    it(c.name, () => {
      expect(advanceIndex(BigInt(c.index), BigInt(c.amount), BigInt(c.bondSupply))).toBe(
        want(c.expected),
      );
    });
  }
});

describe('claimable (FR-016)', () => {
  for (const c of fixtures.claimable) {
    it(c.name, () => {
      expect(
        claimable(BigInt(c.index), BigInt(c.checkpoint), BigInt(c.balance), BigInt(c.accrued)),
      ).toBe(want(c.expected));
    });
  }
});
