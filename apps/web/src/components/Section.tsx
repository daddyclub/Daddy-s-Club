import type { ReactNode } from 'react';

interface SectionProps {
  title: string;
  aside?: ReactNode;
  children: ReactNode;
  className?: string;
}

const Section = ({ title, aside, children, className = '' }: SectionProps) => (
  <section className={`border-t border-board-ink pt-3 ${className}`}>
    <div className="mb-3 flex flex-wrap items-baseline justify-between gap-3">
      <h2 className="text-[11px] uppercase tracking-[0.28em] text-board-ink">{title}</h2>
      {aside === undefined ? null : (
        <div className="text-[10px] uppercase tracking-[0.16em] text-board-dim">{aside}</div>
      )}
    </div>
    {children}
  </section>
);

export default Section;
