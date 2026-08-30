import type { RevenueDay } from '@/data/mock';
import { amount } from '@/lib/format';

interface RevenueChartProps {
  data: RevenueDay[];
  /** when set, each bar is split into the bondholder portion and the kept portion */
  sharePct?: number;
  height?: number;
}

const RevenueChart = ({ data, sharePct, height = 168 }: RevenueChartProps) => {
  const max = data.reduce((acc, day) => (day.revenue > acc ? day.revenue : acc), 0);
  const gridLines = [0.25, 0.5, 0.75, 1];
  const first = data[0];
  const last = data[data.length - 1];

  return (
    <div className="w-full">
      <div className="relative border-b border-board-ink" style={{ height: `${height}px` }}>
        {gridLines.map((line) => (
          <div
            key={line}
            className="absolute inset-x-0 border-t border-dotted border-board-rule"
            style={{ bottom: `${line * 100}%` }}
          />
        ))}
        <div className="absolute inset-0 flex items-end gap-[2px]">
          {data.map((day) => {
            const total = (day.revenue / max) * 100;
            const holder = sharePct === undefined ? 0 : total * (sharePct / 100);
            const kept = total - holder;
            return (
              <div
                key={day.date}
                className="group relative flex h-full flex-1 flex-col justify-end"
                title={`${day.date} — ${amount(day.revenue)} USDC`}
              >
                <div
                  className="w-full bg-board-ink/80 group-hover:bg-board-ink"
                  style={{ height: `${kept}%` }}
                />
                {sharePct === undefined ? null : (
                  <div className="w-full bg-board-accent" style={{ height: `${holder}%` }} />
                )}
              </div>
            );
          })}
        </div>
      </div>
      <div className="mt-1 flex justify-between text-[10px] uppercase tracking-[0.12em] text-board-dim">
        <span>{first?.date ?? ''}</span>
        <span>peak {amount(max)} USDC</span>
        <span>{last?.date ?? ''}</span>
      </div>
    </div>
  );
};

export default RevenueChart;
