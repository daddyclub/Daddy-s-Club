/**
 * Читання випуску з ланцюга і підписка на його зміни (`FR-023`, `FR-033`).
 *
 * Тут немає React навмисно: увесь стан картки — це чиста функція від того, що
 * прийшло з вузла, і саме тому його можна ганяти тестом без браузера. Хук
 * `useIssueAccount` лишається трьома рядками склейки.
 *
 * **Прогрес читається зі стану ончейн, без бекенду й індексатора** (`FR-023`):
 * `repaid_total` і `obligation_total` живуть у самому `Issue`, а не рахуються
 * тут із подій. Ця межа й робить `FR-033` дешевим — оновлення приходить
 * **пушем** підписки, а не опитуванням.
 *
 * **Нулів замість відповіді тут не буває.** Акаунта немає, акаунт чужий, байти
 * не розбираються, вузол недоступний — це чотири різні відмови з іменами, і
 * жодна з них не має права виглядати як «виплачено 0». Те саме правило, що
 * `FR-037` ставить публічному читачеві: відсутність повертається явно.
 */

import { AccountDecodeError, type DecodeFailure, decodeIssue, type Issue } from '@daddys-club/sdk';
import type { PublicKey } from '@solana/web3.js';
import type { AccountFeed, AccountSnapshot } from './rpc';

/**
 * Знаків у розрахунковій валюті цього розгортання. Протокол їх не фіксує —
 * `intercept` бере `decimals` із самого мінта, — тому число тут описує мінт, у
 * якому підняте демо, і впливає лише на **підпис** суми. Ані лічильник, ані
 * частка від зобов'язання від нього не залежать: обидва рахуються в тих самих
 * найменших одиницях, у яких їх веде програма.
 */
export const USDC_DECIMALS = 6;

/** Стан картки. Кожна відмова названа, бо дії за ними різні. */
export type IssueSnapshot =
  | { readonly status: 'loading' }
  /** Випуск прочитаний. `slot` і `at` — доказ того, що лічильник живий. */
  | { readonly status: 'live'; readonly issue: Issue; readonly slot: number; readonly at: number }
  /** За адресою немає акаунта. Явна відсутність, не нулі. */
  | { readonly status: 'missing'; readonly slot: number }
  /** Акаунт є, але належить не нашій програмі — адреса не та. */
  | { readonly status: 'foreign'; readonly owner: PublicKey; readonly slot: number }
  /** Акаунт наш, але це не `Issue` або він із новішої версії програми. */
  | {
      readonly status: 'undecodable';
      readonly reason: DecodeFailure;
      readonly detail: string;
      readonly slot: number;
    }
  /** Вузол не відповів. Лічильник не «нульовий», а невідомий. */
  | { readonly status: 'unreachable'; readonly detail: string };

/** Чисте тлумачення того, що прийшло з вузла. Мережі тут уже немає. */
export function interpret(
  snapshot: AccountSnapshot,
  programId: PublicKey,
  now: number = Date.now(),
): IssueSnapshot {
  const { slot, account } = snapshot;

  if (account === null) return { status: 'missing', slot };

  // Власник перевіряється **до** декодера, а не після. Дискримінатор доводить
  // лише те, що байти виглядають як `Issue`; хто їх написав, каже власник, і
  // покласти потрібні вісім байтів у власний акаунт може будь-хто.
  if (!account.owner.equals(programId)) {
    return { status: 'foreign', owner: account.owner, slot };
  }

  try {
    return { status: 'live', issue: decodeIssue(account.data), slot, at: now };
  } catch (error) {
    if (error instanceof AccountDecodeError) {
      return { status: 'undecodable', reason: error.reason, detail: error.message, slot };
    }
    throw error;
  }
}

function detailOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Тримає картку на поточному стані випуску (`FR-033`).
 *
 * Порядок двох дій має значення: підписка ставиться **перед** читанням.
 * Навпаки — і зміна, що сталася між відповіддю й підпискою, не дійшла б до
 * картки взагалі, а наступної могло б не бути ще довго.
 *
 * Звідси ж і гонка, яку знімає слот: пуш має право випередити відповідь на
 * початкове читання, і тоді старіша відповідь ляже поверх свіжішої. Тому
 * застосовується не те, що прийшло останнім, а те, що з більшого слота.
 *
 * Повідомлення несе **увесь** акаунт, а не приріст, тому пропущене оновлення
 * не накопичується в помилку: після розриву сокета перше ж наступне
 * повідомлення повертає лічильник на місце.
 */
export function watchIssue(
  feed: AccountFeed,
  address: PublicKey,
  programId: PublicKey,
  sink: (snapshot: IssueSnapshot) => void,
): () => void {
  let stopped = false;
  let lastSlot = -1;

  const apply = (snapshot: AccountSnapshot): void => {
    if (stopped || snapshot.slot < lastSlot) return;
    lastSlot = snapshot.slot;
    sink(interpret(snapshot, programId));
  };

  const unwatch = feed.watch(address, apply);

  void feed.fetch(address).then(apply, (error: unknown) => {
    // Відмова початкового читання не має права затерти вже отриманий пуш:
    // підписка жива, лічильник справжній, а не прочитати ще раз — не подія.
    if (stopped || lastSlot >= 0) return;
    sink({ status: 'unreachable', detail: detailOf(error) });
  });

  return () => {
    stopped = true;
    unwatch();
  };
}

/** Прогрес погашення так, як його називає `FR-023`. */
export interface RepaymentView {
  /** Виплачено — у найменших одиницях розрахункової валюти. */
  readonly repaid: bigint;
  /** Повне зобов'язання: номінал + купон (`FR-018`). */
  readonly obligation: bigint;
  /** Залишок зобов'язання. */
  readonly remaining: bigint;
  /** Частка від зобов'язання у відсотках, з двома знаками. */
  readonly pct: number;
  /** Зобов'язання закрите — перехоплення припинилось саме (`FR-019`). */
  readonly settled: boolean;
}

/**
 * Три числа `FR-023` з одного акаунта.
 *
 * Частка рахується в `bigint` і лише наприкінці стає числом: `repaid` і
 * `obligation` — це u64 у найменших одиницях, і ділити їх через `number`
 * означало б втратити точність рівно там, де вимога просить показати прогрес.
 */
export function repaymentView(issue: Issue): RepaymentView {
  const { repaidTotal: repaid, obligationTotal: obligation } = issue;

  // Виплачене понад зобов'язання — порушення `SC-004`, і ховати його за
  // від'ємним залишком не можна: залишок нульовий, а невідповідність видно по
  // частці, яка перевалила за сто.
  const remaining = repaid >= obligation ? 0n : obligation - repaid;
  const pct = obligation === 0n ? 0 : Number((repaid * 10_000n) / obligation) / 100;

  return { repaid, obligation, remaining, pct, settled: repaid >= obligation };
}

/**
 * Найменші одиниці у число для показу.
 *
 * Ділиться окремо цілу частину й дріб: `Number(bigint)` губить точність за
 * 2^53, і на сумах, які взагалі бувають у USDC, це видно як зайву копійку.
 */
export function fromBaseUnits(value: bigint, decimals: number = USDC_DECIMALS): number {
  const scale = 10n ** BigInt(decimals);
  return Number(value / scale) + Number(value % scale) / Number(scale);
}
