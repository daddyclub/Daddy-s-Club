import type { ReactNode } from 'react';

interface NoticePanelProps {
  word: string;
  tone?: 'ink' | 'accent' | 'red' | 'dim';
  children: ReactNode;
}

const TONE: Record<'ink' | 'accent' | 'red' | 'dim', string> = {
  ink: 'border-board-ink text-board-ink',
  accent: 'border-board-accent text-board-accent',
  red: 'border-board-red text-board-red',
  dim: 'border-board-faint text-board-dim',
};

/**
 * Colour never carries meaning on its own — the tone is always
 * accompanied by the leading word.
 */
const NoticePanel = ({ word, tone = 'ink', children }: NoticePanelProps) => (
  <div className={`border-t-2 pt-2 ${TONE[tone]}`}>
    <div className="mb-1 text-[10px] uppercase tracking-[0.24em]">{word}</div>
    <div className="max-w-[68ch] text-[12px] leading-[1.7] text-board-ink">{children}</div>
  </div>
);

export default NoticePanel;
