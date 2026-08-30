import { useEffect, useRef, useState } from 'react';
import { TAPE_SEED_SPLITS, type TapeRow } from '@/data/mock';
import { clock } from '@/lib/format';

const round2 = (value: number): number => Math.round(value * 100) / 100;

const MAX_ROWS = 8;

interface FeeStream {
  repaid: number;
  rows: TapeRow[];
  settled: boolean;
}

const buildSeed = (start: number, sharePct: number): TapeRow[] => {
  const base = Date.now();
  let running = start;
  return TAPE_SEED_SPLITS.map((split, index) => {
    const row: TapeRow = {
      id: -(index + 1),
      time: clock(new Date(base - (index + 1) * 3200)),
      feeEarned: round2(split / (sharePct / 100)),
      toHolders: split,
      runningTotal: round2(running),
    };
    running = round2(running - split);
    return row;
  });
};

/**
 * Simulates fees arriving every 2 to 4 seconds. Each fee is split at
 * `sharePct` and pushed onto the tape until the obligation is met.
 */
export const useFeeStream = (
  start: number,
  totalOwed: number,
  sharePct: number,
  enabled: boolean,
): FeeStream => {
  const [repaid, setRepaid] = useState(start);
  const [rows, setRows] = useState<TapeRow[]>(() => (enabled ? buildSeed(start, sharePct) : []));
  const repaidRef = useRef(start);
  const seqRef = useRef(0);

  useEffect(() => {
    if (!enabled) return;
    let timer = 0;

    const tick = (): void => {
      const previous = repaidRef.current;
      if (previous >= totalOwed) return;

      const raw = 40 + Math.random() * 220;
      const next = Math.min(totalOwed, round2(previous + raw));
      const split = round2(next - previous);
      const feeEarned = round2(split / (sharePct / 100));

      repaidRef.current = next;
      seqRef.current += 1;

      setRepaid(next);
      setRows((current) =>
        [
          {
            id: seqRef.current,
            time: clock(new Date()),
            feeEarned,
            toHolders: split,
            runningTotal: next,
          },
          ...current,
        ].slice(0, MAX_ROWS),
      );

      if (next < totalOwed) schedule();
    };

    const schedule = (): void => {
      timer = window.setTimeout(tick, 2000 + Math.random() * 2000);
    };

    schedule();
    return () => window.clearTimeout(timer);
  }, [enabled, sharePct, totalOwed]);

  return { repaid, rows, settled: repaid >= totalOwed };
};
