/**
 * Замір `SC-002` на localnet: від підтвердження транзакції з комісією до
 * оновлення лічильника погашення в інтерфейсі — менше 10 секунд (p95).
 *
 * **Що саме тут міряється.** Дві сторони, два різні з'єднання, як у житті:
 * трейдер надсилає своп зі свого клієнта, власник бонда дивиться на картку в
 * своєму браузері. Годинник запускається тоді, коли **клієнт трейдера дізнався,
 * що транзакція підтверджена** (`commitment: confirmed` — те саме, що показує
 * гаманець), і спиняється тоді, коли на боці власника з пуша вузла розібрався
 * новий `repaid_total`.
 *
 * **Чого в цьому числі немає.** Рендеру React. Підписка й декодер тут ті самі,
 * що в картці (`apps/web/src/lib/rpc.ts`, декодер SDK), а от намалювати вже
 * готове число браузер устигає за частки мілісекунди при бюджеті в десять
 * секунд. Це єдина ланка ланцюга, яку замір не проходить, і вона названа.
 *
 * **Друге число — від надсилання, а не від підтвердження.** Пуш підписки має
 * право прийти раніше, ніж клієнт трейдера дізнається про підтвердження: це два
 * різні канали, і швидший з них не зобов'язаний чекати повільнішого. Тоді
 * різниця, якої просить `SC-002`, виходить від'ємною — критерій виконано, але
 * саме число нічого не каже про шлях. Тому поруч рахується інтервал від
 * **надсилання** свопу: він завжди додатний і накриває весь шлях цілком.
 *
 * **Чого замір не доводить.** Нічого поза localnet. Там немає ані конкуренції
 * за стан, ані черг, ані затримки до вузла через мережу; поведінка при реальних
 * обсягах mainnet цим не перевіряється. Це прийнята межа демо (`SPEC.md` →
 * Припущення, `PLAN.md` → «Жодна віха не доводить `SC-002` на mainnet»).
 *
 * Запуск (валідатор і програми — див. README поруч):
 *   pnpm --filter @daddys-club/scripts measure:sc002
 */

import { writeFileSync } from 'node:fs';
import { mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Connection, Keypair, type PublicKey, Transaction } from '@solana/web3.js';
import { feedFromConnection } from '../../apps/web/src/lib/rpc.ts';
import { decodeIssue } from '../../packages/sdk/src/accounts.ts';
import { PLEDGE_BPS, standUpWorld, SWAP_AMOUNT_IN, USDC } from './lib/world.ts';

/** Скільки замірів. Двадцять — те, що просить рядок `SC-002` у `TASKS.md`. */
const SAMPLES = Number(process.env.SC002_SAMPLES ?? 20);
const RPC_URL = process.env.RPC_URL ?? 'http://127.0.0.1:8899';
/** Бюджет вимоги. Перевищення — не аварія скрипта, а результат заміру. */
const BUDGET_MS = 10_000;
/** Скільки чекати на пуш, перш ніж визнати замір зірваним. */
const PATIENCE_MS = 60_000;

/** Комісія демо-свопу — 0.3% входу; в ескроу з неї йде узгоджена частка. */
const FEE_BPS = 30n;
const EXPECTED_STEP = (((SWAP_AMOUNT_IN * FEE_BPS) / 10_000n) * BigInt(PLEDGE_BPS)) / 10_000n;

interface Sample {
  readonly index: number;
  /** `SC-002`: від підтвердження до оновлення лічильника, мс. */
  readonly confirmedToCounterMs: number;
  /** Той самий шлях, але від надсилання свопу, мс. */
  readonly sentToCounterMs: number;
  /** Наскільки зрушив лічильник — доказ, що заміряно саме цю комісію. */
  readonly step: string;
  readonly slot: number;
  readonly signature: string;
}

const log = (line: string): void => {
  process.stdout.write(`${line}\n`);
};

/** Нерангова p95: значення під номером ceil(0.95·n) у зростаючому порядку. */
function percentile(values: number[], fraction: number): number {
  const sorted = [...values].sort((left, right) => left - right);
  const rank = Math.max(1, Math.ceil(fraction * sorted.length));
  const value = sorted[rank - 1];
  if (value === undefined) throw new Error('порожній набір замірів');
  return value;
}

/**
 * Чекає, доки картка побачить наступний крок лічильника.
 *
 * Обіцянка ставиться **до** надсилання свопу: підписка вже стоїть, і
 * повідомлення має право прийти раніше, ніж повернеться підтвердження.
 */
interface CounterWatch {
  next(previous: bigint): Promise<{ at: number; repaid: bigint; slot: number }>;
  stop(): void;
}

function watchCounter(connection: Connection, issue: PublicKey): CounterWatch {
  let waiter: ((update: { at: number; repaid: bigint; slot: number }) => void) | null = null;
  let target = -1n;

  const feed = feedFromConnection(connection);
  const stop = feed.watch(issue, (snapshot) => {
    if (snapshot.account === null) return;
    // Декодер той самий, що в картці: у число `SC-002` входить і розбір байтів.
    const repaid = decodeIssue(snapshot.account.data).repaidTotal;
    const at = performance.now();
    if (waiter === null || repaid <= target) return;
    const resolve = waiter;
    waiter = null;
    resolve({ at, repaid, slot: snapshot.slot });
  });

  return {
    next(previous) {
      target = previous;
      return new Promise((resolve, reject) => {
        waiter = resolve;
        setTimeout(() => {
          if (waiter === null) return;
          waiter = null;
          reject(new Error(`пуш не прийшов за ${PATIENCE_MS} мс`));
        }, PATIENCE_MS).unref();
      });
    },
    stop,
  };
}

