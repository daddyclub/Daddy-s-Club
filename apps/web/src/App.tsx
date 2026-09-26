import { Navigate, Route, Routes, useLocation } from 'react-router-dom';
import Header from '@/components/Header';
import IssueDetail from '@/pages/IssueDetail';
import IssuerDashboard from '@/pages/IssuerDashboard';
import Marketplace from '@/pages/Marketplace';
import NotFound from '@/pages/NotFound';
import IssueRoute from '@/routes/issue';
import OffersRoute from '@/routes/offers';

/**
 * Підпис унизу сторінки. Демо-дошка M0 стоїть на вигаданих числах і мусить це
 * казати; живий екран читає акаунт із ланцюга, і той самий підпис на ньому був
 * би просто неправдою.
 */
const Footnote = () => {
  const live = useLocation().pathname.startsWith('/live/');
  return (
    <span>
      {live
        ? 'Every figure on this screen is read from the issue account on chain'
        : 'All figures shown are demonstration data'}
    </span>
  );
};

const App = () => (
  <div className="flex min-h-full flex-col bg-board-bg text-board-ink">
    <Header />
    <main className="flex-1">
      <Routes>
        <Route path="/" element={<Marketplace />} />
        <Route path="/issue/:id" element={<IssueDetail />} />
        <Route path="/issuer" element={<Navigate to="/issuer/quillfin-swap" replace />} />
        <Route path="/issuer/:id" element={<IssuerDashboard />} />
        {/* Живий екран на стані ланцюга. Демо-дошка вище лишається на вигаданих
            числах, тому шляхи розведені; злиття — на T045. */}
        <Route path="/live/issue" element={<IssueRoute />} />
        <Route path="/live/issue/:address" element={<IssueRoute />} />
        <Route path="/live/issue/:address/offers" element={<OffersRoute />} />
        <Route path="*" element={<NotFound />} />
      </Routes>
    </main>
    <footer className="mt-12 border-t border-board-ink">
      <div className="mx-auto flex w-full max-w-[1400px] flex-wrap items-center justify-between gap-3 px-5 py-4 text-[10px] uppercase tracking-[0.16em] text-board-dim">
        <span>Daddy&apos;s Club &middot; bonds against future fee revenue</span>
        <Footnote />
      </div>
    </footer>
  </div>
);

export default App;
