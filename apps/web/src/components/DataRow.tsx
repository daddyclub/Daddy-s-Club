import type { ReactNode } from 'react';

interface DataRowProps {
  label: ReactNode;
  value: ReactNode;
  tone?: 'ink' | 'accent' | 'red' | 'dim';
}

const TONE: Record<'ink' | 'accent' | 'red' | 'dim', string> = {
  ink: 'text-board-ink',
  accent: 'text-board-accent',
  red: 'text-board-red',
  dim: 'text-board-dim',
};

const DataRow = ({ label, value, tone = 'ink' }: DataRowProps) => (
  <div className="flex items-baseline justify-between gap-4 border-b border-board-rule py-[7px]">
    <span className="text-[10px] uppercase tracking-[0.16em] text-board-dim">{label}</span>
    <span className={`text-[13px] tabular-nums tracking-tight ${TONE[tone]}`}>{value}</span>
  </div>
);

export default DataRow;
