/**
 * Деривації TS звіряються з Rust не на око, а з файлів.
 *
 * Спільного джерела seeds між боками немає — байти переписані. Тому тест читає
 * `programs/daddys-club/src/state.rs` (самі seed-байти) і
 * `programs/daddys-club/tests/harness.rs` (їхній порядок у деривації) і
 * порівнює з тим, що робить `pda.ts`. Перейменований seed або переставлений
 * аргумент падає тут, а не на першому виклику, який промахнувся повз акаунт.
 *
 * Читання чужих файлів навмисне: те саме роблять фікстури арифметики
 * (`math.test.ts`), і з тієї ж причини — доказ мусить спиратися на джерело, а
 * не на його копію.
 */

import { readFileSync } from 'node:fs';
import { PublicKey } from '@solana/web3.js';
import { describe, expect, it } from 'vitest';
import {
  CONFIG_SEED,
  ESCROW_SEED,
  EXTRA_METAS_SEED,
  HOLDER_SEED,
  ISSUE_SEED,
  OFFER_SEED,
  PROGRAM_ID,
  SOURCE_SEED,
  configPda,
  extraAccountMetasPda,
  holderPda,
  issuePda,
  MARKET_PROGRAM_ID,
  offerEscrowPda,
  offerPda,
  sourcePda,
} from './pda.js';

const stateRs = readFileSync(new URL('../../../programs/daddys-club/src/state.rs', import.meta.url), 'utf8');
const harnessRs = readFileSync(new URL('../../../programs/daddys-club/tests/harness.rs', import.meta.url), 'utf8');
const libRs = readFileSync(new URL('../../../programs/daddys-club/src/lib.rs', import.meta.url), 'utf8');
const marketStateRs = readFileSync(
  new URL('../../../programs/daddys-market/src/state.rs', import.meta.url),
  'utf8',
);
const marketLibRs = readFileSync(
  new URL('../../../programs/daddys-market/src/lib.rs', import.meta.url),
  'utf8',
);
const issueRs = readFileSync(
  new URL('../../../programs/daddys-club/src/instructions/issue.rs', import.meta.url),
  'utf8',
);

const utf8 = new TextDecoder();

/** Ключі-пустушки: важлива лише їхня різність, а не походження. */
const key = (fill: number): PublicKey => new PublicKey(new Uint8Array(32).fill(fill));
const ISSUER = key(12);
const INVESTOR = key(13);
const BOND_MINT = key(23);

function must<T>(value: T | undefined, what: string): T {
  if (value === undefined) {
    throw new Error(`${what} не знайдено — тест більше нічого не доводить`);
  }
  return value;
}

/** `pub const X_SEED: &[u8] = b"...";` зі `state.rs` названої програми. */
function seedBytesFromState(name: string, source: string = stateRs): string {
  const found = new RegExp(`pub const ${name}: &\\[u8\\] = b"([^"]+)";`).exec(source);
  return must(found?.[1], `${name} у state.rs`);
}

/**
 * Список seeds у порядку, в якому їх складає харнес. Береться текстом із
 * `find_program_address(&[ ... ])`: саме цей порядок і перевіряє
 * `the_pda_helpers_derive_from_the_seeds_the_protocol_documents`.
 */
function harnessSeeds(fn: string): string[] {
  const declaration = harnessRs.indexOf(`pub fn ${fn}(`);
  if (declaration < 0) throw new Error(`${fn} у harness.rs не знайдено`);
  const call = harnessRs.indexOf('find_program_address(', declaration);
  // Список seeds буває і в рядок, і розкладеним rustfmt — шукається його
  // початок, а не конкретне форматування.
  const list = harnessRs.indexOf('&[', call);
  const end = harnessRs.indexOf(']', list);
  if (call < 0 || list < 0 || end < 0) {
    throw new Error(`${fn} у harness.rs дерівує не через find_program_address`);
  }
  return harnessRs
    .slice(list + '&['.length, end)
    .split(',')
    .map((part) => part.trim())
    .filter((part) => part.length > 0);
}

describe('seeds збігаються зі state.rs', () => {
  it.each([
    ['CONFIG_SEED', CONFIG_SEED],
    ['SOURCE_SEED', SOURCE_SEED],
    ['ISSUE_SEED', ISSUE_SEED],
    ['HOLDER_SEED', HOLDER_SEED],
  ])('%s', (name, bytes) => {
    expect(utf8.decode(bytes)).toBe(seedBytesFromState(name));
  });

  it.each([
    ['OFFER_SEED', OFFER_SEED],
    ['ESCROW_SEED', ESCROW_SEED],
  ])('%s збігається зі state.rs програми ринку', (name, bytes) => {
    // Акаунти не ядра: вторинка виїхала в `daddys_market`, бо ядро є гуком
    // мінта бонда й не може переказати його зі сховища оферти.
    expect(utf8.decode(bytes)).toBe(seedBytesFromState(name, marketStateRs));
  });

  it('EXTRA_METAS_SEED збігається з issue.rs', () => {
    // Цей seed живе не в `state.rs`, бо він не наш: за ним Token-2022 шукає
    // список сам. Що байти в програмі ті самі, що й у Token-2022, доводить
    // `the_metas_seed_is_the_one_token_2022_looks_for` у самій програмі.
    const found = /pub const EXTRA_METAS_SEED: &\[u8\] = b"([^"]+)";/.exec(issueRs);
    expect(utf8.decode(EXTRA_METAS_SEED)).toBe(must(found?.[1], 'EXTRA_METAS_SEED у issue.rs'));
  });

  it('інших seed-констант у програмі немає', () => {
    // Seed, доданий у state.rs без деривації тут, — це акаунт, який клієнт не
    // вміє знайти. Хай про нього скаже тест, а не порожній екран.
    const declared = [...stateRs.matchAll(/pub const (\w+_SEED):/g)].map((found) => found[1]);
    expect(declared.sort()).toEqual(['CONFIG_SEED', 'HOLDER_SEED', 'ISSUE_SEED', 'SOURCE_SEED']);

    const market = [...marketStateRs.matchAll(/pub const (\w+_SEED):/g)].map((found) => found[1]);
    expect(market.sort()).toEqual(['ESCROW_SEED', 'OFFER_SEED']);
  });
});

