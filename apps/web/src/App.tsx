import { Navigate, Route, Routes } from 'react-router-dom';
import Header from '@/components/Header';
import IssueDetail from '@/pages/IssueDetail';
import IssuerDashboard from '@/pages/IssuerDashboard';
import Marketplace from '@/pages/Marketplace';
import NotFound from '@/pages/NotFound';

const App = () => (
  <div className="flex min-h-full flex-col bg-board-bg text-board-ink">
    <Header />
    <main className="flex-1">
      <Routes>
        <Route path="/" element={<Marketplace />} />
        <Route path="/issue/:id" element={<IssueDetail />} />
        <Route path="/issuer" element={<Navigate to="/issuer/quillfin-swap" replace />} />
        <Route path="/issuer/:id" element={<IssuerDashboard />} />
        <Route path="*" element={<NotFound />} />
      </Routes>
    </main>
    <footer className="mt-12 border-t border-board-ink">
      <div className="mx-auto flex w-full max-w-[1400px] flex-wrap items-center justify-between gap-3 px-5 py-4 text-[10px] uppercase tracking-[0.16em] text-board-dim">
        <span>Daddy&apos;s Club &middot; bonds against future fee revenue</span>
        <span>All figures shown are demonstration data</span>
      </div>
    </footer>
  </div>
);

export default App;
