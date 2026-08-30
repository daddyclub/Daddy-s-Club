import { useEffect, useRef, useState } from 'react';

type FlapSize = 'xl' | 'lg' | 'md' | 'sm';

const SIZE_CLASS: Record<FlapSize, string> = {
  xl: 'text-[clamp(2.6rem,8vw,5.2rem)] w-[0.68em] h-[1.22em]',
  lg: 'text-[clamp(1.5rem,3.4vw,2.5rem)] w-[0.68em] h-[1.24em]',
  md: 'text-[1.125rem] w-[0.7em] h-[1.3em]',
  sm: 'text-[0.8125rem] w-[0.72em] h-[1.34em]',
};

const SEP_CLASS: Record<FlapSize, string> = {
  xl: 'text-[clamp(2.6rem,8vw,5.2rem)] w-[0.32em]',
  lg: 'text-[clamp(1.5rem,3.4vw,2.5rem)] w-[0.3em]',
  md: 'text-[1.125rem] w-[0.32em]',
  sm: 'text-[0.8125rem] w-[0.34em]',
};

type Phase = 'idle' | 'out' | 'in';

interface FlapCellProps {
  char: string;
  size: FlapSize;
  tone: string;
}

/** One digit cell. It only moves when its own character changes. */
const FlapCell = ({ char, size, tone }: FlapCellProps) => {
  const [shown, setShown] = useState(char);
  const [phase, setPhase] = useState<Phase>('idle');
  const pending = useRef(char);

  useEffect(() => {
    pending.current = char;
    if (char === shown) return;
    setPhase('out');
    const drop = window.setTimeout(() => {
      setShown(pending.current);
      setPhase('in');
    }, 80);
    const settle = window.setTimeout(() => setPhase('idle'), 195);
    return () => {
      window.clearTimeout(drop);
      window.clearTimeout(settle);
    };
  }, [char, shown]);

  const faceClass =
    phase === 'out'
      ? 'flap-face flap-face-out'
      : phase === 'in'
        ? 'flap-face flap-face-in'
        : 'flap-face';

  return (
    <span
      className={`relative inline-flex items-center justify-center border border-board-rule bg-board-cell tabular-nums leading-none tracking-tight ${SIZE_CLASS[size]} ${tone}`}
      style={{ perspective: '260px' }}
    >
      <span className={faceClass}>{shown}</span>
      <span className="pointer-events-none absolute inset-x-0 top-1/2 h-px bg-board-rule" />
    </span>
  );
};

interface SeparatorProps {
  char: string;
  size: FlapSize;
}

/** A comma, dot, colon or space between cells. Never flips. */
const Separator = ({ char, size }: SeparatorProps) => (
  <span
    className={`inline-flex items-end justify-center pb-[0.18em] leading-none text-board-dim ${SEP_CLASS[size]}`}
  >
    {char === ' ' ? '' : char}
  </span>
);

interface SplitFlapProps {
  value: string;
  size?: FlapSize;
  tone?: string;
  label?: string;
}

/**
 * Renders a string as a row of flip cells. Anything that is not a
 * digit separator (comma, dot, colon, space) gets its own cell.
 */
const SplitFlap = ({ value, size = 'md', tone = 'text-board-ink', label }: SplitFlapProps) => {
  const chars = value.split('');
  return (
    <span className="inline-flex items-end gap-px" aria-label={label ?? value} role="img">
      {chars.map((char, index) => {
        // A cell's identity is its slot on the board, not its character: keying by
        // content would re-mount cells on every tick and kill the flip animation.
        if (char === ',' || char === '.' || char === ':' || char === ' ') {
          // biome-ignore lint/suspicious/noArrayIndexKey: the slot is the identity
          return <Separator key={`sep-${index}`} char={char} size={size} />;
        }
        // biome-ignore lint/suspicious/noArrayIndexKey: the slot is the identity
        return <FlapCell key={`cell-${index}`} char={char} size={size} tone={tone} />;
      })}
    </span>
  );
};

export default SplitFlap;
