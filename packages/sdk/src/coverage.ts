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

export type DropSource = Pick<
  RevenueSource,
  'firstSeenTs' | 'observedBeforeIssue' | 'totalObserved'
>;

export type DropIssue = Pick<Issue, 'createdTs'>;

const DROP_MIN_AGE_SECS = 14n * 86_400n;

/**
 * Revenue drop signal (`FR-030`): `true` when the rate through the hook since
 * the issue is below half the rate the source showed before it.
 *
 * A signal, not an accusation: protocol revenue falls for market reasons too,
 * and only what passed the hook is visible.
 *
 * Compared by cross-multiplying, so no rate is floored before the comparison.
 * `observedBeforeIssue` is the source's latest cut: pass the issue that is the
 * source's active one, or the base belongs to a different issue.
 *
 * `null` means too early to judge (under 14 days since the issue) or no time
 * before the issue to take a base rate from.
 */
export function revenueDropSignal(
  source: DropSource,
  issue: DropIssue,
  now: bigint,
): boolean | null {
  const spanBefore = issue.createdTs - source.firstSeenTs;
  const spanAfter = now - issue.createdTs;
  if (spanBefore <= 0n || spanAfter < DROP_MIN_AGE_SECS) return null;
  const observedAfter = source.totalObserved - source.observedBeforeIssue;
  return 2n * observedAfter * spanBefore < source.observedBeforeIssue * spanAfter;
}
