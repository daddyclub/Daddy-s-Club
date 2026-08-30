import { Link, useParams } from 'react-router-dom';
import BoardButton from '@/components/BoardButton';
import Countdown from '@/components/Countdown';
import DataRow from '@/components/DataRow';
import FeeTape from '@/components/FeeTape';
import HolderPosition from '@/components/HolderPosition';
import InfoTip from '@/components/InfoTip';
import NoticePanel from '@/components/NoticePanel';
import OffersPanel from '@/components/OffersPanel';
import ProgressBar from '@/components/ProgressBar';
import RepaymentBoard from '@/components/RepaymentBoard';
import RevenueChart from '@/components/RevenueChart';
import Section from '@/components/Section';
import SplitFlap from '@/components/SplitFlap';
import StateChip from '@/components/StateChip';
import {
  COPPERLINE,
  findIssue,
  findPosition,
  MARROWBONE,
  offersFor,
  QUILLFIN,
  REVENUE_30D,
  SALTMARSH,
  TANGLEWOOD,
} from '@/data/mock';
import { useFeeStream } from '@/hooks/useFeeStream';
import { amount, percent, ratio } from '@/lib/format';

const COVERAGE_NOTE =
  'Coverage ratio is the revenue expected over the term, multiplied by the pledged share, divided by the total owed.';

