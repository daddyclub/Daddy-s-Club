import { amount, percent } from '@/lib/format';
import ProgressBar from './ProgressBar';
import SplitFlap from './SplitFlap';

interface RepaymentBoardProps {
  repaid: number;
  totalOwed: number;
  settled: boolean;
  caption: string;
}

const RepaymentBoard = ({ repaid, totalOwed, settled, caption }: RepaymentBoardProps) => {
  const remaining = Math.max(0, totalOwed - repaid);
  const pct = (repaid / totalOwed) * 100;

  return (
    <div className="border-y border-board-ink py-6">
      <div className="mb-4 flex flex-wrap items-baseline justify-between gap-3">
        <span className="text-[10px] uppercase tracking-[0.28em] text-board-dim">
          Repaid to bondholders
        </span>
        <span className="flex items-center gap-2 text-[10px] uppercase tracking-[0.2em]">
          <span
            className={`inline-block h-[7px] w-[7px] ${settled ? 'bg-board-ink' : 'bg-board-accent'}`}
          />
          <span className={settled ? 'text-board-ink' : 'text-board-accent'}>
            {settled ? 'Settled' : 'Live \u00b7 splitting fees'}
          </span>
        </span>
      </div>

      {settled ? (
        <div className="board-settle border border-board-ink bg-board-cell px-5 py-8">
          <div className="text-[clamp(1.4rem,4vw,2.6rem)] uppercase tracking-[0.2em]">
            Revenue stream released
          </div>
          <p className="mt-3 max-w-[64ch] text-[12px] leading-[1.7] text-board-dim">
            Face value and coupon are repaid in full. The split has stopped by itself and 100% of
            fees return to the issuer.
          </p>
        </div>
      ) : (
        <SplitFlap value={amount(repaid)} size="xl" label={`${amount(repaid)} USDC repaid`} />
      )}

      <div className="mt-5 grid gap-4 md:grid-cols-[1fr_auto] md:items-end">
        <div>
          <ProgressBar pct={pct} tone={settled ? 'ink' : 'accent'} height={10} />
          <div className="mt-2 flex flex-wrap justify-between gap-3 text-[11px] uppercase tracking-[0.16em] text-board-dim">
            <span>{caption}</span>
            <span>{percent(pct)} of obligation</span>
          </div>
        </div>
        <div className="flex gap-8 md:justify-end">
          <div>
            <div className="mb-1 text-[10px] uppercase tracking-[0.16em] text-board-dim">
              Total owed
            </div>
            <div className="text-[15px] tabular-nums tracking-tight">{amount(totalOwed)} USDC</div>
          </div>
          <div>
            <div className="mb-1 text-[10px] uppercase tracking-[0.16em] text-board-dim">
              Remaining
            </div>
            <div className="text-[15px] tabular-nums tracking-tight">
              <SplitFlap value={amount(remaining)} size="md" />
              <span className="ml-2 text-[11px] text-board-dim">USDC</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

export default RepaymentBoard;
