import { useMemo, useState } from 'react';
import { Link } from 'react-router-dom';
import InfoTip from '@/components/InfoTip';
import ProgressBar from '@/components/ProgressBar';
import StateChip from '@/components/StateChip';
import { ISSUE_STATES, ISSUES, type IssueState } from '@/data/mock';
import { amount, percent, ratio } from '@/lib/format';

type Filter = IssueState | 'All';

const FILTERS: Filter[] = ['All', ...ISSUE_STATES];

const COVERAGE_NOTE =
  'Coverage ratio is the revenue expected over the term, multiplied by the pledged share, divided by the total owed.';

const GRID =
  'grid grid-cols-[minmax(150px,1.5fr)_128px_minmax(120px,1fr)_84px_120px_96px_92px_minmax(150px,1.3fr)] gap-4 items-center';

const maturityCell = (issue: (typeof ISSUES)[number]): { text: string; tone: string } => {
  if (issue.state === 'Overdue' && issue.daysToMaturity !== null) {
    return { text: `${Math.abs(issue.daysToMaturity)} days overdue`, tone: 'text-board-red' };
  }
  if (issue.daysToMaturity === null) return { text: '\u2014', tone: 'text-board-dim' };
  return { text: `${issue.daysToMaturity} days`, tone: 'text-board-ink' };
};

const Marketplace = () => {
  const [filter, setFilter] = useState<Filter>('All');

  const rows = useMemo(
    () => (filter === 'All' ? ISSUES : ISSUES.filter((issue) => issue.state === filter)),
    [filter],
  );

  return (
    <div className="mx-auto w-full max-w-[1400px] px-5 py-8">
      <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
        <div>
          <h1 className="text-[13px] uppercase tracking-[0.3em]">Departures &middot; all issues</h1>
          <p className="mt-2 max-w-[72ch] text-[12px] leading-[1.7] text-board-dim">
            Protocols raise against their own fee revenue. A fixed share of each fee is split off
            the moment it is earned and paid to bondholders until face plus coupon is repaid.
          </p>
        </div>
        <div className="text-right text-[10px] uppercase tracking-[0.16em] text-board-dim">
          {rows.length} of {ISSUES.length} issues shown
        </div>
      </div>

      <div className="mb-5 flex flex-wrap items-center gap-2 border-y border-board-rule py-3">
        <span className="mr-2 text-[10px] uppercase tracking-[0.2em] text-board-dim">Filter</span>
        {FILTERS.map((item) => {
          const active = item === filter;
          return (
            <button
              key={item}
              type="button"
              onClick={() => setFilter(item)}
              className={`border px-2 py-[4px] text-[10px] uppercase tracking-[0.16em] transition-colors ${
                active
                  ? 'border-board-ink bg-board-ink text-board-bg'
                  : 'border-board-rule text-board-dim hover:border-board-ink hover:text-board-ink'
              }`}
            >
              {item}
            </button>
          );
        })}
      </div>

      <div
        className={`${GRID} border-b border-board-ink pb-2 text-[10px] uppercase tracking-[0.16em] text-board-dim`}
      >
        <span>Issuer</span>
        <span>State</span>
        <span className="text-right">Face value</span>
        <span className="text-right">Coupon</span>
        <span className="text-right">To maturity</span>
        <span className="text-right">Share</span>
        <span className="text-right">
          <InfoTip text={COVERAGE_NOTE}>
            <span>Coverage</span>
          </InfoTip>
        </span>
        <span>Progress</span>
      </div>

      {rows.length === 0 ? (
        <div className="border-b border-board-rule py-10 text-center text-[12px] text-board-dim">
          No issues in this state. Clear the filter to see the whole board.
        </div>
      ) : (
        rows.map((issue) => {
          const maturity = maturityCell(issue);
          const tone =
            issue.state === 'Overdue'
              ? 'red'
              : issue.state === 'Undersubscribed'
                ? 'faint'
                : issue.state === 'Repaid'
                  ? 'ink'
                  : 'accent';
          return (
            <Link
              key={issue.id}
              to={`/issue/${issue.id}`}
              className={`${GRID} border-b border-board-rule py-[14px] transition-colors hover:bg-board-panel`}
            >
              <span className="text-[14px] tracking-tight">{issue.issuer}</span>
              <span>
                <StateChip state={issue.state} />
              </span>
              <span className="text-right text-[13px] tabular-nums tracking-tight">
                {amount(issue.face)} USDC
              </span>
              <span className="text-right text-[13px] tabular-nums">
                {percent(issue.couponPct)}
              </span>
              <span className={`text-right text-[13px] tabular-nums ${maturity.tone}`}>
                {maturity.text}
              </span>
              <span className="text-right text-[13px] tabular-nums">{issue.shareLabel}</span>
              <span className="text-right text-[13px] tabular-nums">
                {issue.coverage === null ? (
                  <span className="text-board-dim">&mdash;</span>
                ) : (
                  ratio(issue.coverage)
                )}
              </span>
              <span className="flex items-center gap-3">
                <ProgressBar pct={issue.progressPct} tone={tone} />
                <span className="w-[92px] shrink-0 text-right text-[11px] tabular-nums text-board-dim">
                  {percent(issue.progressPct)}{' '}
                  {issue.progressKind === 'repayment' ? 'repaid' : 'raised'}
                </span>
              </span>
            </Link>
          );
        })
      )}

      <div className="mt-6 flex flex-wrap gap-x-8 gap-y-2 text-[10px] uppercase tracking-[0.16em] text-board-dim">
        <span>Board time {'\u2014'} 26 Aug 2026</span>
        <span>Repaying and Subscribing shown in ink-blue</span>
        <span className="text-board-red">Red is reserved for overdue</span>
      </div>
    </div>
  );
};

export default Marketplace;