const IssueDetail = () => {
  const { id } = useParams<{ id: string }>();
  const issue = findIssue(id);
  const isQuillfin = issue?.id === QUILLFIN.id;

  const stream = useFeeStream(
    QUILLFIN.repaidStart,
    QUILLFIN.totalOwed,
    QUILLFIN.sharePct,
    isQuillfin,
  );

  if (issue === undefined) {
    return (
      <div className="mx-auto w-full max-w-[1400px] px-5 py-16">
        <div className="text-[13px] uppercase tracking-[0.28em]">Issue not on the board</div>
        <Link
          to="/"
          className="mt-4 inline-block border-b border-board-accent text-[11px] uppercase tracking-[0.2em] text-board-accent"
        >
          Back to marketplace
        </Link>
      </div>
    );
  }

  const position = findPosition(issue.id);
  const offers = offersFor(issue.id);

  return (
    <div className="mx-auto w-full max-w-[1400px] px-5 py-8">
      <Link
        to="/"
        className="text-[10px] uppercase tracking-[0.2em] text-board-dim hover:text-board-ink"
      >
        &larr; All issues
      </Link>

      <div className="mb-6 mt-4 flex flex-wrap items-end justify-between gap-4 border-b border-board-ink pb-4">
        <div>
          <div className="flex flex-wrap items-center gap-4">
            <h1 className="text-[clamp(1.4rem,3vw,2rem)] uppercase tracking-[0.14em]">
              {issue.issuer}
            </h1>
            <StateChip state={issue.state} withMark />
          </div>
          <p className="mt-3 max-w-[74ch] text-[12px] leading-[1.7] text-board-dim">
            {issue.summary}
          </p>
        </div>
        <div className="flex flex-wrap items-end gap-x-8 gap-y-2 text-right">
          <div>
            <div className="text-[10px] uppercase tracking-[0.16em] text-board-dim">Matures</div>
            <div className="text-[13px] tabular-nums">{issue.matures}</div>
          </div>
          <div>
            <div className="text-[10px] uppercase tracking-[0.16em] text-board-dim">
              Pledged share
            </div>
            <div className="text-[13px] tabular-nums">{issue.shareLabel}</div>
          </div>
          {isQuillfin ? (
            <Link
              to={`/issuer/${issue.id}`}
              className="border-b border-board-accent pb-[2px] text-[10px] uppercase tracking-[0.2em] text-board-accent"
            >
              Issuer desk
            </Link>
          ) : null}
        </div>
      </div>

      {/* ---------------- hero, per state ---------------- */}

      {isQuillfin ? (
        <>
          <RepaymentBoard
            repaid={stream.repaid}
            totalOwed={QUILLFIN.totalOwed}
            settled={stream.settled}
            caption={`Started at ${amount(QUILLFIN.repaidStart)} USDC \u00b7 ${QUILLFIN.sharePct}% of every fee`}
          />
          <Section
            title="Fee tape · intercepted in real time"
            aside="Newest at the top · last 8 fees"
            className="mt-8"
          >
            <FeeTape rows={stream.rows} sharePct={QUILLFIN.sharePct} live={!stream.settled} />
          </Section>
        </>
      ) : null}

      {issue.state === 'Subscribing' ? (
        <div className="border-y border-board-ink py-6">
          <div className="mb-4 text-[10px] uppercase tracking-[0.28em] text-board-dim">
            Raised so far
          </div>
          <div className="flex flex-wrap items-end gap-3">
            <SplitFlap value={amount(TANGLEWOOD.raised)} size="xl" />
            <span className="pb-2 text-[12px] uppercase tracking-[0.16em] text-board-dim">
              USDC of {amount(TANGLEWOOD.target)} USDC
            </span>
          </div>
          <div className="mt-5 grid gap-6 md:grid-cols-[1fr_auto] md:items-end">
            <div>
              <ProgressBar pct={TANGLEWOOD.raisedPct} height={10} />
              <div className="mt-2 flex justify-between text-[11px] uppercase tracking-[0.16em] text-board-dim">
                <span>Subscription book open</span>
                <span>{percent(TANGLEWOOD.raisedPct)} of target</span>
              </div>
            </div>
            <Countdown seconds={TANGLEWOOD.closesInSeconds} label="Subscription closes in" />
          </div>
          <div className="mt-6">
            <NoticePanel word="Not yet splitting" tone="accent">
              No fees are being split yet. The {issue.shareLabel} share starts the moment the book
              closes and runs until {amount(TANGLEWOOD.totalOwed)} USDC is repaid.
            </NoticePanel>
          </div>
        </div>
      ) : null}

      {issue.state === 'Repaid' ? (
        <RepaymentBoard
          repaid={COPPERLINE.totalOwed}
          totalOwed={COPPERLINE.totalOwed}
          settled
          caption={`Repaid in full in ${COPPERLINE.repaidInDays} days of the ${COPPERLINE.plannedDays} planned`}
        />
      ) : null}

      {issue.state === 'Overdue' ? (
        <>
          <RepaymentBoard
            repaid={MARROWBONE.repaid}
            totalOwed={MARROWBONE.totalOwed}
            settled={false}
            caption={`Overdue by ${MARROWBONE.overdueDays} days \u00b7 ${amount(MARROWBONE.remaining)} USDC remaining`}
          />
          <div className="mt-8 grid gap-8 lg:grid-cols-2">
            <NoticePanel word="Overdue · share raised automatically" tone="red">
              Maturity passed {MARROWBONE.overdueDays} days ago with {amount(MARROWBONE.remaining)}{' '}
              USDC still outstanding, so the pledged share rose on its own from{' '}
              {MARROWBONE.shareBefore}% to the {MARROWBONE.shareNow}% ceiling. The amount owed did
              not change &mdash; only the speed at which fees are intercepted. The split still stops
              by itself once {amount(MARROWBONE.totalOwed)} USDC is repaid.
            </NoticePanel>
            <NoticePanel word="Signal · revenue pace has dropped" tone="dim">
              Fee revenue averaged {amount(MARROWBONE.revenueBefore)} USDC per day before the issue
              and {amount(MARROWBONE.revenueNow)} USDC per day now. A slower pace means a slower
              repayment; the cause may simply be the market rather than anything the issuer has
              done.
            </NoticePanel>
          </div>
        </>
      ) : null}

      {issue.state === 'Undersubscribed' ? (
        <div className="border-y border-board-ink py-6">
          <div className="mb-4 text-[10px] uppercase tracking-[0.28em] text-board-dim">
            Raised before the window closed
          </div>
          <div className="flex flex-wrap items-end gap-3">
            <SplitFlap value={amount(SALTMARSH.raised)} size="xl" tone="text-board-dim" />
            <span className="pb-2 text-[12px] uppercase tracking-[0.16em] text-board-dim">
              USDC of {amount(SALTMARSH.target)} USDC
            </span>
          </div>
          <div className="mt-5">
            <ProgressBar pct={SALTMARSH.raisedPct} tone="faint" height={10} />
            <div className="mt-2 flex justify-between text-[11px] uppercase tracking-[0.16em] text-board-dim">
              <span>Window closed {SALTMARSH.windowClosed}</span>
              <span>{percent(SALTMARSH.raisedPct)} of target</span>
            </div>
          </div>
          <div className="mt-6">
            <NoticePanel word="Undersubscribed · deposits returned" tone="dim">
              The book did not reach its target before the window closed, so the issue was never
              struck. Every deposit is being returned in full &mdash; no fee was taken, no bond
              units were minted and no obligation exists for {issue.issuer}. Nothing will be split
              from its revenue.
            </NoticePanel>
            <div className="mt-4">
              <BoardButton variant="line">Reclaim deposit</BoardButton>
            </div>
          </div>
        </div>
      ) : null}

      {/* ---------------- terms + revenue ---------------- */}

      <div className="mt-10 grid gap-10 lg:grid-cols-2">
        <Section title="Bond terms">
          <DataRow label="Face value" value={`${amount(issue.face)} USDC`} />
          <DataRow label="Coupon" value={percent(issue.couponPct)} />
          <DataRow
            label="Total owed"
            value={issue.totalOwed === null ? '\u2014' : `${amount(issue.totalOwed)} USDC`}
          />
          <DataRow label="Term" value={`${issue.termDays} days`} />
          <DataRow label="Revenue share" value={issue.shareLabel} />
          <DataRow label="Maturity" value={issue.matures} />
          <DataRow
            label={<InfoTip text={COVERAGE_NOTE}>Coverage ratio</InfoTip>}
            value={issue.coverage === null ? '\u2014' : ratio(issue.coverage)}
            tone={issue.coverage === null ? 'dim' : 'ink'}
          />
        </Section>

        <Section
          title="Revenue"
          aside={isQuillfin ? 'Last 30 days \u00b7 daily fee revenue' : undefined}
        >
          {isQuillfin ? (
            <>
              <RevenueChart data={REVENUE_30D} />
              <div className="mt-4">
                <DataRow
                  label="Average revenue"
                  value={`${amount(QUILLFIN.avgDailyRevenue)} USDC per day`}
                />
                <DataRow
                  label={`Through the split \u00b7 ${QUILLFIN.sharePct}%`}
                  value={`${amount(QUILLFIN.dailyToHolders)} USDC per day`}
                  tone="accent"
                />
                <DataRow label="Remaining" value={`${amount(QUILLFIN.remaining)} USDC`} />
                <DataRow
                  label="At the current pace"
                  value={`Released in about ${QUILLFIN.daysToRelease} days`}
                />
              </div>
            </>
          ) : issue.state === 'Subscribing' ? (
            <>
              <DataRow
                label="Average revenue"
                value={`${amount(TANGLEWOOD.avgDailyRevenue)} USDC per day`}
              />
              <DataRow
                label={<InfoTip text={COVERAGE_NOTE}>Coverage ratio</InfoTip>}
                value={ratio(TANGLEWOOD.coverage)}
              />
              <DataRow
                label="Total owed once struck"
                value={`${amount(TANGLEWOOD.totalOwed)} USDC`}
              />
            </>
          ) : issue.state === 'Repaid' ? (
            <>
              <DataRow label="Total owed" value={`${amount(COPPERLINE.totalOwed)} USDC`} />
              <DataRow
                label="Repaid in"
                value={`${COPPERLINE.repaidInDays} days of ${COPPERLINE.plannedDays}`}
              />
              <DataRow label="Revenue stream released" value={COPPERLINE.releasedOn} />
            </>
          ) : issue.state === 'Overdue' ? (
            <>
              <DataRow
                label="Revenue before the issue"
                value={`${amount(MARROWBONE.revenueBefore)} USDC per day`}
              />
              <DataRow
                label="Revenue now"
                value={`${amount(MARROWBONE.revenueNow)} USDC per day`}
                tone="red"
              />
              <DataRow label="Repaid" value={`${amount(MARROWBONE.repaid)} USDC`} />
              <DataRow label="Remaining" value={`${amount(MARROWBONE.remaining)} USDC`} />
            </>
          ) : (
            <>
              <DataRow label="Raised" value={`${amount(SALTMARSH.raised)} USDC`} />
              <DataRow label="Target" value={`${amount(SALTMARSH.target)} USDC`} />
              <DataRow label="Obligation created" value="None" tone="dim" />
            </>
          )}
        </Section>
      </div>

      <Section title="Your position" className="mt-10">
        <HolderPosition position={position} issuer={issue.issuer} />
      </Section>

      <Section
        title="Secondary market"
        className="mt-10"
        aside="Bonds are tradable before maturity"
      >
        <OffersPanel offers={offers} issuer={issue.issuer} />
      </Section>
    </div>
  );
};

export default IssueDetail;
