/**
 * Перші тести в `apps/web`. До T030 їх тут не було навмисно: демо-екрани M0
 * рендерять сталі з `data/mock.ts`, і тест на них доводив би лише те, що стала
 * дорівнює сама собі.
 *
 * Тут предмет інший. Картка випуску показує **гроші, прочитані з ланцюга**, і
 * стереже цей файл рівно два роди помилок, кожен із яких мовчазний:
 *
 * 1. **Невідоме, показане як нуль.** Вузол не відповів, акаунт чужий, байти не
 *    розбираються — і на екрані «виплачено 0.00 USDC». Це не помилка рендеру, а
 *    хибне число на екрані про гроші.
 * 2. **Лічильник, що спинився і не сказав.** Підписка, поставлена після
 *    читання, губить зміну між ними; відповідь на початкове читання, що лягла
 *    поверх свіжішого пуша, відкочує лічильник назад. Обидві гонки на localnet
 *    не відтворюються — вузол там надто швидкий, — а на живому вузлі
 *    відтворюються самі.
 *
 * Мережі, React і браузера тут немає: `watchIssue` — чиста функція від порту
 * `AccountFeed`, і саме тому обидві гонки можна ганяти детерміновано.
 */

import { DISCRIMINATORS, type IssueState, ISSUE_STATES } from '@daddys-club/sdk';
import { PublicKey } from '@solana/web3.js';
import { describe, expect, it } from 'vitest';
import {
  fromBaseUnits,
  interpret,
  type IssueSnapshot,
  repaymentView,
  watchIssue,
} from './issue-feed';
import type { AccountFeed, AccountSnapshot, RawAccount } from './rpc';

const PROGRAM = new PublicKey('7eT5T7mq1uD9piYJ2rMAzma8iYL7C7CZGPgxsB8DckoB');
const OTHER_PROGRAM = new PublicKey('8wKjGiLvnMTv7oi9PcztmbRv4v63emT2qPPrA8x1fW3z');
const ADDRESS = new PublicKey('11111111111111111111111111111112');

/** USDC має шість знаків — те саме число, що в `tests/harness.rs`. */
const USDC = 1_000_000n;

interface IssueFields {
  face: bigint;
  obligationTotal: bigint;
  repaidTotal: bigint;
  raised: bigint;
  state: IssueState;
}

/**
 * Байти акаунта `Issue` руками, у порядку полів зі `state.rs`.
 *
 * Зібрані тут, а не взяті з вузла: тест мусить уміти показати стан, якого на
 * localnet не буває — виплачене понад зобов'язання, чужого власника, обрізаний
 * акаунт. Що розкладка та сама, що в програмі, доводить `accounts.test.ts`
 * у SDK; тут вона лише використовується.
 */
function issueBytes(fields: IssueFields): Uint8Array {
  const bytes = new Uint8Array(8 + 214);
  bytes.set(DISCRIMINATORS.Issue, 0);
  const view = new DataView(bytes.buffer);

  // Чотири ключі поспіль: source, bond_mint, escrow_vault, subscription_vault.
  for (let i = 0; i < 4; i += 1) bytes[8 + i * 32] = i + 1;

  let offset = 8 + 128;
  const u64 = (value: bigint): void => {
    view.setBigUint64(offset, value, true);
    offset += 8;
  };
  const u16 = (value: number): void => {
    view.setUint16(offset, value, true);
    offset += 2;
  };

  u64(fields.face);
  u16(950); // coupon_bps
  u16(1_200); // pledge_bps
  view.setBigInt64(offset, 1_800_000_000n, true); // maturity_ts
  offset += 8;
  view.setBigInt64(offset, 1_790_000_000n, true); // subscription_end_ts
  offset += 8;
  u64(1_000n * USDC); // min_lot
  u64(fields.raised);
  u64(fields.obligationTotal);
  u64(fields.repaidTotal);
  // payout_index — u128, двома половинами little-endian.
  u64(0n);
  u64(0n);
  view.setUint8(offset, ISSUE_STATES.indexOf(fields.state));
  offset += 1;
  u64(0n); // seq
  view.setUint8(offset, 255); // bump

  return bytes;
}

function account(fields: Partial<IssueFields> = {}, owner: PublicKey = PROGRAM): RawAccount {
  return {
    owner,
    data: issueBytes({
      face: 250_000n * USDC,
      obligationTotal: 273_750n * USDC,
      repaidTotal: 0n,
      raised: 250_000n * USDC,
      state: 'Repaying',
      ...fields,
    }),
  };
}

