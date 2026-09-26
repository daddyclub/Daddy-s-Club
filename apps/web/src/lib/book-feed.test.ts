import { ACCOUNT_SPACE, DISCRIMINATORS, MARKET_PROGRAM_ID, type Offer } from '@daddys-club/sdk';
import { PublicKey } from '@solana/web3.js';
import { describe, expect, it } from 'vitest';
import { type Book, watchBook } from './book-feed';
import type { AccountFeed, AccountSnapshot, KeyedAccount, ProgramFeed } from './rpc';

const key = (seed: number): PublicKey => new PublicKey(new Uint8Array(32).fill(seed));
const ISSUE = key(1);
const OTHER_ISSUE = key(2);
const SELLER = key(3);
const A = key(10);
const B = key(11);
const C = key(12);

function offerBytes(offer: Offer): Uint8Array {
  const data = new Uint8Array(8 + ACCOUNT_SPACE.Offer);
  const view = new DataView(data.buffer);
  data.set(DISCRIMINATORS.Offer, 0);
  data.set(offer.seller.toBytes(), 8);
  data.set(offer.issue.toBytes(), 40);
  view.setBigUint64(72, offer.amount, true);
  view.setBigUint64(80, offer.price, true);
  data.set(offer.tokenEscrow.toBytes(), 88);
  view.setBigUint64(120, offer.nonce, true);
  data[128] = offer.bump;
  return data;
}

const offer = (amount: bigint, price: bigint, issue = ISSUE): Offer => ({
  seller: SELLER,
  issue,
  amount,
  price,
  tokenEscrow: key(99),
  nonce: 0n,
  bump: 255,
});

const ours = (o: Offer) => ({ owner: MARKET_PROGRAM_ID, data: offerBytes(o) });

/** Вузол у пам'яті: тесту видно, на що підписано, і він сам штовхає зміни. */
function node() {
  let listResolve: (value: { slot: number; accounts: KeyedAccount[] }) => void = () => {};
  let listReject: (error: unknown) => void = () => {};
  let programPush: ((address: PublicKey, s: AccountSnapshot) => void) | null = null;
  const accountPush = new Map<string, (s: AccountSnapshot) => void>();

  const program: ProgramFeed = {
    list: () =>
      new Promise((resolve, reject) => {
        listResolve = resolve;
        listReject = reject;
      }),
    watch: (_id, _filters, on) => {
      programPush = on;
      return () => {
        programPush = null;
      };
    },
  };
  const accounts: AccountFeed = {
    fetch: () => Promise.reject(new Error('не має кликатись')),
    watch: (address, on) => {
      accountPush.set(address.toBase58(), on);
      return () => accountPush.delete(address.toBase58());
    },
  };

  const books: Book[] = [];
  const stop = watchBook(program, accounts, MARKET_PROGRAM_ID, ISSUE, (b) => books.push(b));
  const flush = () => new Promise((r) => setTimeout(r, 0));

  return {
    books,
    stop,
    accountPush,
    last: (): Book | undefined => books.at(-1),
    list: async (slot: number, rows: KeyedAccount[]) => {
      listResolve({ slot, accounts: rows });
      await flush();
    },
    failList: async (error: unknown) => {
      listReject(error);
      await flush();
    },
    pushProgram: (address: PublicKey, s: AccountSnapshot) => programPush?.(address, s),
    pushAccount: (address: PublicKey, s: AccountSnapshot) =>
      accountPush.get(address.toBase58())?.(s),
    watchingProgram: () => programPush !== null,
  };
}

const addresses = (book: Book | undefined): string[] =>
  book?.status === 'live' ? book.entries.map((e) => e.address.toBase58()) : [];

describe('watchBook', () => {
  it('до першої вибірки стакан не показується — порожній і «ще не прочитаний» різні речі', async () => {
    const n = node();
    n.pushProgram(A, { slot: 5, account: ours(offer(10n, 9n)) });
    expect(n.books).toEqual([]);
    // Вибірка старіша за пуш — оферта, якої вона ще не бачила, лишається.
    await n.list(4, []);
    expect(addresses(n.last())).toEqual([A.toBase58()]);
  });

  it('найдешевша за одиницю номіналу — першою', async () => {
    const n = node();
    await n.list(10, [
      { address: A, account: ours(offer(100n, 99n)) },
      { address: B, account: ours(offer(100n, 90n)) },
      { address: C, account: ours(offer(50n, 46n)) },
    ]);
    expect(addresses(n.last())).toEqual([B.toBase58(), C.toBase58(), A.toBase58()]);
  });

  it('закриття приходить підпискою на саму оферту — і оферта зникає', async () => {
    const n = node();
    await n.list(10, [{ address: A, account: ours(offer(100n, 99n)) }]);
    expect(n.accountPush.has(A.toBase58())).toBe(true);

    n.pushAccount(A, { slot: 12, account: null });
    expect(addresses(n.last())).toEqual([]);
    expect(n.accountPush.has(A.toBase58())).toBe(false);
  });

  it('вибірка зі старішого слота не воскрешає закриту оферту', async () => {
    const n = node();
    n.pushProgram(A, { slot: 8, account: ours(offer(100n, 99n)) });
    n.pushAccount(A, { slot: 12, account: null });
    await n.list(10, [{ address: A, account: ours(offer(100n, 99n)) }]);
    expect(addresses(n.last())).toEqual([]);
  });

  it('вибірка з новішого слота, де оферти вже немає, прибирає її', async () => {
    const n = node();
    n.pushProgram(A, { slot: 8, account: ours(offer(100n, 99n)) });
    await n.list(10, []);
    expect(addresses(n.last())).toEqual([]);
  });

  it('чужий випуск, чужий власник і сміття — не оферти', async () => {
    const n = node();
    await n.list(10, [
      { address: A, account: ours(offer(100n, 99n, OTHER_ISSUE)) },
      { address: B, account: { owner: key(77), data: offerBytes(offer(1n, 1n)) } },
      { address: C, account: { owner: MARKET_PROGRAM_ID, data: new Uint8Array(129) } },
    ]);
    expect(addresses(n.last())).toEqual([]);
  });

  it('вузол не відповів на вибірку — «unreachable», а не порожній стакан', async () => {
    const n = node();
    await n.failList(new Error('ECONNREFUSED'));
    expect(n.last()).toEqual({ status: 'unreachable', detail: 'ECONNREFUSED' });
  });

  it('після зупинки — жодної підписки і жодного виклику', async () => {
    const n = node();
    await n.list(10, [{ address: A, account: ours(offer(100n, 99n)) }]);
    const before = n.books.length;
    n.stop();
    expect(n.watchingProgram()).toBe(false);
    expect(n.accountPush.size).toBe(0);
    n.pushProgram(B, { slot: 20, account: ours(offer(1n, 1n)) });
    expect(n.books.length).toBe(before);
  });
});
