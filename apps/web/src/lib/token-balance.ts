/**
 * Баланс токен-рахунку наживо — бонд і USDC гаманця на екрані оферт.
 *
 * Саме тут продавець бачить, що гроші прийшли: `SC-009` закінчується не на
 * підтвердженні транзакції покупця, а на пуші рахунку продавця. Правило слота
 * те саме, що в `watchIssue`.
 */

import { decodeTokenAccount, TOKEN_2022_PROGRAM_ID } from '@daddys-club/sdk';
import type { PublicKey } from '@solana/web3.js';
import type { AccountFeed, AccountSnapshot } from './rpc';

export type Balance =
  | { readonly status: 'loading' }
  | { readonly status: 'live'; readonly amount: bigint; readonly slot: number }
  /** Рахунку немає — це нуль для того, хто хоче отримати, і відмова для того, хто платить. */
  | { readonly status: 'missing'; readonly slot: number }
  /** За адресою не токен-рахунок Token-2022. */
  | { readonly status: 'foreign'; readonly slot: number }
  | { readonly status: 'unreachable'; readonly detail: string };

export function interpretBalance(snapshot: AccountSnapshot): Balance {
  const { slot, account } = snapshot;
  if (account === null) return { status: 'missing', slot };
  if (!account.owner.equals(TOKEN_2022_PROGRAM_ID)) return { status: 'foreign', slot };
  try {
    return { status: 'live', amount: decodeTokenAccount(account.data).amount, slot };
  } catch {
    return { status: 'foreign', slot };
  }
}

/** Скільки є на рахунку для розрахунків: відсутній рахунок — нуль, невідомий — `null`. */
export function spendable(balance: Balance): bigint | null {
  if (balance.status === 'live') return balance.amount;
  if (balance.status === 'missing') return 0n;
  return null;
}

export function watchTokenBalance(
  feed: AccountFeed,
  address: PublicKey,
  sink: (balance: Balance) => void,
): () => void {
  let stopped = false;
  let lastSlot = -1;

  const apply = (snapshot: AccountSnapshot): void => {
    if (stopped || snapshot.slot < lastSlot) return;
    lastSlot = snapshot.slot;
    sink(interpretBalance(snapshot));
  };

  const unwatch = feed.watch(address, apply);
  void feed.fetch(address).then(apply, (error: unknown) => {
    if (stopped || lastSlot >= 0) return;
    sink({ status: 'unreachable', detail: error instanceof Error ? error.message : String(error) });
  });

  return () => {
    stopped = true;
    unwatch();
  };
}
