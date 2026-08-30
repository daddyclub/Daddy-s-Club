import type { IssueState } from '@/data/mock';

const TONE: Record<IssueState, string> = {
  Subscribing: 'border-board-accent text-board-accent',
  Repaying: 'border-board-accent text-board-accent',
  Repaid: 'border-board-ink text-board-ink',
  Overdue: 'border-board-red text-board-red',
  Undersubscribed: 'border-board-faint text-board-dim',
};

const MARK: Record<IssueState, string> = {
  Subscribing: 'OPEN',
  Repaying: 'LIVE',
  Repaid: 'DONE',
  Overdue: 'LATE',
  Undersubscribed: 'CLOSED',
};

interface StateChipProps {
  state: IssueState;
  withMark?: boolean;
}

const StateChip = ({ state, withMark = false }: StateChipProps) => (
  <span
    className={`inline-flex items-center gap-2 border px-2 py-[3px] text-[11px] uppercase leading-none tracking-[0.14em] ${TONE[state]}`}
  >
    {withMark ? (
      <span className="text-[9px] tracking-[0.2em] opacity-70">{MARK[state]}</span>
    ) : null}
    {state}
  </span>
);

export default StateChip;
