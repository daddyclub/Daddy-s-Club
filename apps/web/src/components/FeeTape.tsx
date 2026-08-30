import type { TapeRow } from '@/data/mock';
import { amount } from '@/lib/format';

interface FeeTapeProps {
  rows: TapeRow[];
  sharePct: number;
  live: boolean;
}

const FeeTape = ({ rows, sharePct, live }: FeeTapeProps) => (
  <div className="w-full">
    <div className="grid grid-cols-[88px_1fr_1fr_1fr] gap-4 border-b border-board-ink pb-[6px] text-[10px] uppercase tracking-[0.16em] text-board-dim">
      <span>Time</span>
      <span className="text-right">Fee earned</span>
      <span className="text-right">Split to bondholders &middot; {sharePct}%</span>
      <span className="text-right">Running total repaid</span>
    </div>
    {rows.length === 0 ? (
      <div className="py-6 text-[12px] text-board-dim">
        No fees intercepted yet. The tape starts as soon as the protocol earns.
      </div>
    ) : (
      rows.map((row, index) => (
        <div
          key={row.id}
          className={`grid grid-cols-[88px_1fr_1fr_1fr] gap-4 border-b border-board-rule py-[7px] text-[12px] tabular-nums tracking-tight ${
            index === 0 && live ? 'tape-enter text-board-ink' : 'text-board-ink'
          }`}
          style={{ opacity: 1 - index * 0.07 }}
        >
          <span className="text-board-dim">{row.time}</span>
          <span className="text-right">{amount(row.feeEarned)} USDC</span>
          <span className="text-right text-board-accent">+{amount(row.toHolders)} USDC</span>
          <span className="text-right">{amount(row.runningTotal)} USDC</span>
        </div>
      ))
    )}
  </div>
);

export default FeeTape;
