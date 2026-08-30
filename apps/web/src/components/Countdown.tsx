import { useEffect, useState } from 'react';
import SplitFlap from './SplitFlap';

interface CountdownProps {
  seconds: number;
  label: string;
}

const pad = (value: number): string => value.toString().padStart(2, '0');

const Countdown = ({ seconds, label }: CountdownProps) => {
  const [left, setLeft] = useState(seconds);

  useEffect(() => {
    const id = window.setInterval(() => {
      setLeft((prev) => (prev <= 0 ? 0 : prev - 1));
    }, 1000);
    return () => window.clearInterval(id);
  }, []);

  const days = Math.floor(left / 86400);
  const hours = Math.floor((left % 86400) / 3600);
  const minutes = Math.floor((left % 3600) / 60);
  const secs = left % 60;

  return (
    <div>
      <div className="mb-2 text-[10px] uppercase tracking-[0.16em] text-board-dim">{label}</div>
      <div className="flex flex-wrap items-end gap-4">
        <div className="flex items-end gap-1">
          <SplitFlap value={pad(days)} size="lg" />
          <span className="pb-1 text-[11px] uppercase tracking-[0.16em] text-board-dim">d</span>
        </div>
        <div className="flex items-end gap-1">
          <SplitFlap value={pad(hours)} size="lg" />
          <span className="pb-1 text-[11px] uppercase tracking-[0.16em] text-board-dim">h</span>
        </div>
        <div className="flex items-end gap-1">
          <SplitFlap value={pad(minutes)} size="lg" />
          <span className="pb-1 text-[11px] uppercase tracking-[0.16em] text-board-dim">m</span>
        </div>
        <div className="flex items-end gap-1">
          <SplitFlap value={pad(secs)} size="lg" />
          <span className="pb-1 text-[11px] uppercase tracking-[0.16em] text-board-dim">s</span>
        </div>
      </div>
    </div>
  );
};

export default Countdown;
