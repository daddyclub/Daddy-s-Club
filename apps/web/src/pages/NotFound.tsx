import { Link } from 'react-router-dom';

const NotFound = () => (
  <div className="mx-auto w-full max-w-[1400px] px-5 py-20">
    <div className="border-b border-board-ink pb-3 text-[13px] uppercase tracking-[0.28em]">
      Not on the board
    </div>
    <p className="mt-4 max-w-[62ch] text-[12px] leading-[1.7] text-board-dim">
      This departure does not exist. Head back to the marketplace to see every open, repaying and
      settled issue.
    </p>
    <Link
      to="/"
      className="mt-5 inline-block border-b border-board-accent pb-[2px] text-[11px] uppercase tracking-[0.2em] text-board-accent"
    >
      Back to marketplace
    </Link>
  </div>
);

export default NotFound;
