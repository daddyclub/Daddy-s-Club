import { describe, expect, it } from 'vitest';
import { type CoverageIssue, type CoverageSource, coverageBps } from './coverage.js';

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
