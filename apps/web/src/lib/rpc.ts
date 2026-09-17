/**
 * Межа з вузлом. Усе, що застосунок бере з мережі, проходить звідси — і
 * проходить як `unknown`, доки його не розбере декодер SDK.
 *
 * Порт вузький навмисно. `Connection` з `@solana/web3.js` уміє сотню речей;
 * картці випуску потрібні дві — прочитати акаунт і слухати його зміни. Вузький
 * порт дає рівно те, чого без нього не було б: `watchIssue` ганяється тестом
 * без мережі, без вузла і без браузера.
 *
 * **Слот їде разом із даними.** Без нього відповідь на початкове читання може
 * лягти поверх свіжішого пуша — це не теорія, а звичайна гонка: підписка
 * ставиться першою, і повідомлення про зміну має право прийти раніше, ніж
 * повернеться `getAccountInfo`. Хто новіший, вирішує номер слота, а не порядок
 * прибуття.
 */

import { Connection, type PublicKey } from '@solana/web3.js';

/** Акаунт так, як його віддає вузол: власник відомий, вміст — ще ні. */
export interface RawAccount {
  readonly owner: PublicKey;
  readonly data: unknown;
}

/** Стан адреси на певному слоті. `null` — акаунта за адресою немає. */
export interface AccountSnapshot {
  readonly slot: number;
  readonly account: RawAccount | null;
}

/** Те, чого картці треба від вузла, і нічого понад те. */
export interface AccountFeed {
  /** Разове читання. Відмова мережі — відхилена обіцянка, не `null`. */
  fetch(address: PublicKey): Promise<AccountSnapshot>;
  /**
   * Підписка на зміни акаунта (`FR-033`). Повертає відписку; після неї жодного
   * виклику `onSnapshot` бути не має.
   */
  watch(address: PublicKey, onSnapshot: (snapshot: AccountSnapshot) => void): () => void;
}

/**
 * Рівень підтвердження. `confirmed` — те, що бачить сам відправник, коли його
 * транзакція «пройшла»; саме від цієї миті `SC-002` рахує свої десять секунд.
 * `finalized` додав би до заміру ще близько тридцяти слотів очікування, яких
 * вимога не просить.
 */
export const COMMITMENT = 'confirmed';

/** Адаптер поверх `Connection`. Тонкий навмисно: тут нічого перевіряти. */
export function feedFromConnection(connection: Connection): AccountFeed {
  return {
    async fetch(address) {
      const response = await connection.getAccountInfoAndContext(address, COMMITMENT);
      const value = response.value;
      return {
        slot: response.context.slot,
        account: value === null ? null : { owner: value.owner, data: value.data },
      };
    },

    watch(address, onSnapshot) {
      const id = connection.onAccountChange(
        address,
        (info, context) => {
          onSnapshot({
            slot: context.slot,
            account: { owner: info.owner, data: info.data },
          });
        },
        COMMITMENT,
      );

      return () => {
        // Відписка асинхронна, а зупинка потоку — ні: `watchIssue` глушить
        // виклики сам, щойно його попросили спинитись. Тому відмова тут нікого
        // не цікавить — сокета вже може не бути.
        void connection.removeAccountChangeListener(id).catch(() => undefined);
      };
    },
  };
}

let shared: AccountFeed | null = null;

/**
 * Спільне з'єднання застосунку. Створюється лінькаво: модуль, який читає
 * конфігурацію при завантаженні, тягне її і в тести, яким вона не потрібна.
 */
export function sharedFeed(rpcUrl: string): AccountFeed {
  shared ??= feedFromConnection(new Connection(rpcUrl, COMMITMENT));
  return shared;
}
