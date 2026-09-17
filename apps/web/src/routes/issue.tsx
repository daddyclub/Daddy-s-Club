/**
 * Картка випуску на живому стані ланцюга (`FR-023`, `FR-033`).
 *
 * Це перший екран, який нічого не вигадує: усі числа тут прочитані з акаунта
 * `Issue` за його адресою, без бекенду й індексатора (`FR-023`). Лічильник
 * погашення рухається сам — вузол **пушить** зміну акаунта підпискою, сторінка
 * не перезавантажується і нічого не опитує (`FR-033`).
 *
 * Демо-екрани M0 (`pages/`) лишаються на вигаданих числах і живуть на своїх
 * адресах. Шлях `/live/issue/:address` тимчасовий: коли `T045` поставить
 * список випусків на дані ланцюга, демо-дошка піде, а ця картка займе `/issue`.
 */

import type { IssueState } from '@daddys-club/sdk';
import { PublicKey } from '@solana/web3.js';
import { type FormEvent, type ReactNode, useMemo, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import BoardButton from '@/components/BoardButton';
import DataRow from '@/components/DataRow';
import NoticePanel from '@/components/NoticePanel';
import ProgressBar from '@/components/ProgressBar';
import RepaymentBoard from '@/components/RepaymentBoard';
import Section from '@/components/Section';
import { useIssueAccount } from '@/hooks/useIssueAccount';
import { webEnv } from '@/lib/env';
import { fromBaseUnits, type IssueSnapshot, repaymentView } from '@/lib/issue-feed';
import { sharedFeed } from '@/lib/rpc';
import { amount, bps, clock, instant, percent, share } from '@/lib/format';

/**
 * Стани показуються тими іменами, які носить сам ланцюг. Демо-дошка M0 звe їх
 * інакше («Overdue», «Undersubscribed»), і зводити два словники в один тут не
 * можна: `Funded` у демо-словнику немає взагалі, а глядач цієї картки дивиться
 * саме на стан акаунта.
 */
const STATE_TONE: Record<IssueState, string> = {
  Subscribing: 'border-board-accent text-board-accent',
  Funded: 'border-board-accent text-board-accent',
  Repaying: 'border-board-accent text-board-accent',
  PastDue: 'border-board-red text-board-red',
  Repaid: 'border-board-ink text-board-ink',
  Failed: 'border-board-faint text-board-dim',
};

/** Стани, у яких зобов'язання вже існує і лічильник має сенс (`FR-012`). */
const REPAYING_STATES: readonly IssueState[] = ['Repaying', 'PastDue', 'Repaid'];

const ChainStateChip = ({ state }: { state: IssueState }) => (
  <span
    className={`inline-flex items-center border px-2 py-[3px] text-[11px] uppercase leading-none tracking-[0.14em] ${STATE_TONE[state]}`}
  >
    {state}
  </span>
);

/** Коротка адреса для заголовка. Повна лишається під нею, щоб її можна було звірити. */
const short = (key: PublicKey): string => {
  const text = key.toBase58();
  return `${text.slice(0, 6)}…${text.slice(-6)}`;
};

/**
 * Ворота адреси. Списку випусків ще немає — він з'явиться на `T045`, — тому
 * поки що адресу вводять руками: без неї до картки не дійти взагалі.
 */
const AddressGate = () => {
  const navigate = useNavigate();
  const [text, setText] = useState('');
  const [refusal, setRefusal] = useState<string | null>(null);

  const submit = (event: FormEvent): void => {
    event.preventDefault();
    const trimmed = text.trim();
    try {
      navigate(`/live/issue/${new PublicKey(trimmed).toBase58()}`);
    } catch {
      setRefusal(`Not a Solana address: ${trimmed || '(empty)'}`);
    }
  };

  return (
    <div className="mx-auto w-full max-w-[1400px] px-5 py-10">
      <h1 className="text-[clamp(1.2rem,2.4vw,1.6rem)] uppercase tracking-[0.14em]">
        Issue on chain
      </h1>
      <p className="mt-3 max-w-[74ch] text-[12px] leading-[1.7] text-board-dim">
        Paste the address of an issue account. Everything on the card is read from that account and
        from nothing else — there is no backend and no indexer behind this screen.
      </p>
      <form onSubmit={submit} className="mt-6 flex flex-wrap items-center gap-3">
        <input
          value={text}
          onChange={(event) => setText(event.target.value)}
          spellCheck={false}
          placeholder="Issue account address"
          aria-label="Issue account address"
          className="w-full max-w-[520px] border border-board-ink bg-board-cell px-3 py-2 text-[12px] tracking-tight text-board-ink outline-none placeholder:text-board-dim"
        />
        <BoardButton variant="line" type="submit">
          Open
        </BoardButton>
      </form>
      {refusal === null ? null : (
        <div className="mt-5">
          <NoticePanel word="Refused" tone="red">
            {refusal}
          </NoticePanel>
        </div>
      )}
    </div>
  );
};

/** Однакова рамка для будь-якої відмови: адреса на місці, пояснення під нею. */
const Refusal = ({
  address,
  word,
  tone,
  children,
}: {
  address: PublicKey;
  word: string;
  tone: 'red' | 'dim' | 'accent';
  children: ReactNode;
}) => (
  <div className="mx-auto w-full max-w-[1400px] px-5 py-10">
    <div className="text-[10px] uppercase tracking-[0.2em] text-board-dim">Issue account</div>
    <div className="mt-2 break-all text-[13px] tracking-tight">{address.toBase58()}</div>
    <div className="mt-6">
      <NoticePanel word={word} tone={tone}>
        {children}
      </NoticePanel>
    </div>
    <div className="mt-6">
      <Link
        to="/live/issue"
        className="border-b border-board-accent text-[11px] uppercase tracking-[0.2em] text-board-accent"
      >
        Another address
      </Link>
    </div>
  </div>
);

interface LiveCardProps {
  address: PublicKey;
  snapshot: Extract<IssueSnapshot, { status: 'live' }>;
}

const LiveCard = ({ address, snapshot }: LiveCardProps) => {
  const { issue, slot, at } = snapshot;
  const view = repaymentView(issue);
  const repaying = REPAYING_STATES.includes(issue.state);
  const raisedPct = issue.face === 0n ? 0 : Number((issue.raised * 10_000n) / issue.face) / 100;

  return (
    <div className="mx-auto w-full max-w-[1400px] px-5 py-8">
      <div className="mb-6 flex flex-wrap items-end justify-between gap-4 border-b border-board-ink pb-4">
        <div>
          <div className="flex flex-wrap items-center gap-4">
            <h1 className="text-[clamp(1.2rem,2.4vw,1.7rem)] uppercase tracking-[0.14em]">
              Issue {short(address)}
            </h1>
            <ChainStateChip state={issue.state} />
          </div>
          <div className="mt-3 break-all text-[11px] tracking-tight text-board-dim">
            {address.toBase58()}
          </div>
        </div>
        <div className="text-right text-[10px] uppercase tracking-[0.16em] text-board-dim">
          <div>Read straight from the account</div>
          <div className="mt-1 tabular-nums">
            Slot {slot} &middot; {clock(new Date(at))}
          </div>
        </div>
      </div>

      {repaying ? (
        <RepaymentBoard
          repaid={fromBaseUnits(view.repaid)}
          totalOwed={fromBaseUnits(view.obligation)}
          settled={view.settled}
          caption={`Pledged share ${bps(issue.pledgeBps)} of every fee · slot ${slot}`}
          pctLabel={share(view.pct)}
        />
      ) : (
        <div className="border-y border-board-ink py-6">
          <div className="mb-4 text-[10px] uppercase tracking-[0.28em] text-board-dim">
            Raised so far
          </div>
          <div className="text-[clamp(1.4rem,4vw,2.4rem)] tabular-nums tracking-tight">
            {amount(fromBaseUnits(issue.raised))}{' '}
            <span className="text-[12px] uppercase tracking-[0.16em] text-board-dim">
              USDC of {amount(fromBaseUnits(issue.face))} USDC
            </span>
          </div>
          <div className="mt-5">
            <ProgressBar pct={raisedPct} height={10} />
            <div className="mt-2 flex justify-between text-[11px] uppercase tracking-[0.16em] text-board-dim">
              <span>Subscription book</span>
              <span>{percent(raisedPct)} of face</span>
            </div>
          </div>
          <div className="mt-6">
            <NoticePanel word="Nothing is being repaid yet" tone="accent">
              The obligation starts when the issuer draws the proceeds, and only then does the split
              begin. Until that moment the counter below stands at zero because nothing has been
              intercepted &mdash; not because the card failed to read it.
            </NoticePanel>
          </div>
        </div>
      )}

      <div className="mt-10 grid gap-10 lg:grid-cols-2">
        <Section title="Repayment progress" aside="FR-023 · straight from the account">
          <DataRow label="Repaid" value={`${amount(fromBaseUnits(view.repaid))} USDC`} />
          <DataRow label="Remaining" value={`${amount(fromBaseUnits(view.remaining))} USDC`} />
          <DataRow label="Share of the obligation" value={share(view.pct)} tone="accent" />
          <DataRow label="Total owed" value={`${amount(fromBaseUnits(view.obligation))} USDC`} />
          <DataRow label="Payout index" value={issue.payoutIndex.toString()} tone="dim" />
        </Section>

        <Section title="Bond terms">
          <DataRow label="Face value" value={`${amount(fromBaseUnits(issue.face))} USDC`} />
          <DataRow label="Coupon" value={bps(issue.couponBps)} />
          <DataRow label="Pledged share" value={bps(issue.pledgeBps)} />
          <DataRow label="Minimum lot" value={`${amount(fromBaseUnits(issue.minLot))} USDC`} />
          <DataRow label="Maturity" value={instant(issue.maturityTs)} />
          <DataRow label="Subscription closes" value={instant(issue.subscriptionEndTs)} />
          <DataRow label="Bond mint" value={short(issue.bondMint)} tone="dim" />
          <DataRow label="Revenue source" value={short(issue.source)} tone="dim" />
        </Section>
      </div>

      <div className="mt-10">
        <NoticePanel word="How this counter moves" tone="dim">
          The number above is <code>repaid_total</code> from this very account, and it changes when
          the node pushes the account to this page &mdash; there is no polling, no backend and no
          indexer in between. Every push carries the whole account, so a dropped socket costs a
          delay, never a wrong number.
        </NoticePanel>
      </div>

      <div className="mt-6">
        <Link
          to="/live/issue"
          className="border-b border-board-accent text-[11px] uppercase tracking-[0.2em] text-board-accent"
        >
          Another address
        </Link>
      </div>
    </div>
  );
};

const IssueRoute = () => {
  const { address: raw } = useParams<{ address: string }>();
  const env = webEnv();
  const feed = useMemo(() => sharedFeed(env.rpcUrl), [env.rpcUrl]);

  // Адреса запам'ятовується за рядком з URL: новий `PublicKey` на кожен рендер
  // перепідписував би сокет по колу.
  const parsed = useMemo(() => {
    if (raw === undefined) return null;
    try {
      return new PublicKey(raw);
    } catch {
      return 'invalid' as const;
    }
  }, [raw]);

  if (parsed === null) return <AddressGate />;

  if (parsed === 'invalid') {
    return (
      <div className="mx-auto w-full max-w-[1400px] px-5 py-10">
        <NoticePanel word="Refused" tone="red">
          <span className="break-all">{raw}</span> is not a Solana address. Nothing was read and
          nothing is being shown for it.
        </NoticePanel>
        <div className="mt-6">
          <Link
            to="/live/issue"
            className="border-b border-board-accent text-[11px] uppercase tracking-[0.2em] text-board-accent"
          >
            Another address
          </Link>
        </div>
      </div>
    );
  }

  return <IssueCard address={parsed} feed={feed} programId={env.programId} />;
};

interface IssueCardProps {
  address: PublicKey;
  feed: ReturnType<typeof sharedFeed>;
  programId: PublicKey;
}

/**
 * Підписка живе тут, а не в `IssueRoute`: хук не можна кликати після
 * дострокового `return`, а відмови розбору адреси мусять стояти до нього.
 */
const IssueCard = ({ address, feed, programId }: IssueCardProps) => {
  const snapshot = useIssueAccount(feed, address, programId);

  switch (snapshot.status) {
    case 'loading':
      return (
        <div className="mx-auto w-full max-w-[1400px] px-5 py-16 text-[12px] uppercase tracking-[0.28em] text-board-dim">
          Reading the account&hellip;
        </div>
      );

    case 'unreachable':
      return (
        <Refusal address={address} word="Node did not answer" tone="red">
          The card could not read the account: {snapshot.detail}. The counter is unknown, not zero
          &mdash; nothing on this screen should be read as «nothing has been repaid».
        </Refusal>
      );

    case 'missing':
      return (
        <Refusal address={address} word="No account at this address" tone="dim">
          There is no account at this address as of slot {snapshot.slot}. An issue that was never
          created and an issue with nothing repaid are different things, and this is the first.
        </Refusal>
      );

    case 'foreign':
      return (
        <Refusal address={address} word="Account belongs to another program" tone="red">
          The account at this address is owned by {snapshot.owner.toBase58()}, not by the protocol.
          Bytes that merely look like an issue are not an issue.
        </Refusal>
      );

    case 'undecodable':
      return (
        <Refusal address={address} word="Account did not decode" tone="red">
          {snapshot.detail} ({snapshot.reason}). The account is owned by the protocol but is not an
          issue this build knows how to read &mdash; most likely it is another account type, or one
          written by a newer version of the program.
        </Refusal>
      );

    case 'live':
      return <LiveCard address={address} snapshot={snapshot} />;
  }
};

export default IssueRoute;
