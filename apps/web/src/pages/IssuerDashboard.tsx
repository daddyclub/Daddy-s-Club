import { Link, useParams } from 'react-router-dom';
import DataRow from '@/components/DataRow';
import NoticePanel from '@/components/NoticePanel';
import ProgressBar from '@/components/ProgressBar';
import RevenueChart from '@/components/RevenueChart';
import Section from '@/components/Section';
import SplitFlap from '@/components/SplitFlap';
import StateChip from '@/components/StateChip';
import { findIssue, ISSUER_30D, QUILLFIN, REVENUE_30D } from '@/data/mock';
import { amount, percent, ratio } from '@/lib/format';

const IssuerDashboard = () => {
  const { id } = useParams<{ id: string }>();
  const issue = findIssue(id ?? QUILLFIN.id);

  if (issue === undefined || issue.id !== QUILLFIN.id) {
    return (
      <div className="mx-auto w-full max-w-[1400px] px-5 py-16">
        <div className="text-[13px] uppercase tracking-[0.28em]">Desk not available</div>
        <p className="mt-3 max-w-[62ch] text-[12px] leading-[1.7] text-board-dim">
          You are signed in as Quillfin Swap. Only your own issuer desk is visible from here.
        </p>
        <Link
          to="/"
          className="mt-4 inline-block border-b border-board-accent text-[11px] uppercase tracking-[0.2em] text-board-accent"
        >
          Back to marketplace
        </Link>
      </div>
    );
  }

  const keptPct = (ISSUER_30D.kept / ISSUER_30D.totalRevenue) * 100;
  const splitPct = (ISSUER_30D.splitToHolders / ISSUER_30D.totalRevenue) * 100;

  return (
    <div className="mx-auto w-full max-w-[1400px] px-5 py-8">
      <Link
        to={`/issue/${issue.id}`}
        className="text-[10px] uppercase tracking-[0.2em] text-board-dim hover:text-board-ink"
      >
        &larr; Issue detail
      </Link>

      <div className="mb-8 mt-4 flex flex-wrap items-end justify-between gap-4 border-b border-board-ink pb-4">
        <div>
          <div className="flex flex-wrap items-center gap-4">
            <h1 className="text-[clamp(1.4rem,3vw,2rem)] uppercase tracking-[0.14em]">
              {issue.issuer}
            </h1>
            <StateChip state={issue.state} withMark />
            <span className="text-[10px] uppercase tracking-[0.2em] text-board-dim">
              Issuer desk
            </span>
          </div>
          <p className="mt-3 max-w-[74ch] text-[12px] leading-[1.7] text-board-dim">
            What your protocol earned, what was split off at the moment of earning, and what stayed
            with you.
          </p>
        </div>
        <div className="text-right">
          <div className="text-[10px] uppercase tracking-[0.16em] text-board-dim">
            Pledged share
          </div>
          <div className="text-[13px] tabular-nums">{issue.shareLabel} of every fee</div>
        </div>
      </div>

      <div className="grid gap-px border border-board-ink bg-board-rule md:grid-cols-3">
        <div className="bg-board-bg px-4 py-5">
          <div className="mb-3 text-[10px] uppercase tracking-[0.2em] text-board-dim">
            Fee revenue &middot; last 30 days
          </div>
          <div className="flex items-end gap-2">
            <SplitFlap value={amount(ISSUER_30D.totalRevenue)} size="lg" />
            <span className="pb-1 text-[10px] uppercase tracking-[0.16em] text-board-dim">
              USDC
            </span>
          </div>
        </div>
        <div className="bg-board-bg px-4 py-5">
          <div className="mb-3 text-[10px] uppercase tracking-[0.2em] text-board-dim">
            Split to bondholders
          </div>
          <div className="flex items-end gap-2">
            <SplitFlap
              value={amount(ISSUER_30D.splitToHolders)}
              size="lg"
              tone="text-board-accent"
            />
            <span className="pb-1 text-[10px] uppercase tracking-[0.16em] text-board-dim">
              USDC
            </span>
          </div>
        </div>
        <div className="bg-board-bg px-4 py-5">
          <div className="mb-3 text-[10px] uppercase tracking-[0.2em] text-board-dim">
            Kept by the protocol
          </div>
          <div className="flex items-end gap-2">
            <SplitFlap value={amount(ISSUER_30D.kept)} size="lg" />
            <span className="pb-1 text-[10px] uppercase tracking-[0.16em] text-board-dim">
              USDC
            </span>
          </div>
        </div>
      </div>

      <div className="mt-3 flex flex-wrap items-center gap-x-6 gap-y-2 text-[10px] uppercase tracking-[0.16em] text-board-dim">
        <span className="flex items-center gap-2">
          <span className="inline-block h-[7px] w-[7px] bg-board-accent" />
          Bondholder portion {percent(splitPct)}
        </span>
        <span className="flex items-center gap-2">
          <span className="inline-block h-[7px] w-[7px] bg-board-ink/80" />
          Kept portion {percent(keptPct)}
        </span>
      </div>

      <div className="mt-10 grid gap-10 lg:grid-cols-[1.5fr_1fr]">
        <Section title="Daily revenue, stacked" aside="Bondholder portion at the base of each bar">
          <RevenueChart data={REVENUE_30D} sharePct={QUILLFIN.sharePct} height={220} />
        </Section>

        <Section title="Obligation">
          <div className="mb-5">
            <div className="mb-2 text-[10px] uppercase tracking-[0.16em] text-board-dim">
              Remaining obligation
            </div>
            <div className="flex items-end gap-2">
              <SplitFlap value={amount(ISSUER_30D.remainingObligation)} size="lg" />
              <span className="pb-1 text-[10px] uppercase tracking-[0.16em] text-board-dim">
                USDC
              </span>
            </div>
            <div className="mt-4">
              <ProgressBar pct={QUILLFIN.repaidPct} height={10} />
              <div className="mt-2 flex justify-between text-[11px] uppercase tracking-[0.16em] text-board-dim">
                <span>{percent(QUILLFIN.repaidPct)} repaid</span>
                <span>of {amount(QUILLFIN.totalOwed)} USDC</span>
              </div>
            </div>
          </div>

          <DataRow label="Face value" value={`${amount(issue.face)} USDC`} />
          <DataRow label="Coupon" value={percent(issue.couponPct)} />
          <DataRow label="Coverage ratio" value={ratio(QUILLFIN.coverage)} />
          <DataRow
            label="Average revenue"
            value={`${amount(QUILLFIN.avgDailyRevenue)} USDC per day`}
          />
          <DataRow
            label="Through the split"
            value={`${amount(QUILLFIN.dailyToHolders)} USDC per day`}
            tone="accent"
          />
          <DataRow
            label="Projected release, at the current pace"
            value={ISSUER_30D.projectedRelease}
          />

          <div className="mt-6">
            <NoticePanel word="The split stops by itself" tone="accent">
              At the current pace the obligation is met on {ISSUER_30D.projectedRelease}. From that
              moment {issue.shareLabel} stops being taken and 100% of fees return to {issue.issuer}.
              Nothing needs to be signed or repaid manually.
            </NoticePanel>
          </div>
        </Section>
      </div>
    </div>
  );
};

export default IssuerDashboard;
