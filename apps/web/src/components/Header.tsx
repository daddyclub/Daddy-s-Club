import { useEffect, useMemo, useState } from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import { PROTOCOL_SETTINGS, TODAY } from '@/data/mock';
import { useProtocolConfig } from '@/hooks/useMarket';
import { webEnv } from '@/lib/env';
import { bps, calendarDay, clock, percent } from '@/lib/format';
import { sharedFeed } from '@/lib/rpc';
import SplitFlap from './SplitFlap';

const NAV = [
  { to: '/', label: 'Marketplace', end: true },
  { to: '/issue/quillfin-swap', label: 'Issue detail', end: false },
  { to: '/issuer/quillfin-swap', label: 'Issuer desk', end: false },
  { to: '/live/issue', label: 'Live issue', end: false },
];

/** The demo board's fixed day and settings: they belong to its made-up figures. */
const DemoFees = () => (
  <>
    <span>Origination fee {percent(PROTOCOL_SETTINGS.originationFeePct)}</span>
    <span>Secondary trading fee {percent(PROTOCOL_SETTINGS.secondaryFeePct)}</span>
    <span>Maximum revenue share {percent(PROTOCOL_SETTINGS.maxRevenueSharePct)}</span>
  </>
);

/** On a live screen the fees are the protocol config on chain, as the program charges them. */
const LiveFees = () => {
  const env = webEnv();
  const feed = useMemo(() => sharedFeed(env.rpcUrl), [env.rpcUrl]);
  const state = useProtocolConfig(feed, env.programId);
  if (state.status === 'loading') return <span>Protocol fees &hellip;</span>;
  if (state.status === 'failed') return <span>Protocol fees unavailable</span>;
  const { config } = state;
  return (
    <>
      <span>Origination fee {bps(config.originationFeeBps)}</span>
      <span>Secondary trading fee {bps(config.tradingFeeBps)}</span>
      <span>Maximum revenue share {bps(config.maxPledgeBps)}</span>
    </>
  );
};

const Header = () => {
  const [now, setNow] = useState(() => new Date());
  // Same split as the footnote in `App.tsx`: live screens read the chain and today's date.
  const live = useLocation().pathname.startsWith('/live/');

  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 1000);
    return () => window.clearInterval(id);
  }, []);

  const time = clock(now);

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
              {live ? calendarDay(now) : TODAY} &middot; board time
            </div>
            <SplitFlap value={time} size="sm" label={`Board time ${time}`} />
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
            {live ? <LiveFees /> : <DemoFees />}
          </div>
        </div>
      </div>
    </header>
  );
};

export default Header;
