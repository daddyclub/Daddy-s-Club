/**
 * Вторинка випуску на живому стані ланцюга (`FR-024`…`FR-027`, `FR-035`).
 *
 * Стакан — вибірка акаунтів ринку з підписками (`lib/book-feed.ts`), без
 * бекенду й індексатора (`FR-023`). Продавець виставляє лот за суму USDC
 * (`FR-024`: ціну задає він, протокол її не рахує), покупець бере його
 * цілком, продавець скасовує своє. Гаманець лише підписує; відправляє
 * застосунок тим самим з'єднанням, на якому живуть підписки, — тому
 * «USDC прийшли» продавець бачить пушем власного рахунку (`SC-009`).
 *
 * Шлях `/live/issue/:address/offers` тимчасовий так само, як і картка: на
 * `T045` обидва переїдуть з-під `/live`.
 */

import {
  associatedTokenAddress,
  findFreeOfferNonce,
  type Issue,
  MARKET_PROGRAM_ID,
  type Offer,
  type ProtocolConfig,
  sellerProceeds,
  tradingFee,
} from '@daddys-club/sdk';
import { PublicKey, type TransactionInstruction } from '@solana/web3.js';
import { type FormEvent, useMemo, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import BoardButton from '@/components/BoardButton';
import DataRow from '@/components/DataRow';
import NoticePanel from '@/components/NoticePanel';
import Section from '@/components/Section';
import { useIssueAccount } from '@/hooks/useIssueAccount';
import { useBook, useProtocolConfig, useTokenBalance } from '@/hooks/useMarket';
import { useWallet } from '@/hooks/useWallet';
import { parseAmount, pricePerFace, showUnits } from '@/lib/amounts';
import type { BookEntry } from '@/lib/book-feed';
import { webEnv } from '@/lib/env';
import { bps } from '@/lib/format';
import {
  cancellationInstructions,
  explainFailure,
  listingInstructions,
  purchaseInstructions,
} from '@/lib/market-tx';
import { sharedConnection, sharedFeed, sharedProgramFeed } from '@/lib/rpc';
import { type Balance, spendable } from '@/lib/token-balance';

const short = (key: PublicKey): string => {
  const text = key.toBase58();
  return `${text.slice(0, 4)}…${text.slice(-4)}`;
};

const usdc = (value: bigint): string => `${showUnits(value)} USDC`;

/** Стан останньої дії. Одна дія за раз — друга кнопка чекає першу. */
type Action =
  | { readonly kind: 'idle' }
  | { readonly kind: 'busy'; readonly what: string }
  | { readonly kind: 'done'; readonly what: string; readonly signature: string }
  | { readonly kind: 'failed'; readonly what: string; readonly reason: string };

const balanceText = (balance: Balance): string => {
  switch (balance.status) {
    case 'loading':
      return '…';
    case 'live':
      return usdc(balance.amount);
    case 'missing':
      return 'no account yet';
    case 'foreign':
      return 'not a Token-2022 account';
    case 'unreachable':
      return 'node did not answer';
  }
};

const WalletBar = () => {
  const { wallets, wallet, publicKey, connecting, error, connect, disconnect } = useWallet();

  if (wallet !== null && publicKey !== null) {
    return (
      <div className="flex flex-wrap items-center gap-3 text-[11px] uppercase tracking-[0.16em]">
        <span className="text-board-dim">{wallet.name}</span>
        <span data-testid="wallet-address" className="break-all normal-case tracking-tight">
          {publicKey.toBase58()}
        </span>
        <BoardButton variant="line" onClick={disconnect}>
          Disconnect
        </BoardButton>
      </div>
    );
  }

  return (
    <div className="flex flex-wrap items-center gap-3">
      {wallets.length === 0 ? (
        <span className="text-[11px] uppercase tracking-[0.16em] text-board-dim">
          No Solana wallet found in this browser
        </span>
      ) : (
        wallets.map((candidate) => (
          <BoardButton
            key={candidate.name}
            data-testid={`connect-${candidate.name}`}
            disabled={connecting}
            onClick={() => void connect(candidate)}
          >
            Connect {candidate.name}
          </BoardButton>
        ))
      )}
      {error === null ? null : <span className="text-[11px] text-board-red">{error}</span>}
    </div>
  );
};

interface BookProps {
  entries: readonly BookEntry[];
  me: PublicKey | null;
  myUsdc: bigint | null;
  busy: boolean;
  onBuy(offer: Offer): void;
  onCancel(offer: Offer): void;
}

/**
 * На вузькому екрані лишаються номінал, ціна й дія: ціна за одиницю й продавець
 * — з `sm`. Колонки `minmax(0,1fr)`, бо звичайний `1fr` не стискається нижче
 * вмісту, і на 375 px таблиця розпирала сторінку вбік.
 */
const GRID =
  'grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_80px] items-center gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_110px_90px_110px]';

const OfferBook = ({ entries, me, myUsdc, busy, onBuy, onCancel }: BookProps) => (
  <div>
    <div
      className={`${GRID} border-b border-board-ink pb-2 text-[10px] uppercase tracking-[0.16em] text-board-dim`}
    >
      <span>Face</span>
      <span className="text-right">Price</span>
      <span className="hidden text-right sm:block">Per 1 face</span>
      <span className="hidden text-right sm:block">Seller</span>
      <span className="text-right">Action</span>
    </div>
    {entries.length === 0 ? (
      <div className="border-b border-board-rule py-8 text-[12px] leading-[1.7] text-board-dim">
        No open offers on this issue. The book fills when a holder lists a lot.
      </div>
    ) : (
      entries.map(({ address, offer }) => {
        const mine = me !== null && offer.seller.equals(me);
        const short_of_cash = myUsdc !== null && myUsdc < offer.price;
        return (
          <div
            key={address.toBase58()}
            data-testid="offer-row"
            data-offer={address.toBase58()}
            className={`${GRID} border-b border-board-rule py-[10px] text-[13px] tabular-nums`}
          >
            <span className="tracking-tight">{showUnits(offer.amount)}</span>
            <span className="text-right">{showUnits(offer.price)}</span>
            <span className="hidden text-right sm:block">
              {pricePerFace(offer.price, offer.amount).toFixed(4)}
            </span>
            <span className="hidden text-right text-[11px] text-board-dim sm:block">
              {mine ? 'you' : short(offer.seller)}
            </span>
            <span className="flex justify-end">
              {me === null ? (
                <span className="text-[10px] uppercase tracking-[0.16em] text-board-dim">
                  Connect
                </span>
              ) : mine ? (
                <BoardButton
                  variant="line"
                  data-testid="cancel"
                  disabled={busy}
                  onClick={() => onCancel(offer)}
                >
                  Cancel
                </BoardButton>
              ) : (
                <BoardButton
                  data-testid="buy"
                  disabled={busy || short_of_cash}
                  title={short_of_cash ? 'Not enough USDC for this lot' : undefined}
                  onClick={() => onBuy(offer)}
                >
                  Buy
                </BoardButton>
              )}
            </span>
          </div>
        );
      })
    )}
  </div>
);

interface ListFormProps {
  config: ProtocolConfig;
  bondBalance: bigint | null;
  connected: boolean;
  busy: boolean;
  onList(amount: bigint, price: bigint): void;
}

const ListForm = ({ config, bondBalance, connected, busy, onList }: ListFormProps) => {
  const [faceText, setFaceText] = useState('');
  const [priceText, setPriceText] = useState('');
  const face = parseAmount(faceText);
  const price = parseAmount(priceText);
  const fee = price === null ? null : tradingFee(price, config.tradingFeeBps);
  const proceeds = price === null ? null : sellerProceeds(price, config.tradingFeeBps);

  let refusal: string | null = null;
  if (faceText !== '' && face === null)
    refusal = 'Face amount is not a number with up to 6 decimals.';
  else if (priceText !== '' && price === null)
    refusal = 'Price is not a number with up to 6 decimals.';
  else if (face === 0n || price === 0n)
    refusal = 'Both the face amount and the price must be above zero.';
  else if (face !== null && bondBalance !== null && face > bondBalance)
    refusal = `You hold ${showUnits(bondBalance)} of face — the lot cannot be larger.`;

  const ready =
    connected &&
    !busy &&
    refusal === null &&
    face !== null &&
    price !== null &&
    face > 0n &&
    price > 0n;

  const submit = (event: FormEvent): void => {
    event.preventDefault();
    if (ready) onList(face, price);
  };

  const field =
    'w-full border border-board-rule bg-board-cell px-2 py-[7px] text-[13px] tabular-nums tracking-tight outline-none focus:border-board-accent';

  return (
    <form onSubmit={submit}>
      <label className="mb-4 block">
        <span className="mb-1 block text-[10px] uppercase tracking-[0.16em] text-board-dim">
          Face to sell (USDC of face)
        </span>
        <input
          data-testid="list-face"
          inputMode="decimal"
          value={faceText}
          onChange={(event) => setFaceText(event.target.value)}
          className={field}
        />
      </label>
      <label className="mb-5 block">
        <span className="mb-1 block text-[10px] uppercase tracking-[0.16em] text-board-dim">
          Total price for the lot (USDC)
        </span>
        <input
          data-testid="list-price"
          inputMode="decimal"
          value={priceText}
          onChange={(event) => setPriceText(event.target.value)}
          className={field}
        />
      </label>

      <DataRow
        label="Price per 1 face"
        value={
          face !== null && price !== null && face > 0n ? pricePerFace(price, face).toFixed(4) : '—'
        }
        tone="dim"
      />
      <DataRow
        label={`Trading fee ${bps(config.tradingFeeBps)}`}
        value={fee === null ? '—' : `−${usdc(fee)}`}
      />
      <DataRow label="You receive" value={proceeds === null ? '—' : usdc(proceeds)} tone="accent" />

      <div className="mt-4 flex flex-wrap items-center gap-3">
        <BoardButton type="submit" data-testid="list-submit" disabled={!ready}>
          List the lot
        </BoardButton>
        {refusal === null ? null : <span className="text-[11px] text-board-red">{refusal}</span>}
      </div>
      <p className="mt-4 max-w-[60ch] text-[11px] leading-[1.7] text-board-dim">
        The lot moves into the offer&apos;s escrow and stays there until someone buys it whole or
        you cancel. What the issue pays out while it stands is yours and comes back to you either
        way.
      </p>
    </form>
  );
};

const ActionNotice = ({ action }: { action: Action }) => {
  if (action.kind === 'idle') return null;
  return (
    <div data-testid="action-status" data-kind={action.kind} className="mt-8">
      {action.kind === 'busy' ? (
        <NoticePanel word={action.what} tone="dim">
          Waiting for the wallet signature and the cluster to confirm&hellip;
        </NoticePanel>
      ) : action.kind === 'done' ? (
        <NoticePanel word={`${action.what} — confirmed`} tone="accent">
          <span className="break-all">Signature {action.signature}</span>
        </NoticePanel>
      ) : (
        <NoticePanel word={`${action.what} — refused`} tone="red">
          {action.reason}
        </NoticePanel>
      )}
    </div>
  );
};

interface MarketProps {
  address: PublicKey;
  issue: Issue;
  config: ProtocolConfig;
}

const Market = ({ address, issue, config }: MarketProps) => {
  const env = webEnv();
  const feed = useMemo(() => sharedFeed(env.rpcUrl), [env.rpcUrl]);
  const program = useMemo(() => sharedProgramFeed(env.rpcUrl), [env.rpcUrl]);
  const connection = useMemo(() => sharedConnection(env.rpcUrl), [env.rpcUrl]);
  const wallet = useWallet();
  const me = wallet.publicKey;

  const book = useBook(program, feed, MARKET_PROGRAM_ID, address);
  const bondAccount = me === null ? null : associatedTokenAddress(me, issue.bondMint);
  const usdcAccount = me === null ? null : associatedTokenAddress(me, config.usdcMint);
  const bond = useTokenBalance(feed, bondAccount);
  const cash = useTokenBalance(feed, usdcAccount);
  const [action, setAction] = useState<Action>({ kind: 'idle' });
  const busy = action.kind === 'busy';

  const run = async (what: string, build: () => Promise<readonly TransactionInstruction[]>) => {
    setAction({ kind: 'busy', what });
    try {
      const signature = await wallet.submit(connection, await build());
      setAction({ kind: 'done', what, signature });
    } catch (error) {
      setAction({ kind: 'failed', what, reason: explainFailure(error) });
    }
  };

  const list = (amount: bigint, price: bigint) => {
    if (me === null) return;
    void run('Listing', async () =>
      listingInstructions({
        issue,
        config,
        seller: me,
        nonce: await findFreeOfferNonce(connection, address, me),
        amount,
        price,
      }),
    );
  };
  const buy = (offer: Offer) => {
    if (me === null) return;
    void run('Purchase', async () => purchaseInstructions({ issue, config, offer, buyer: me }));
  };
  const cancel = (offer: Offer) => {
    void run('Cancellation', async () => cancellationInstructions({ issue, config, offer }));
  };

  return (
    <>
      <div className="mb-8 border-b border-board-rule pb-4">
        <WalletBar />
        {me === null ? null : (
          <div className="mt-4 grid gap-x-10 sm:grid-cols-2">
            <DataRow
              label="Bond you hold (face)"
              value={bond.status === 'live' ? showUnits(bond.amount) : balanceText(bond)}
            />
            <div
              data-testid="usdc-balance"
              data-units={cash.status === 'live' ? cash.amount.toString() : ''}
            >
              <DataRow label="USDC on your account" value={balanceText(cash)} />
            </div>
          </div>
        )}
      </div>

      <div className="grid gap-10 lg:grid-cols-[1.35fr_1fr]">
        <Section
          className="min-w-0"
          title="Open offers"
          aside={`Fee ${bps(config.tradingFeeBps)} is taken from the seller · FR-035`}
        >
          {book.status === 'loading' ? (
            <div className="py-6 text-[12px] uppercase tracking-[0.28em] text-board-dim">
              Reading the book&hellip;
            </div>
          ) : book.status === 'unreachable' ? (
            <NoticePanel word="Node did not answer" tone="red">
              The book could not be read: {book.detail}. It is unknown, not empty.
            </NoticePanel>
          ) : (
            <OfferBook
              entries={book.entries}
              me={me}
              myUsdc={spendable(cash)}
              busy={busy}
              onBuy={buy}
              onCancel={cancel}
            />
          )}
        </Section>

        <Section className="min-w-0" title="List a lot for sale" aside="FR-024 · price is yours">
          <ListForm
            config={config}
            bondBalance={spendable(bond)}
            connected={me !== null}
            busy={busy}
            onList={list}
          />
        </Section>
      </div>

      <ActionNotice action={action} />
    </>
  );
};

const OffersScreen = ({ address }: { address: PublicKey }) => {
  const env = webEnv();
  const feed = useMemo(() => sharedFeed(env.rpcUrl), [env.rpcUrl]);
  const snapshot = useIssueAccount(feed, address, env.programId);
  const config = useProtocolConfig(feed, env.programId);

  let body: JSX.Element;
  if (snapshot.status === 'loading' || config.status === 'loading') {
    body = (
      <div className="py-10 text-[12px] uppercase tracking-[0.28em] text-board-dim">
        Reading the issue&hellip;
      </div>
    );
  } else if (snapshot.status !== 'live') {
    body = (
      <NoticePanel word="No issue to trade" tone="red">
        The issue at this address could not be read ({snapshot.status}). Open its card for the
        reason.
      </NoticePanel>
    );
  } else if (config.status === 'failed') {
    body = (
      <NoticePanel word="Protocol config unreadable" tone="red">
        {config.detail}. Without the trading fee and the currency the market cannot be shown
        honestly.
      </NoticePanel>
    );
  } else {
    body = <Market address={address} issue={snapshot.issue} config={config.config} />;
  }

  return (
    <div className="mx-auto w-full max-w-[1400px] px-5 py-8">
      <div className="mb-6 flex flex-wrap items-end justify-between gap-4 border-b border-board-ink pb-4">
        <div>
          <h1 className="text-[clamp(1.2rem,2.4vw,1.7rem)] uppercase tracking-[0.14em]">
            Offers · issue {short(address)}
          </h1>
          <div className="mt-3 break-all text-[11px] tracking-tight text-board-dim">
            {address.toBase58()}
          </div>
        </div>
        <Link
          to={`/live/issue/${address.toBase58()}`}
          className="border-b border-board-accent text-[11px] uppercase tracking-[0.2em] text-board-accent"
        >
          Issue card
        </Link>
      </div>
      {body}
    </div>
  );
};

const OffersRoute = () => {
  const { address: raw } = useParams<{ address: string }>();
  const parsed = useMemo(() => {
    try {
      return raw === undefined ? null : new PublicKey(raw);
    } catch {
      return null;
    }
  }, [raw]);

  if (parsed === null) {
    return (
      <div className="mx-auto w-full max-w-[1400px] px-5 py-10">
        <NoticePanel word="Refused" tone="red">
          <span className="break-all">{raw ?? '(empty)'}</span> is not a Solana address.
        </NoticePanel>
      </div>
    );
  }
  return <OffersScreen address={parsed} />;
};

export default OffersRoute;
