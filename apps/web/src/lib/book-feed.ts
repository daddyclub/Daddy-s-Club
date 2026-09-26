/**
 * Стакан оферт випуску на стані ланцюга (`FR-024`, `FR-033`).
 *
 * Індексатора немає (`FR-023`), тому стакан — це вибірка акаунтів ринку з
 * фільтром за випуском плюс дві підписки. Перша — на програму: вона приносить
 * нові оферти. Друга — на кожну видиму оферту окремо: підписка на програму з
 * `memcmp` про закриття мовчить (занулені дані фільтр не пропускає), і без
 * неї викуплена оферта лишилась би в стакані до перезавантаження.
 *
 * Правило те саме, що в `watchIssue`: застосовується не те, що прийшло
 * останнім, а те, що з більшого слота. Зникла оферта лишає по собі «надгробок»
 * зі слотом — інакше вибірка, зроблена раніше за закриття, але повернута
 * пізніше, воскресила б її.
 */

import { AccountDecodeError, decodeOffer, type Offer, offerFilters } from '@daddys-club/sdk';
import type { PublicKey } from '@solana/web3.js';
import type { AccountFeed, AccountSnapshot, ProgramFeed } from './rpc';

export interface BookEntry {
  readonly address: PublicKey;
  readonly offer: Offer;
  readonly slot: number;
}

export type Book =
  | { readonly status: 'loading' }
  /** Оферти, найдешевші за одиницю номіналу — першими. */
  | { readonly status: 'live'; readonly entries: readonly BookEntry[]; readonly slot: number }
  | { readonly status: 'unreachable'; readonly detail: string };

interface Slot {
  readonly slot: number;
  readonly entry: BookEntry | null;
}

/** Дешевша за одиницю — вище. `a.price / a.amount < b.price / b.amount` без ділення. */
export function cheaperFirst(a: BookEntry, b: BookEntry): number {
  const left = a.offer.price * b.offer.amount;
  const right = b.offer.price * a.offer.amount;
  if (left !== right) return left < right ? -1 : 1;
  return a.address.toBase58() < b.address.toBase58() ? -1 : 1;
}

/**
 * Що означає акаунт за адресою для стакана. `null` — оферти тут немає: акаунт
 * закрито, він чужий, не декодується або належить іншому випуску.
 */
export function interpretOffer(
  address: PublicKey,
  snapshot: AccountSnapshot,
  marketId: PublicKey,
  issue: PublicKey,
): BookEntry | null {
  const { account } = snapshot;
  if (account === null || !account.owner.equals(marketId)) return null;
  try {
    const offer = decodeOffer(account.data);
    return offer.issue.equals(issue) ? { address, offer, slot: snapshot.slot } : null;
  } catch (error) {
    if (error instanceof AccountDecodeError) return null;
    throw error;
  }
}

function detailOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function watchBook(
  program: ProgramFeed,
  accounts: AccountFeed,
  marketId: PublicKey,
  issue: PublicKey,
  sink: (book: Book) => void,
): () => void {
  const state = new Map<string, Slot>();
  const perOffer = new Map<string, () => void>();
  let stopped = false;
  let listed = false;
  let lastSlot = -1;

  const emit = (): void => {
    if (!listed) return;
    const entries = [...state.values()]
      .map((slot) => slot.entry)
      .filter((entry): entry is BookEntry => entry !== null)
      .sort(cheaperFirst);
    sink({ status: 'live', entries, slot: lastSlot });
  };

  const apply = (address: PublicKey, snapshot: AccountSnapshot): void => {
    if (stopped) return;
    const key = address.toBase58();
    const previous = state.get(key);
    if (previous !== undefined && snapshot.slot < previous.slot) return;

    const entry = interpretOffer(address, snapshot, marketId, issue);
    state.set(key, { slot: snapshot.slot, entry });
    lastSlot = Math.max(lastSlot, snapshot.slot);

    if (entry !== null && !perOffer.has(key)) {
      perOffer.set(
        key,
        accounts.watch(address, (next) => apply(address, next)),
      );
    }
    if (entry === null) {
      perOffer.get(key)?.();
      perOffer.delete(key);
    }
    emit();
  };

  const filters = offerFilters(issue);
  const unwatchProgram = program.watch(marketId, filters, apply);

  void program.list(marketId, filters).then(
    ({ slot, accounts: rows }) => {
      if (stopped) return;
      const seen = new Set<string>();
      for (const { address, account } of rows) {
        seen.add(address.toBase58());
        apply(address, { slot, account });
      }
      // Оферта, яку ми знаємо зі старішого слота, а вибірка її вже не бачить,
      // — закрита між тим. Свіжіший пуш про неї (слот ≥ вибірки) лишається.
      for (const [key, known] of state) {
        if (!seen.has(key) && known.entry !== null && known.slot < slot) {
          apply(known.entry.address, { slot, account: null });
        }
      }
      listed = true;
      lastSlot = Math.max(lastSlot, slot);
      emit();
    },
    (error: unknown) => {
      if (stopped || listed) return;
      sink({ status: 'unreachable', detail: detailOf(error) });
    },
  );

  return () => {
    stopped = true;
    unwatchProgram();
    for (const unwatch of perOffer.values()) unwatch();
    perOffer.clear();
  };
}
