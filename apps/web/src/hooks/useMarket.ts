/**
 * Склейка стакана, балансів і конфігу протоколу з React. Уся логіка — у
 * `lib/book-feed.ts` і `lib/token-balance.ts`, які ганяються тестами; тут лише
 * життєвий цикл підписок.
 */

import { configPda, decodeProtocolConfig, type ProtocolConfig } from '@daddys-club/sdk';
import type { PublicKey } from '@solana/web3.js';
import { useEffect, useState } from 'react';
import { type Book, watchBook } from '@/lib/book-feed';
import type { AccountFeed, ProgramFeed } from '@/lib/rpc';
import { type Balance, watchTokenBalance } from '@/lib/token-balance';

export function useBook(
  program: ProgramFeed,
  accounts: AccountFeed,
  marketId: PublicKey,
  issue: PublicKey,
): Book {
  const [book, setBook] = useState<Book>({ status: 'loading' });
  useEffect(() => {
    setBook({ status: 'loading' });
    return watchBook(program, accounts, marketId, issue, setBook);
  }, [program, accounts, marketId, issue]);
  return book;
}

/** `address === null` — гаманець не підключений, і рахунку нема про що питати. */
export function useTokenBalance(feed: AccountFeed, address: PublicKey | null): Balance {
  const [balance, setBalance] = useState<Balance>({ status: 'loading' });
  const key = address?.toBase58() ?? null;
  // biome-ignore lint/correctness/useExhaustiveDependencies: адреса порівнюється рядком — новий PublicKey на рендер перепідписував би сокет
  useEffect(() => {
    setBalance({ status: 'loading' });
    if (address === null) return;
    return watchTokenBalance(feed, address, setBalance);
  }, [feed, key]);
  return balance;
}

export type ConfigState =
  | { readonly status: 'loading' }
  | { readonly status: 'live'; readonly config: ProtocolConfig }
  | { readonly status: 'failed'; readonly detail: string };

/**
 * Параметри протоколу — ставка комісії, валюта, скарбниця. Читаються раз: вони
 * змінюються рідко, а ставку угоди все одно бере сама програма з актуального
 * конфігу, тож застарілий екран не може взяти з продавця іншу комісію.
 */
export function useProtocolConfig(feed: AccountFeed, programId: PublicKey): ConfigState {
  const [state, setState] = useState<ConfigState>({ status: 'loading' });
  useEffect(() => {
    let live = true;
    feed.fetch(configPda(programId).address).then(
      ({ account }) => {
        if (!live) return;
        if (account === null || !account.owner.equals(programId)) {
          setState({ status: 'failed', detail: 'the protocol config account is missing' });
          return;
        }
        try {
          setState({ status: 'live', config: decodeProtocolConfig(account.data) });
        } catch (error) {
          setState({
            status: 'failed',
            detail: error instanceof Error ? error.message : String(error),
          });
        }
      },
      (error: unknown) => {
        if (live)
          setState({
            status: 'failed',
            detail: error instanceof Error ? error.message : String(error),
          });
      },
    );
    return () => {
      live = false;
    };
  }, [feed, programId]);
  return state;
}
