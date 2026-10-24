import { describe, expect, it } from 'vitest';
import {
  type CoverageIssue,
  type CoverageSource,
  coverageBps,
  type DropIssue,
  type DropSource,
  revenueDropSignal,
} from './coverage.js';

const DAY = 86_400n;
const NOW = 1_800_000_000n;

const source: CoverageSource = {
  firstSeenTs: NOW - 30n * DAY,
  totalObserved: 743_687_500_000n,
};

const issue: CoverageIssue = {
  pledgeBps: 2_000,
  subscriptionEndTs: NOW + 7n * DAY,
  maturityTs: NOW + 97n * DAY,
  obligationTotal: 273_750_000_000n,
};

describe('coverageBps', () => {
  it('projects the average revenue over the repayment window against the full obligation', () => {
    expect(coverageBps(source, issue, NOW)).toBe(16_300n);
  });

  it('rounds down, never to the nearest', () => {
    const short = { ...source, totalObserved: source.totalObserved - 1n };
    expect(coverageBps(short, issue, NOW)).toBe(16_299n);
  });

  it('averages over the whole history, so the same revenue over twice the time halves it', () => {
    const older = { ...source, firstSeenTs: NOW - 60n * DAY };
    expect(coverageBps(older, issue, NOW)).toBe(8_150n);
  });

  it('leaves the subscription window out of the term', () => {
    const lateClose = { ...issue, subscriptionEndTs: NOW + 52n * DAY };
    expect(coverageBps(source, lateClose, NOW)).toBe(8_150n);
  });

  it('scales with the pledged share', () => {
    expect(coverageBps(source, { ...issue, pledgeBps: 1_000 }, NOW)).toBe(8_150n);
  });

  it('is zero, not unknown, when the source has a history but no revenue', () => {
    expect(coverageBps({ ...source, totalObserved: 0n }, issue, NOW)).toBe(0n);
  });

  it('does not overflow at the top of the u64 counters', () => {
    const max = (1n << 64n) - 1n;
    const huge = { ...source, totalObserved: max };
    const expected = (max * 90n * DAY * 2_000n) / (30n * DAY * issue.obligationTotal);
    expect(coverageBps(huge, issue, NOW)).toBe(expected);
  });

  it('is unknown with no elapsed history to average over', () => {
    expect(coverageBps({ ...source, firstSeenTs: NOW }, issue, NOW)).toBeNull();
  });

  it('is unknown when the source claims to start in the future', () => {
    expect(coverageBps({ ...source, firstSeenTs: NOW + 1n }, issue, NOW)).toBeNull();
  });

  it('is unknown against a zero obligation', () => {
    expect(coverageBps(source, { ...issue, obligationTotal: 0n }, NOW)).toBeNull();
  });
});

describe('revenueDropSignal', () => {
  const created = NOW - 14n * DAY;
  const dropIssue: DropIssue = { createdTs: created };
  // 1_000 a day over the 30 days before the issue.
  const base: DropSource = {
    firstSeenTs: created - 30n * DAY,
    observedBeforeIssue: 30_000n,
    totalObserved: 30_000n,
  };
  const withSince = (amount: bigint): DropSource => ({
    ...base,
    totalObserved: base.observedBeforeIssue + amount,
  });

  it('fires when the rate since the issue falls below half the rate before it', () => {
    expect(revenueDropSignal(withSince(6_999n), dropIssue, NOW)).toBe(true);
  });

  it('stays quiet at exactly half the rate before', () => {
    expect(revenueDropSignal(withSince(7_000n), dropIssue, NOW)).toBe(false);
  });

  it('stays quiet when revenue holds its pace', () => {
    expect(revenueDropSignal(withSince(14_000n), dropIssue, NOW)).toBe(false);
  });

  it('fires when nothing has come through since the issue', () => {
    expect(revenueDropSignal(withSince(0n), dropIssue, NOW)).toBe(true);
  });

  it('compares rates, not amounts: the same revenue over a longer base is a slower pace', () => {
    const longBase = { ...withSince(6_999n), firstSeenTs: created - 60n * DAY };
    expect(revenueDropSignal(longBase, dropIssue, NOW)).toBe(false);
  });

  it('judges from exactly fourteen days after the issue', () => {
    expect(revenueDropSignal(withSince(0n), dropIssue, created + 14n * DAY)).toBe(true);
  });

  it('is unknown a second before fourteen days, however steep the fall', () => {
    expect(revenueDropSignal(withSince(0n), dropIssue, created + 14n * DAY - 1n)).toBeNull();
  });

  it('is unknown without a span before the issue to take a rate from', () => {
    expect(revenueDropSignal({ ...base, firstSeenTs: created }, dropIssue, NOW)).toBeNull();
  });

  it('does not overflow at the top of the u64 counters', () => {
    const max = (1n << 64n) - 1n;
    const huge = { ...base, observedBeforeIssue: max, totalObserved: max };
    expect(revenueDropSignal(huge, dropIssue, NOW)).toBe(true);
  });
});