/**
 * Вузол, яким керує тест. Читання й пуші подаються руками, тому порядок їхнього
 * прибуття — предмет заміру, а не випадковість мережі.
 */
class FakeFeed implements AccountFeed {
  readonly calls: string[] = [];
  unwatched = 0;
  private push: ((snapshot: AccountSnapshot) => void) | null = null;
  /**
   * Той самий колбек, але не забутий після відписки. `removeAccountChangeListener`
   * асинхронний: повідомлення, яке вже було в дорозі, приходить і після нього, і
   * глушити його мусить сам `watchIssue`, а не вузол.
   */
  private inFlight: ((snapshot: AccountSnapshot) => void) | null = null;
  private settle: ((snapshot: AccountSnapshot) => void) | null = null;
  private fail: ((error: unknown) => void) | null = null;

  fetch(): Promise<AccountSnapshot> {
    this.calls.push('fetch');
    return new Promise((resolve, reject) => {
      this.settle = resolve;
      this.fail = reject;
    });
  }

  watch(_address: PublicKey, onSnapshot: (snapshot: AccountSnapshot) => void): () => void {
    this.calls.push('watch');
    this.push = onSnapshot;
    this.inFlight = onSnapshot;
    return () => {
      this.unwatched += 1;
      this.push = null;
    };
  }

  /** Пуш від вузла — те, чим рухається лічильник (`FR-033`). */
  notify(snapshot: AccountSnapshot): void {
    if (this.push === null) throw new Error('пуш після відписки — вузол так не робить');
    this.push(snapshot);
  }

  /** Пуш повз відписку: сокет уже мав це повідомлення в дорозі. */
  notifyRegardless(snapshot: AccountSnapshot): void {
    this.inFlight?.(snapshot);
  }

  answer(snapshot: AccountSnapshot): Promise<void> {
    this.settle?.(snapshot);
    return Promise.resolve();
  }

  refuse(error: unknown): Promise<void> {
    this.fail?.(error);
    return Promise.resolve();
  }
}

function collect(feed: FakeFeed): { seen: IssueSnapshot[]; stop: () => void } {
  const seen: IssueSnapshot[] = [];
  const stop = watchIssue(feed, ADDRESS, PROGRAM, (snapshot) => seen.push(snapshot));
  return { seen, stop };
}

/** Останнє, що побачила картка. */
function last(seen: IssueSnapshot[]): IssueSnapshot {
  const value = seen.at(-1);
  if (value === undefined) throw new Error('картка не побачила нічого');
  return value;
}

function repaidOf(snapshot: IssueSnapshot): bigint {
  if (snapshot.status !== 'live')
    throw new Error(`очікувався живий випуск, а не ${snapshot.status}`);
  return snapshot.issue.repaidTotal;
}

describe('лічильник погашення на пушах вузла (FR-023, FR-033)', () => {
  it('підписується перед читанням, інакше зміна між ними не дійшла б нікуди', () => {
    const feed = new FakeFeed();
    collect(feed);

    expect(feed.calls).toEqual(['watch', 'fetch']);
  });

  it('показує те, що прочитано з акаунта', async () => {
    const feed = new FakeFeed();
    const { seen } = collect(feed);

    await feed.answer({ slot: 10, account: account({ repaidTotal: 720n * USDC }) });

    expect(repaidOf(last(seen))).toBe(720n * USDC);
  });

  it('рухає лічильник пушем вузла, не читаючи акаунт удруге', async () => {
    const feed = new FakeFeed();
    const { seen } = collect(feed);
    await feed.answer({ slot: 10, account: account({ repaidTotal: 720n * USDC }) });

    feed.notify({ slot: 11, account: account({ repaidTotal: 756n * USDC }) });

    expect(repaidOf(last(seen))).toBe(756n * USDC);
    expect(feed.calls.filter((call) => call === 'fetch')).toHaveLength(1);
  });

  it('не відкочує лічильник відповіддю зі старішого слота', async () => {
    const feed = new FakeFeed();
    const { seen } = collect(feed);

    // Пуш випередив відповідь на початкове читання — на живому вузлі це
    // звичайна річ, бо це два різні канали.
    feed.notify({ slot: 12, account: account({ repaidTotal: 756n * USDC }) });
    await feed.answer({ slot: 10, account: account({ repaidTotal: 720n * USDC }) });

    expect(repaidOf(last(seen))).toBe(756n * USDC);
  });

  it('після відписки не показує нічого з читання, яке ще не повернулось', async () => {
    const feed = new FakeFeed();
    const { seen, stop } = collect(feed);

    stop();
    await feed.answer({ slot: 10, account: account({ repaidTotal: 720n * USDC }) });

    expect(seen).toHaveLength(0);
  });

  it('після відписки не показує нічого, навіть якщо пуш уже був у дорозі', async () => {
    const feed = new FakeFeed();
    const { seen, stop } = collect(feed);
    await feed.answer({ slot: 10, account: account({ repaidTotal: 720n * USDC }) });
    const afterRead = seen.length;

    stop();
    feed.notifyRegardless({ slot: 11, account: account({ repaidTotal: 756n * USDC }) });

    expect(feed.unwatched).toBe(1);
    expect(seen).toHaveLength(afterRead);
  });
});

