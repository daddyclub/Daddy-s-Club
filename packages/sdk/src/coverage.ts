import type { Issue, RevenueSource } from './accounts.js';

export type CoverageSource = Pick<RevenueSource, 'firstSeenTs' | 'totalObserved'>;

export type CoverageIssue = Pick<
  Issue,
  'pledgeBps' | 'subscriptionEndTs' | 'maturityTs' | 'obligationTotal'
>;

/**
 * Coverage ratio in bps (`FR-029`): 10_000n means the pledged share of the
 * expected revenue exactly covers what is owed.
 *
 * The term starts when subscription closes, not at creation: repayment starts
 * earlier only if the issue fills early, which nothing guarantees, so the
 * window an investor can count on is the one after the close.
 * The rate is the average over the source's whole history up to `now`; the
 * creation time is not on chain, so there is no "before the issue" cut.
 *
 * `pledgeBps` already carries the bps scale, so the result needs no extra
 * factor. Everything is multiplied before the single division: one floor at
 * the end, and coverage is never overstated.
 *
 * `null` means there is nothing to average over or nothing to cover.
 */
export function coverageBps(
  source: CoverageSource,
  issue: CoverageIssue,
  now: bigint,
): bigint | null {
  const elapsed = now - source.firstSeenTs;
  if (elapsed <= 0n || issue.obligationTotal === 0n) return null;
  const term = issue.maturityTs - issue.subscriptionEndTs;
  const covered = source.totalObserved * term * BigInt(issue.pledgeBps);
  return covered / (elapsed * issue.obligationTotal);
}
