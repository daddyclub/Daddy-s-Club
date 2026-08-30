import type { ReactNode } from 'react';

interface InfoTipProps {
  text: string;
  children: ReactNode;
}

/** Hairline tooltip. Hover or keyboard focus, no library. */
const InfoTip = ({ text, children }: InfoTipProps) => (
  <span className="group relative inline-flex items-center gap-1">
    {children}
    <button
      type="button"
      aria-label={text}
      className="border border-board-faint px-[3px] text-[9px] leading-[12px] text-board-dim outline-none focus-visible:border-board-ink focus-visible:text-board-ink"
    >
      ?
    </button>
    <span
      role="tooltip"
      className="pointer-events-none absolute bottom-full left-0 z-20 mb-2 hidden w-[290px] border border-board-ink bg-board-bg p-2 text-[11px] normal-case leading-[1.5] tracking-normal text-board-ink group-focus-within:block group-hover:block"
    >
      {text}
    </span>
  </span>
);

export default InfoTip;
