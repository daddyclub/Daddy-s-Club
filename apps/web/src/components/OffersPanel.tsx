import { useMemo, useState } from 'react';
import type { Offer } from '@/data/mock';
import { PROTOCOL_SETTINGS } from '@/data/mock';
import { amount, units as fmtUnits, percent, price } from '@/lib/format';
import BoardButton from './BoardButton';
import SplitFlap from './SplitFlap';

interface OffersPanelProps {
  offers: Offer[];
  issuer: string;
}

const GRID = 'grid grid-cols-[1fr_120px_140px_110px] gap-4 items-center';

/**
 * Keeps a decimal field locale-independent: `type="number"` both renders and
 * parses with the browser's decimal separator, so on a comma locale "0.93"
 * cannot be typed at all. Accept either separator, store a dot.
 */
const toDecimal = (raw: string): string => {
  const dotted = raw.replace(',', '.');
  const [whole = '', ...rest] = dotted.split('.');
  const digits = whole.replace(/[^\d]/g, '');
  if (rest.length === 0) return digits;
  return `${digits}.${rest.join('').replace(/[^\d]/g, '')}`;
};

const OffersPanel = ({ offers, issuer }: OffersPanelProps) => {
  const [bought, setBought] = useState<string | null>(null);
  const [listUnits, setListUnits] = useState('5000');
  const [listPrice, setListPrice] = useState('0.930');
  const [listed, setListed] = useState(false);

  const quote = useMemo(() => {
    const u = Number.parseFloat(listUnits);
    const p = Number.parseFloat(listPrice);
    if (Number.isNaN(u) || Number.isNaN(p) || u <= 0 || p <= 0) {
      return { gross: 0, fee: 0, net: 0, valid: false };
    }
    const gross = u * p;
    const fee = (gross * PROTOCOL_SETTINGS.secondaryFeePct) / 100;
    return { gross, fee, net: gross - fee, valid: true };
  }, [listUnits, listPrice]);

  return (
    <div className="grid gap-10 lg:grid-cols-[1.35fr_1fr]">
      <div>
        <div
          className={`${GRID} border-b border-board-ink pb-2 text-[10px] uppercase tracking-[0.16em] text-board-dim`}
        >
          <span>Units offered</span>
          <span className="text-right">Price / unit</span>
          <span className="text-right">Total</span>
          <span className="text-right">Action</span>
        </div>

        {offers.length === 0 ? (
          <div className="border-b border-board-rule py-8 text-[12px] leading-[1.7] text-board-dim">
            No open offers on {issuer} bonds. The secondary book is empty until a holder lists
            units.
          </div>
        ) : (
          offers.map((offer) => (
            <div key={offer.id} className={`${GRID} border-b border-board-rule py-[12px]`}>
              <span className="text-[13px] tabular-nums tracking-tight">
                {fmtUnits(offer.units)} units
              </span>
              <span className="text-right text-[13px] tabular-nums">
                {price(offer.pricePerUnit)} USDC
              </span>
              <span className="text-right text-[13px] tabular-nums">
                {amount(offer.units * offer.pricePerUnit)} USDC
              </span>
              <span className="flex justify-end">
                {bought === offer.id ? (
                  <span className="text-[10px] uppercase tracking-[0.16em] text-board-accent">
                    Filled
                  </span>
                ) : (
                  <BoardButton variant="line" onClick={() => setBought(offer.id)}>
                    Buy
                  </BoardButton>
                )}
              </span>
            </div>
          ))
        )}

        <div className="mt-3 text-[10px] uppercase tracking-[0.16em] text-board-dim">
          Secondary trading fee {percent(PROTOCOL_SETTINGS.secondaryFeePct)} deducted from seller
          proceeds
        </div>
      </div>

      <div className="border-t border-board-ink pt-4 lg:border-t-0 lg:border-l lg:border-board-rule lg:pl-8 lg:pt-0">
        <div className="mb-4 text-[11px] uppercase tracking-[0.24em]">List your units for sale</div>

        <label className="mb-4 block">
          <span className="mb-1 block text-[10px] uppercase tracking-[0.16em] text-board-dim">
            Units
          </span>
          <input
            type="text"
            inputMode="numeric"
            value={listUnits}
            onChange={(event) => {
              setListUnits(event.target.value.replace(/[^\d]/g, ''));
              setListed(false);
            }}
            className="w-full border border-board-rule bg-board-cell px-2 py-[7px] text-[13px] tabular-nums tracking-tight outline-none focus:border-board-accent"
          />
        </label>

        <label className="mb-5 block">
          <span className="mb-1 block text-[10px] uppercase tracking-[0.16em] text-board-dim">
            Price per unit (USDC)
          </span>
          <input
            type="text"
            inputMode="decimal"
            value={listPrice}
            onChange={(event) => {
              setListPrice(toDecimal(event.target.value));
              setListed(false);
            }}
            className="w-full border border-board-rule bg-board-cell px-2 py-[7px] text-[13px] tabular-nums tracking-tight outline-none focus:border-board-accent"
          />
        </label>

        <div className="flex items-baseline justify-between border-b border-board-rule py-[7px] text-[12px] tabular-nums">
          <span className="text-[10px] uppercase tracking-[0.16em] text-board-dim">Gross</span>
          <span>{amount(quote.gross)} USDC</span>
        </div>
        <div className="flex items-baseline justify-between border-b border-board-rule py-[7px] text-[12px] tabular-nums">
          <span className="text-[10px] uppercase tracking-[0.16em] text-board-dim">
            Trading fee {percent(PROTOCOL_SETTINGS.secondaryFeePct)}
          </span>
          <span>&minus;{amount(quote.fee)} USDC</span>
        </div>
        <div className="flex items-end justify-between gap-3 border-b border-board-ink py-3">
          <span className="text-[10px] uppercase tracking-[0.16em] text-board-dim">
            You receive
          </span>
          <span className="flex items-end gap-2">
            <SplitFlap value={amount(quote.net)} size="md" />
            <span className="text-[10px] uppercase tracking-[0.16em] text-board-dim">USDC</span>
          </span>
        </div>

        <div className="mt-4 flex items-center justify-between gap-4">
          <BoardButton onClick={() => setListed(true)} disabled={!quote.valid}>
            {listed ? 'Listed' : 'List units'}
          </BoardButton>
          {listed ? (
            <span className="text-[10px] uppercase tracking-[0.16em] text-board-accent">
              Offer posted to the book
            </span>
          ) : null}
        </div>
      </div>
    </div>
  );
};

export default OffersPanel;