describe('порядок seeds збігається з харнесом', () => {
  it.each([
    ['config_pda', ['CONFIG_SEED']],
    ['source_pda', ['SOURCE_SEED', 'issuer.as_ref()', '&seq.to_le_bytes()']],
    ['issue_pda', ['ISSUE_SEED', 'source.as_ref()', '&seq.to_le_bytes()']],
    ['holder_pda', ['HOLDER_SEED', 'issue.as_ref()', 'owner.as_ref()']],
    ['extra_metas_pda', ['EXTRA_METAS_SEED', 'mint.as_ref()']],
    ['offer_pda', ['OFFER_SEED', 'issue.as_ref()', 'seller.as_ref()', '&nonce.to_le_bytes()']],
    ['offer_escrow_pda', ['ESCROW_SEED', 'offer.as_ref()']],
  ])('%s', (fn, expected) => {
    expect(harnessSeeds(fn)).toEqual(expected);
  });
});

describe('program id', () => {
  it('той самий, що declare_id! у програмі', () => {
    const found = /declare_id!\("([^"]+)"\)/.exec(libRs);
    expect(PROGRAM_ID.toBase58()).toBe(must(found?.[1], 'declare_id! у lib.rs'));
  });

  it('id ринку той самий, що declare_id! у його програмі', () => {
    const found = /declare_id!\("([^"]+)"\)/.exec(marketLibRs);
    expect(MARKET_PROGRAM_ID.toBase58()).toBe(must(found?.[1], 'declare_id! у market lib.rs'));
  });

  it('це різні програми', () => {
    // Одна й та сама адреса тут означала б, що вторинку зібрали в ядрі — тобто
    // те, що Solana відхиляє реентрансі.
    expect(PROGRAM_ID.equals(MARKET_PROGRAM_ID)).toBe(false);
  });
});

describe('деривації', () => {
  const source = sourcePda(ISSUER, 0n).address;
  const issue = issuePda(source, 0n).address;

  it('адреси лежать поза кривою — це PDA, а не гаманці', () => {
    for (const derived of [
      configPda(),
      sourcePda(ISSUER, 7n),
      issuePda(source, 3n),
      holderPda(issue, INVESTOR),
      extraAccountMetasPda(BOND_MINT),
      offerPda(issue, INVESTOR, 5n),
      offerEscrowPda(offerPda(issue, INVESTOR, 5n).address),
    ]) {
      expect(PublicKey.isOnCurve(derived.address.toBytes())).toBe(false);
      expect(derived.bump).toBeGreaterThanOrEqual(0);
      expect(derived.bump).toBeLessThanOrEqual(255);
    }
  });

  it('лічильник у seeds справді розводить адреси', () => {
    // Якби `seq` губився, другий випуск джерела мовчки писався б у перший.
    expect(sourcePda(ISSUER, 0n).address.equals(sourcePda(ISSUER, 1n).address)).toBe(false);
    expect(issuePda(source, 0n).address.equals(issuePda(source, 1n).address)).toBe(false);
    expect(
      offerPda(issue, INVESTOR, 0n).address.equals(offerPda(issue, INVESTOR, 1n).address),
    ).toBe(false);
  });

  it('порядок ключів у seeds не симетричний', () => {
    // `["holder", issue, owner]` і `["holder", owner, issue]` — різні акаунти;
    // переставлені аргументи мають бути видно, а не «іноді працює».
    expect(holderPda(issue, INVESTOR).address.equals(holderPda(INVESTOR, issue).address)).toBe(
      false,
    );
  });

  it('інший program id дає іншу адресу', () => {
    const other = new PublicKey(new Uint8Array(32).fill(9));
    expect(configPda(other).address.equals(configPda().address)).toBe(false);
  });

  it('лічильник поза u64 — відмова, а не інша адреса', () => {
    expect(() => sourcePda(ISSUER, -1n)).toThrow(RangeError);
    expect(() => sourcePda(ISSUER, 1n << 64n)).toThrow(RangeError);
    expect(() => offerPda(issue, INVESTOR, 1n << 64n)).toThrow(RangeError);
  });

  it('межа u64 приймається', () => {
    expect(() => sourcePda(ISSUER, (1n << 64n) - 1n)).not.toThrow();
  });
});
