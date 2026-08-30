import { useEffect, useState } from 'react';
import { NavLink } from 'react-router-dom';
import { PROTOCOL_SETTINGS, TODAY } from '@/data/mock';
import { clock, percent } from '@/lib/format';
import SplitFlap from './SplitFlap';

const NAV = [
  { to: '/', label: 'Marketplace', end: true },
  { to: '/issue/quillfin-swap', label: 'Issue detail', end: false },
  { to: '/issuer/quillfin-swap', label: 'Issuer desk', end: false },
];

const Header = () => {
  const [now, setNow] = useState(() => clock(new Date()));

  useEffect(() => {
    const id = window.setInterval(() => setNow(clock(new Date())), 1000);
    return () => window.clearInterval(id);
  }, []);

  return (
    <header className="border-b border-board-ink">
      <div className="mx-auto flex w-full max-w-[1400px] flex-wrap items-end justify-between gap-4 px-5 pb-3 pt-5">
        <div className="flex items-end gap-6">
          <NavLink to="/" className="leading-none">
            <span className="block text-[15px] font-bold uppercase tracking-[0.42em]">
              Daddy&apos;s Club
            </span>
            <span className="mt-2 block text-[10px] uppercase tracking-[0.24em] text-board-dim">
              Revenue bonds &middot; departures board
            </span>
          </NavLink>
        </div>

        <div className="flex items-end gap-6">
          <div className="text-right">
            <div className="mb-1 text-[10px] uppercase tracking-[0.24em] text-board-dim">
              {TODAY} &middot; board time
            </div>
            <SplitFlap value={now} size="sm" label={`Board time ${now}`} />
          </div>
        </div>
      </div>

      <div className="border-t border-board-rule">
        <div className="mx-auto flex w-full max-w-[1400px] flex-wrap items-center justify-between gap-x-8 gap-y-2 px-5 py-2">
          <nav className="flex flex-wrap items-center gap-x-7 gap-y-1">
            {NAV.map((item) => (
              <NavLink
                key={item.to}
                to={item.to}
                end={item.end}
                className={({ isActive }) =>
                  `border-b-2 pb-[2px] text-[11px] uppercase tracking-[0.2em] transition-colors ${
                    isActive
                      ? 'border-board-accent text-board-accent'
                      : 'border-transparent text-board-dim hover:text-board-ink'
                  }`
                }
              >
                {item.label}
              </NavLink>
            ))}
          </nav>
          <div className="flex flex-wrap items-center gap-x-6 gap-y-1 text-[10px] uppercase tracking-[0.16em] text-board-dim">
            <span>Origination fee {percent(PROTOCOL_SETTINGS.originationFeePct)}</span>
            <span>Secondary trading fee {percent(PROTOCOL_SETTINGS.secondaryFeePct)}</span>
            <span>Maximum revenue share {percent(PROTOCOL_SETTINGS.maxRevenueSharePct)}</span>
          </div>
        </div>
      </div>
    </header>
  );
};

export default Header;