describe('невідоме не показується нулем', () => {
  it('вузол не відповів — стан невідомий, а не нульовий', async () => {
    const feed = new FakeFeed();
    const { seen } = collect(feed);

    await feed.refuse(new Error('fetch failed'));

    expect(last(seen)).toEqual({ status: 'unreachable', detail: 'fetch failed' });
  });

  it('відмова читання не затирає пуш, який уже прийшов', async () => {
    const feed = new FakeFeed();
    const { seen } = collect(feed);

    feed.notify({ slot: 12, account: account({ repaidTotal: 756n * USDC }) });
    await feed.refuse(new Error('fetch failed'));

    expect(repaidOf(last(seen))).toBe(756n * USDC);
  });

  it('акаунта немає — явна відсутність, а не випуск без виплат', () => {
    expect(interpret({ slot: 7, account: null }, PROGRAM)).toEqual({ status: 'missing', slot: 7 });
  });

  it('акаунт чужої програми не стає випуском від того, що байти схожі', () => {
    const foreign = interpret({ slot: 7, account: account({}, OTHER_PROGRAM) }, PROGRAM);

    expect(foreign.status).toBe('foreign');
  });

  it('байти, які не розбираються, — названа відмова, а не нулі', () => {
    const broken = interpret(
      { slot: 7, account: { owner: PROGRAM, data: new Uint8Array(32) } },
      PROGRAM,
    );

    expect(broken).toMatchObject({ status: 'undecodable', reason: 'wrong-size' });
  });
});

describe('три числа FR-023', () => {
  /** Випуск береться через декодер, а не збирається літералом: числа мусять
   *  пройти той самий шлях із байтів, яким вони йдуть із вузла. */
  const view = (repaidTotal: bigint, obligationTotal: bigint) => {
    const snapshot = interpret(
      { slot: 1, account: account({ repaidTotal, obligationTotal }) },
      PROGRAM,
    );
    if (snapshot.status !== 'live') throw new Error(`акаунт не розібрався: ${snapshot.status}`);
    return repaymentView(snapshot.issue);
  };

  it('залишок і частка рахуються від того, що записано в акаунті', () => {
    const progress = view(168_356n * USDC, 273_750n * USDC);

    expect(progress.remaining).toBe(105_394n * USDC);
    // 61.5008…% округлено **вниз**, як і решта арифметики проєкту: показати
    // більше, ніж виплачено, картка не має права.
    expect(progress.pct).toBe(61.49);
    expect(progress.settled).toBe(false);
  });

  it('частка не губить найменшої одиниці на числах розміру u64', () => {
    // Однієї одиниці не вистачає до повного погашення. `number` такий дріб не
    // тримає: `Number(10n ** 18n - 1n)` дорівнює рівно 10^18, тож ділення через
    // нього дало б 100%, і картка оголосила б погашеним випуск, який винен.
    const progress = view(10n ** 18n - 1n, 10n ** 18n);

    expect(progress.pct).toBe(99.99);
    expect(progress.settled).toBe(false);
    expect(progress.remaining).toBe(1n);
  });

  it('сума для показу не округлюється мимохідь', () => {
    // 2^53 + 1 найменших одиниць: `Number(bigint)` на цьому числі вже бреше, і
    // копійка, яку він губить, — це копійка на екрані про гроші.
    expect(fromBaseUnits(9_007_199_254_740_993n)).toBe(9_007_199_254.740993);
  });

  it('повне погашення закриває лічильник, а не переливає його', () => {
    const progress = view(273_750n * USDC, 273_750n * USDC);

    expect(progress.remaining).toBe(0n);
    expect(progress.pct).toBe(100);
    expect(progress.settled).toBe(true);
  });
});
