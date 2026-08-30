import { useState } from 'react';
import type { Position } from '@/data/mock';
import { amount, units as fmtUnits } from '@/lib/format';
import BoardButton from './BoardButton';
import DataRow from './DataRow';
import SplitFlap from './SplitFlap';

interface HolderPositionProps {
  position: Position | undefined;
  issuer: string;
}

const HolderPosition = ({ position, issuer }: HolderPositionProps) => {
  const [claimed, setClaimed] = useState(false);

  if (position === undefined) {
    return (
      <div className="border-b border-board-rule py-6">
        <div className="text-[12px] uppercase tracking-[0.2em] text-board-dim">No position</div>
        <p className="mt-2 max-w-[62ch] text-[12px] leading-[1.7] text-board-dim">
          This wallet holds no {issuer} bond units. Nothing is claimable and nothing is owed to you.
          Units bought on the secondary market appear here immediately.
        </p>
      </div>
    );
  }

  return (
    <div>
      <DataRow label="Units held" value={fmtUnits(position.units)} />
      <DataRow label="Claimed to date" value={`${amount(position.claimedToDate)} USDC`} />
      <div className="flex flex-wrap items-end justify-between gap-4 border-b border-board-rule py-4">
        <div>
          <div className="mb-2 text-[10px] uppercase tracking-[0.16em] text-board-dim">
            Claimable now
          </div>
          <div className="flex items-end gap-2">
            <SplitFlap
              value={amount(claimed ? 0 : position.claimableNow)}
              size="lg"
              tone="text-board-accent"
            />
            <span className="pb-1 text-[11px] uppercase tracking-[0.16em] text-board-dim">
              USDC
            </span>
          </div>
        </div>
        <div className="text-right">
          <BoardButton onClick={() => setClaimed(true)} disabled={claimed}>
            {claimed ? 'Claimed' : 'Claim'}
          </BoardButton>
          {claimed ? (
            <div className="mt-2 text-[10px] uppercase tracking-[0.16em] text-board-accent">
              Confirmed &middot; {amount(position.claimableNow)} USDC released to your wallet
            </div>
          ) : (
            <div className="mt-2 text-[10px] uppercase tracking-[0.16em] text-board-dim">
              Claim any time &middot; no deadline
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default HolderPosition;