async function main(): Promise<void> {
  log(`вузол: ${RPC_URL}`);

  // Два з'єднання навмисно: сокет трейдера і сокет власника бонда — різні, як
  // і в житті. Спільне з'єднання зробило б замір заміром самого себе.
  const sender = new Connection(RPC_URL, 'confirmed');
  const subscriber = new Connection(RPC_URL, 'confirmed');

  const world = await standUpWorld(sender, Keypair.generate(), log);
  log('');

  const counter = watchCounter(subscriber, world.issue);
  const samples: Sample[] = [];
  let repaid = 0n;

  for (let index = 1; index <= SAMPLES; index += 1) {
    const pending = counter.next(repaid);

    const transaction = new Transaction().add(world.swapInstruction());
    const { blockhash, lastValidBlockHeight } = await sender.getLatestBlockhash('confirmed');
    transaction.recentBlockhash = blockhash;
    transaction.feePayer = world.trader.publicKey;
    transaction.sign(world.trader);

    const sentAt = performance.now();
    const signature = await sender.sendRawTransaction(transaction.serialize());
    await sender.confirmTransaction({ signature, blockhash, lastValidBlockHeight }, 'confirmed');
    const confirmedAt = performance.now();

    const update = await pending;
    const step = update.repaid - repaid;
    repaid = update.repaid;

    samples.push({
      index,
      confirmedToCounterMs: Number((update.at - confirmedAt).toFixed(1)),
      sentToCounterMs: Number((update.at - sentAt).toFixed(1)),
      step: step.toString(),
      slot: update.slot,
      signature,
    });

    const last = samples[samples.length - 1];
    if (last === undefined) throw new Error('замір не записався');
    log(
      `${String(index).padStart(2)} / ${SAMPLES}  ` +
        `підтвердження → лічильник ${String(last.confirmedToCounterMs).padStart(8)} мс  ` +
        `надсилання → лічильник ${String(last.sentToCounterMs).padStart(8)} мс  ` +
        `крок ${step} (очікувано ${EXPECTED_STEP})`,
    );
  }

  counter.stop();

  const confirmed = samples.map((sample) => sample.confirmedToCounterMs);
  const sent = samples.map((sample) => sample.sentToCounterMs);
  const wrongStep = samples.filter((sample) => sample.step !== EXPECTED_STEP.toString());

  const report = {
    criterion: 'SC-002',
    cluster: 'localnet',
    rpcUrl: RPC_URL,
    samples: samples.length,
    budgetMs: BUDGET_MS,
    swapAmountInUsdc: (SWAP_AMOUNT_IN / USDC).toString(),
    expectedStepUnits: EXPECTED_STEP.toString(),
    stepMismatches: wrongStep.length,
    confirmedToCounterMs: {
      min: Math.min(...confirmed),
      median: percentile(confirmed, 0.5),
      p95: percentile(confirmed, 0.95),
      max: Math.max(...confirmed),
    },
    sentToCounterMs: {
      min: Math.min(...sent),
      median: percentile(sent, 0.5),
      p95: percentile(sent, 0.95),
      max: Math.max(...sent),
    },
    issue: world.issue.toBase58(),
    takenAt: new Date().toISOString(),
    measurements: samples,
  };

  const here = dirname(fileURLToPath(import.meta.url));
  const target = resolve(here, '../out/sc002.json');
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, `${JSON.stringify(report, null, 2)}\n`, 'utf8');

  log('');
  log(`заміри: ${report.samples}, бюджет ${BUDGET_MS} мс`);
  log(
    `підтвердження → лічильник: p95 ${report.confirmedToCounterMs.p95} мс ` +
      `(медіана ${report.confirmedToCounterMs.median}, max ${report.confirmedToCounterMs.max})`,
  );
  log(
    `надсилання   → лічильник: p95 ${report.sentToCounterMs.p95} мс ` +
      `(медіана ${report.sentToCounterMs.median}, max ${report.sentToCounterMs.max})`,
  );
  log(`кроків лічильника не тим числом: ${report.stepMismatches}`);
  log(`звіт: ${target}`);

  // Замір, у якому лічильник рухався не на ту суму, — це не замір швидкості, а
  // замір чогось іншого. Такий прогін не має мовчки стати цифрою в `TASKS.md`.
  if (wrongStep.length > 0) process.exitCode = 1;
  if (report.confirmedToCounterMs.p95 >= BUDGET_MS) process.exitCode = 1;
}

main().then(
  () => process.exit(process.exitCode ?? 0),
  (error: unknown) => {
    log(`замір зірвано: ${error instanceof Error ? error.stack : String(error)}`);
    process.exit(1);
  },
);
