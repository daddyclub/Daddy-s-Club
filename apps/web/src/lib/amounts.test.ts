import { describe, expect, it } from 'vitest';
import { formatUnits, parseAmount, pricePerFace, showUnits } from './amounts';

describe('parseAmount — рядок у цілі одиниці без float', () => {
  it.each([
    ['5000', 5_000_000_000n],
    ['4900.5', 4_900_500_000n],
    ['0.1', 100_000n],
    ['.25', 250_000n],
    ['12,75', 12_750_000n],
    ['0.000001', 1n],
    ['  7  ', 7_000_000n],
    ['3.', 3_000_000n],
  ])('%s → %s', (text, expected) => {
    expect(parseAmount(text)).toBe(expected);
  });

  it.each(['', '.', '-1', '1e6', 'abc', '1.2.3', '0.0000001'])(
    '%s — відмова, а не округлення',
    (text) => {
      expect(parseAmount(text)).toBeNull();
    },
  );
});

describe('formatUnits', () => {
  it.each([
    [4_875_500_000n, '4875.5'],
    [1n, '0.000001'],
    [0n, '0'],
    [5_000_000_000n, '5000'],
  ])('%s → %s', (value, text) => {
    expect(formatUnits(value)).toBe(text);
  });

  it('розбір і подання — взаємно обернені', () => {
    for (const value of [0n, 1n, 999_999n, 1_000_000n, 123_456_789_012n]) {
      expect(parseAmount(formatUnits(value))).toBe(value);
    }
  });
});

describe('pricePerFace', () => {
  it('ціна лота на одиницю номіналу', () => {
    expect(pricePerFace(4_900_000_000n, 5_000_000_000n)).toBe(0.98);
    expect(pricePerFace(1n, 0n)).toBe(0);
  });
});

describe('showUnits — точно, з групуванням', () => {
  it.each([
    [4_875_500_000n, '4,875.50'],
    [24_500_000n, '24.50'],
    [5_000n, '0.005'],
    [1n, '0.000001'],
    [0n, '0.00'],
    [250_000_000_000n, '250,000.00'],
  ])('%s → %s', (value, text) => {
    expect(showUnits(value)).toBe(text);
  });
});
