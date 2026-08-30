interface ProgressBarProps {
  pct: number;
  tone?: 'accent' | 'red' | 'ink' | 'faint';
  height?: number;
}

const FILL: Record<'accent' | 'red' | 'ink' | 'faint', string> = {
  accent: 'bg-board-accent',
  red: 'bg-board-red',
  ink: 'bg-board-ink',
  faint: 'bg-board-faint',
};

const ProgressBar = ({ pct, tone = 'accent', height = 6 }: ProgressBarProps) => {
  const clamped = Math.max(0, Math.min(100, pct));
  return (
    <div
      className="relative w-full border border-board-rule bg-board-cell"
      style={{ height: `${height}px` }}
    >
      <div className={`h-full ${FILL[tone]}`} style={{ width: `${clamped}%` }} />
      <div className="pointer-events-none absolute inset-y-0 left-1/4 w-px bg-board-bg/70" />
      <div className="pointer-events-none absolute inset-y-0 left-1/2 w-px bg-board-bg/70" />
      <div className="pointer-events-none absolute inset-y-0 left-3/4 w-px bg-board-bg/70" />
    </div>
  );
};

export default ProgressBar;
