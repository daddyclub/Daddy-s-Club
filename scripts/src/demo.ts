/**
 * `SC-006` на localnet: повний демо-цикл — створити випуск → підписатись двома
 * гаманцями → згенерувати комісії → побачити погашення → забрати виплату —
 * проходиться **вручну** менш ніж за 3 хвилини.
 *
 * **Що цей скрипт робить і чим він не є.** Він не автоматизація демо, а його
 * партитура: дев'ять кроків у тому самому порядку, у якому їх робить людина на
 * показі, кожен зі своїм секундоміром. Запущений із `DEMO_PAUSE=1` він
 * зупиняється перед кожним кроком і чекає на оператора — тоді загальний час і є
 * той самий «вручну», якого просить критерій. Без паузи він міряє **машинну
 * підлогу** циклу: скільки з трихвилинного бюджету з'їдає ланцюг, і скільки
 * лишається людині.
 *
 * **Що входить у число.** Рівно цикл: створення випуску, два гаманці з
 * відкриттям обліку і внеском, видача (без неї випуск не в `Repaying`, і
 * розщеплювати комісію нічому), свопи демо-емітента, поява нового
 * `repaid_total` **на боці інвестора** — з пуша його власного сокета, тим самим
 * портом, що в картці, — і дві виплати з перевіркою, що USDC дійсно прийшли.
 *
 * **Чого в числі немає.** Підняття світу: валідатора, деплою обох програм,
 * параметрів протоколу, розрахункової валюти, реєстрації джерела і фінансування
 * гаманців. Це `prepareWorld`, і секундомір запускається після нього — бо
 * `SC-006` починає цикл зі створення випуску, а не з установки Solana. Немає в
 * ньому й рендеру React і, головне, **підтверджень у гаманці**: на M1 веб
 * тільки читає ланцюг, підписувати транзакції з інтерфейсу нічим, тому
 * дев'ять кроків нижче — це термінал, а не браузер.
 *
 * **Чого замір не доводить.** Нічого поза localnet. Клієнт і валідатор стоять
 * на одній машині: немає ані конкуренції за стан, ані черг, ані мережевої
 * відстані до вузла. Це прийнята межа демо на змішаних даних (`SPEC.md` →
 * Припущення).
 *
 * Запуск (валідатор і програми — див. README поруч):
 *   pnpm --filter @daddys-club/scripts demo
 *   DEMO_PAUSE=1 pnpm --filter @daddys-club/scripts demo   # цикл веде оператор
 */

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { createInterface, type Interface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { Connection, Keypair, type PublicKey } from '@solana/web3.js';
import { z } from 'zod';
import { feedFromConnection } from '../../apps/web/src/lib/rpc.ts';
import { decodeIssue } from '../../packages/sdk/src/accounts.ts';
import { claimable, obligationTotal } from '../../packages/sdk/src/math.ts';
import { readHolder, readIssue, readTokenAmount } from './lib/read.ts';
import {
  claimInstruction,
  COUPON_BPS,
  FACE,
  formatUsdc as usdc,
  type HolderHandle,
  type IssueHandle,
  issueProceeds,
  joinIssue,
  openIssue,
  PLEDGE_BPS,
  prepareWorld,
  send,
  SWAP_AMOUNT_IN,
  swapInstruction,
  USDC,
  type WorldStage,
} from './lib/world.ts';

const RPC_URL = process.env.RPC_URL ?? 'http://127.0.0.1:8899';
/** Адреса картки випуску — те, на що оператор дивиться під час кроків 5 і 7. */
const WEB_URL = process.env.WEB_URL ?? 'http://127.0.0.1:5173';
/** Бюджет `SC-006`. Перевищення — не аварія скрипта, а результат заміру. */
const BUDGET_MS = 180_000;
/** Скільки свопів робить демо-протокол. П'ять — щоб рух лічильника було видно. */
const SWAPS = Number(process.env.DEMO_SWAPS ?? 5);
/** Скільки чекати на пуш вузла, перш ніж визнати цикл зірваним. */
const PATIENCE_MS = 60_000;
/** Цикл веде оператор: скрипт зупиняється перед кожним кроком. */
const PAUSE = process.env.DEMO_PAUSE === '1';

/**
 * Два гаманці й **нерівні лоти**, як просить `SC-006`. Обидва пропонують по
 * 150 000 USDC на номінал 250 000: перший бере свою пропозицію цілком, другому
 * лишається хвіст, і `FR-009` приймає з нього `min(пропозиція, залишок)`.
 * Часткового прийому в демо не уникнути й не варто — саме на ньому випуск і
 * добирається до рівності `raised == face`.
 */
const OFFER = 150_000n * USDC;

/** Комісія демо-свопу — 0.3% входу; в ескроу з неї йде узгоджена частка. */
const FEE_BPS = 30n;
const FEE_PER_SWAP = (SWAP_AMOUNT_IN * FEE_BPS) / 10_000n;
const STEP_PER_SWAP = (FEE_PER_SWAP * BigInt(PLEDGE_BPS)) / 10_000n;
const EXPECTED_INTERCEPTED = STEP_PER_SWAP * BigInt(SWAPS);

const log = (line: string): void => {
  process.stdout.write(`${line}\n`);
};

function seconds(ms: number): string {
  return (ms / 1000).toFixed(1);
}

// ── Секундомір циклу ──────────────────────────────────────────────────────────

interface Step {
  readonly n: number;
  /** Яку з п'яти фаз `SC-006` цей крок обслуговує. */
  readonly phase: string;
  readonly title: string;
  /** Від старту циклу до початку кроку, мс. */
  readonly atMs: number;
  /** Машинний час кроку — без очікування оператора, мс. */
  readonly tookMs: number;
  readonly note: string;
}

const steps: Step[] = [];
let cycleStartedAt = 0;

/**
 * Очікування оператора. Стоїть **між** кроками, а не всередині: тривалість
 * кроку — це машинний час, а загальний час циклу — настінний, і різниця між
 * ними і є та людина, заради якої `SC-006` написаний зі словом «вручну».
 *
 * Інтерфейс один на весь цикл, а не по одному на крок: кожен новий заковтує
 * увесь наявний буфер `stdin`, і наступний крок лишився б без свого рядка.
 *
 * Режим вимагає **живого терміналу**. Ввід із каналу закривається, щойно
 * вичерпається, і питати другий крок уже нікого — тому закритий ввід тут
 * **названа відмова**. Без неї цикл або падав би службовою помилкою readline,
 * або тихо виходив би посеред кроку з нульовим кодом і без звіту, тобто
 * виглядав би пройденим.
 */
let operator: Interface | null = null;
let operatorGone = false;

function operatorInput(): Interface {
  if (operator === null) {
    operator = createInterface({ input: process.stdin, output: process.stdout });
    operator.on('close', () => {
      operatorGone = true;
    });
  }
  return operator;
}

async function waitForOperator(prompt: string): Promise<void> {
  if (!PAUSE) return;
  const rl = operatorInput();
  if (operatorGone) {
    throw new Error(`ввід оператора закритий — крок «${prompt}» нікому підтвердити`);
  }

  let answered = false;
  await new Promise<void>((done, fail) => {
    rl.once('close', () => {
      if (answered) return;
      fail(new Error(`ввід оператора закрився на кроці «${prompt}» — цикл не пройдено`));
    });
    rl.question(`\n▸ ${prompt} — Enter `, () => {
      answered = true;
      done();
    });
  });
}

/** Відпустити термінал: без цього процес не завершиться після останнього кроку. */
function releaseOperator(): void {
  operator?.close();
  operator = null;
}

async function step<T>(
  n: number,
  phase: string,
  title: string,
  run: (note: (line: string) => void) => Promise<T>,
): Promise<T> {
  await waitForOperator(`крок ${n}: ${title}`);
  const from = performance.now();
  const notes: string[] = [];
  const result = await run((line) => {
    notes.push(line);
  });
  const to = performance.now();
  steps.push({
    n,
    phase,
    title,
    atMs: from - cycleStartedAt,
    tookMs: to - from,
    note: notes.join('; '),
  });
  log(
    `${String(n).padStart(2)}. ${title.padEnd(46)} ${seconds(to - cycleStartedAt).padStart(6)} с` +
      `  (+${seconds(to - from)})`,
  );
  for (const line of notes) log(`    ${line}`);
  return result;
}

// ── Замки: що робить прогін заміром саме цього циклу ──────────────────────────

interface Lock {
  readonly id: string;
  readonly what: string;
  readonly held: boolean;
  readonly detail: string;
}

const locks: Lock[] = [];

function lock(id: string, what: string, held: boolean, detail: string): void {
  locks.push({ id, what, held, detail });
}

// ── Спостереження за лічильником із боку інвестора ────────────────────────────

interface Sighting {
  readonly repaid: bigint;
  readonly slot: number;
  readonly atMs: number;
}

interface CounterWatch {
  /** Чекає на пуш, який показує щонайменше `target`. */
  until(target: bigint): Promise<Sighting>;
  /** Усе, що прийшло сокетом, у порядку прибуття. */
  seen(): readonly Sighting[];
  stop(): void;
}

/**
 * Те саме, що робить картка випуску: підписка на акаунт випуску і декодер SDK.
 * Порт узятий із `apps/web/src/lib/rpc.ts` — не копія, а сам модуль, тому «те,
 * що бачить інвестор», тут не метафора.
 */
function watchCounter(connection: Connection, issue: PublicKey): CounterWatch {
  const sightings: Sighting[] = [];
  let waiter: { target: bigint; resolve: (sighting: Sighting) => void } | null = null;

  const feed = feedFromConnection(connection);
  const stop = feed.watch(issue, (snapshot) => {
    if (snapshot.account === null) return;
    const sighting: Sighting = {
      repaid: decodeIssue(snapshot.account.data).repaidTotal,
      slot: snapshot.slot,
      atMs: performance.now(),
    };
    sightings.push(sighting);
    if (waiter !== null && sighting.repaid >= waiter.target) {
      const resolve = waiter.resolve;
      waiter = null;
      resolve(sighting);
    }
  });

  return {
    until(target) {
      const already = sightings.find((sighting) => sighting.repaid >= target);
      if (already !== undefined) return Promise.resolve(already);
      return new Promise((resolve, reject) => {
        waiter = { target, resolve };
        setTimeout(() => {
          if (waiter === null) return;
          waiter = null;
          reject(new Error(`пуш із ${usdc(target)} USDC не прийшов за ${PATIENCE_MS} мс`));
        }, PATIENCE_MS).unref();
      });
    },
    seen: () => sightings,
    stop,
  };
}

// ── Адмін: ключ, яким карбується розрахункова валюта ──────────────────────────

const secretKey = z.array(z.number().int().min(0).max(255)).length(64);

/**
 * Конфіг протоколу — singleton (`FR-036`), а карбувати його розрахункову валюту
 * може лише той, хто створив мінт. Тому на вже піднятому вузлі демо просить того
 * самого адміна: шлях до ключа в `DEMO_ADMIN_KEYPAIR`, формат — `solana-keygen`.
 * Без змінної ключ разовий, і вузол для наступного прогону піднімається з
 * `--reset`.
 */
function admin(path: string | undefined): Keypair {
  if (path === undefined) return Keypair.generate();

  if (existsSync(path)) {
    const parsed = secretKey.safeParse(JSON.parse(readFileSync(path, 'utf8')) as unknown);
    if (!parsed.success) throw new Error(`${path}: не ключ solana-keygen із 64 байтів`);
    return Keypair.fromSecretKey(Uint8Array.from(parsed.data));
  }

  const fresh = Keypair.generate();
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify([...fresh.secretKey]), 'utf8');
  log(`ключ адміна створено: ${path}`);
  return fresh;
}

// ── Бік інвестора ─────────────────────────────────────────────────────────────

/** Те, що інвестор бачить на картці, — з тих самих чисел, що й вона (`FR-023`). */
async function investorView(
  connection: Connection,
  issue: IssueHandle,
  holders: readonly HolderHandle[],
  note: (line: string) => void,
): Promise<void> {
  const state = await readIssue(connection, issue.issue);
  const remaining = state.obligationTotal - state.repaidTotal;
  const percent = (Number(state.repaidTotal) / Number(state.obligationTotal)) * 100;

  note(
    `стан ${state.state}: погашено ${usdc(state.repaidTotal)} із ` +
      `${usdc(state.obligationTotal)} USDC (${percent.toFixed(2)}%), лишилось ${usdc(remaining)}`,
  );

  for (const holder of holders) {
    const checkpoint = await readHolder(connection, holder.holder);
    const claim = claimable(
      state.payoutIndex,
      checkpoint.indexAtCheckpoint,
      holder.accepted,
      checkpoint.accrued,
    );
    if (claim === null) throw new Error('претензія не порахувалась');
    note(
      `${holder.investor.keypair.publicKey.toBase58().slice(0, 8)}…: ` +
        `бонда ${usdc(holder.accepted)}, до виплати ${usdc(claim)} USDC`,
    );
  }
}

/** Виплата: передбачення з арифметики SDK, потім транзакція, потім баланс. */
async function collect(
  stage: WorldStage,
  issue: IssueHandle,
  holder: HolderHandle,
  label: string,
  note: (line: string) => void,
): Promise<{ predicted: bigint; moved: bigint }> {
  const connection = stage.connection;
  const state = await readIssue(connection, issue.issue);
  const checkpoint = await readHolder(connection, holder.holder);
  const predicted = claimable(
    state.payoutIndex,
    checkpoint.indexAtCheckpoint,
    holder.accepted,
    checkpoint.accrued,
  );
  if (predicted === null) throw new Error('претензія не порахувалась');

  const before = await readTokenAmount(connection, holder.investor.usdc);
  await send(connection, [claimInstruction(stage, issue, holder)], [holder.investor.keypair]);
  const after = await readTokenAmount(connection, holder.investor.usdc);
  const moved = after - before;

  note(`${label}: очікувано ${usdc(predicted)}, на рахунку +${usdc(moved)} USDC`);
  lock(
    `Z6.${label}`,
    `виплата ${label} дорівнює арифметиці SDK`,
    moved === predicted,
    `${moved} проти ${predicted}`,
  );
  return { predicted, moved };
}

// ── Цикл ──────────────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  log(`вузол: ${RPC_URL}`);
  log(PAUSE ? 'режим: цикл веде оператор (DEMO_PAUSE=1)' : 'режим: без пауз — машинна підлога');
  log('');

  // Два з'єднання навмисно: емітент, трейдер і інвестори працюють зі свого
  // клієнта, а картка інвестора слухає вузол зі свого. Спільне з'єднання
  // зробило б «інвестор побачив» відповіддю скрипта самому собі.
  const wire = new Connection(RPC_URL, 'confirmed');
  const investorWire = new Connection(RPC_URL, 'confirmed');

  log('── підняття світу (у хронометраж не входить) ──');
  const stage = await prepareWorld(
    wire,
    admin(process.env.DEMO_ADMIN_KEYPAIR),
    [OFFER, OFFER],
    log,
  );
  const [walletA, walletB] = stage.investors;
  if (walletA === undefined || walletB === undefined) throw new Error('потрібні два гаманці');
  log('');

  log('── цикл SC-006 ──');
  cycleStartedAt = performance.now();

  const issue = await step(1, 'створити випуск', 'емітент створив випуск', async (note) => {
    const handle = await openIssue(stage, note);
    const obligation = obligationTotal(FACE, COUPON_BPS);
    note(`номінал ${usdc(FACE)} USDC, зобов'язання ${usdc(obligation ?? 0n)} USDC`);
    return handle;
  });

  const holderA = await step(
    2,
    'підписатись двома гаманцями',
    'гаманець A: облік і внесок',
    (note) => joinIssue(stage, issue, walletA, OFFER, note),
  );

  const holderB = await step(
    3,
    'підписатись двома гаманцями',
    'гаманець B: облік і внесок (хвіст)',
    (note) => joinIssue(stage, issue, walletB, OFFER, note),
  );

  await step(4, 'зв’язка', 'емітент забрав номінал — випуск у погашенні', async (note) => {
    await issueProceeds(stage, issue, note);
    // Замок `Z3` стоїть і в кінці, на підсумковому стані, — але тут він ще й
    // ворота: у стані, який не `Repaying`, перехоплення лише спостерігає, і
    // цикл далі просто чекав би на пуш, якого не буде. Впасти тут із назвою
    // замка дешевше, ніж мовчати хвилину й упасти на таймауті.
    const state = (await readIssue(stage.connection, issue.issue)).state;
    note(`стан випуску: ${state}`);
    if (state !== 'Repaying') {
      throw new Error(`Z3: випуск у стані ${state} — комісія не розщеплюватиметься`);
    }
  });

  const holders = [holderA, holderB] as const;

  const counter = await step(
    5,
    'побачити погашення',
    'інвестор відкрив картку випуску',
    async (note) => {
      const watch = watchCounter(investorWire, issue.issue);
      note(`${WEB_URL}/live/issue/${issue.issue.toBase58()}`);
      await investorView(investorWire, issue, holders, note);
      return watch;
    },
  );

  await step(6, 'згенерувати комісії', `демо-протокол зробив ${SWAPS} свопів`, async (note) => {
    for (let index = 0; index < SWAPS; index += 1) {
      await send(wire, [swapInstruction(stage, issue)], [stage.trader]);
    }
    note(
      `${SWAPS} × ${usdc(SWAP_AMOUNT_IN)} USDC входу → комісія ${usdc(FEE_PER_SWAP)} ` +
        `за своп, у сховище ${usdc(STEP_PER_SWAP)} (${PLEDGE_BPS / 100}%)`,
    );
  });

  await step(7, 'побачити погашення', 'лічильник у інвестора зрушився', async (note) => {
    const sighting = await counter.until(EXPECTED_INTERCEPTED);
    note(
      `пуш вузла на слоті ${sighting.slot}: погашено ${usdc(sighting.repaid)} USDC ` +
        `(очікувано ${usdc(EXPECTED_INTERCEPTED)})`,
    );
    await investorView(investorWire, issue, holders, note);
  });

  const paidA = await step(8, 'забрати виплату', 'гаманець A забрав виплату', (note) =>
    collect(stage, issue, holderA, 'A', note),
  );

  const paidB = await step(9, 'забрати виплату', 'гаманець B забрав виплату', (note) =>
    collect(stage, issue, holderB, 'B', note),
  );

  const finishedAt = performance.now();
  counter.stop();
  releaseOperator();

  // ── Замки ───────────────────────────────────────────────────────────────────

  const finalIssue = await readIssue(wire, issue.issue);
  const escrowLeft = await readTokenAmount(wire, issue.escrowVault);
  const bondSupply = holderA.accepted + holderB.accepted;
  const sightings = counter.seen();
  const monotonic = sightings.every(
    (sighting, index) => index === 0 || sighting.repaid >= (sightings[index - 1]?.repaid ?? 0n),
  );

  lock(
    'Z1',
    'два гаманці разом зібрали рівно номінал',
    holderA.accepted > 0n && holderB.accepted > 0n && bondSupply === FACE,
    `A ${holderA.accepted} + B ${holderB.accepted} проти ${FACE}`,
  );
  lock(
    'Z2',
    'хвіст номіналу прийнято частково (FR-009)',
    holderB.accepted < OFFER && holderB.accepted > 0n,
    `запропоновано ${OFFER}, прийнято ${holderB.accepted}`,
  );
  lock(
    'Z3',
    'комісії йшли у випуск, який у погашенні',
    finalIssue.state === 'Repaying',
    `стан ${finalIssue.state}`,
  );
  lock(
    'Z4',
    'лічильник зрушив рівно на перехоплене',
    finalIssue.repaidTotal === EXPECTED_INTERCEPTED,
    `${finalIssue.repaidTotal} проти ${EXPECTED_INTERCEPTED}`,
  );
  lock(
    'Z5',
    'інвестор побачив рух зі свого сокета, не відкотом назад',
    sightings.length > 0 &&
      monotonic &&
      sightings[sightings.length - 1]?.repaid === EXPECTED_INTERCEPTED,
    `${sightings.length} пушів, монотонність ${String(monotonic)}`,
  );
  lock(
    'Z7',
    'виплати справді прийшли на рахунки, а не лише в облік',
    paidA.moved > 0n && paidB.moved > 0n,
    `A +${paidA.moved}, B +${paidB.moved}`,
  );
  lock(
    'Z8',
    'виплачене плюс залишок ескроу дорівнює перехопленому',
    paidA.moved + paidB.moved + escrowLeft === EXPECTED_INTERCEPTED,
    `${paidA.moved} + ${paidB.moved} + ${escrowLeft} проти ${EXPECTED_INTERCEPTED}`,
  );
  lock(
    'Z9',
    'цикл уклався в бюджет SC-006',
    finishedAt - cycleStartedAt < BUDGET_MS,
    `${seconds(finishedAt - cycleStartedAt)} с проти ${seconds(BUDGET_MS)} с`,
  );

  // ── Звіт ────────────────────────────────────────────────────────────────────

  const machineMs = steps.reduce((sum, one) => sum + one.tookMs, 0);
  const wallMs = finishedAt - cycleStartedAt;

  log('');
  log('── хронометраж ──');
  log(`кроків: ${steps.length}`);
  log(`настінний час циклу: ${seconds(wallMs)} с із бюджету ${seconds(BUDGET_MS)} с`);
  log(
    `з них машинного: ${seconds(machineMs)} с; людині лишається ${seconds(BUDGET_MS - machineMs)} с`,
  );
  log('');
  log('── замки ──');
  for (const one of locks) {
    log(`${one.held ? '✅' : '❌'} ${one.id.padEnd(6)} ${one.what} — ${one.detail}`);
  }

  const broken = locks.filter((one) => !one.held);

  const report = {
    criterion: 'SC-006',
    cluster: 'localnet',
    rpcUrl: RPC_URL,
    operatorDriven: PAUSE,
    budgetMs: BUDGET_MS,
    wallMs: Number(wallMs.toFixed(1)),
    machineMs: Number(machineMs.toFixed(1)),
    swaps: SWAPS,
    interceptedUnits: EXPECTED_INTERCEPTED.toString(),
    faceUnits: FACE.toString(),
    lots: { a: holderA.accepted.toString(), b: holderB.accepted.toString() },
    claimed: { a: paidA.moved.toString(), b: paidB.moved.toString() },
    escrowLeftUnits: escrowLeft.toString(),
    pushes: sightings.length,
    issue: issue.issue.toBase58(),
    brokenLocks: broken.map((one) => one.id),
    takenAt: new Date().toISOString(),
    steps,
    locks,
  };

  const here = dirname(fileURLToPath(import.meta.url));
  const target = resolve(here, '../out/sc006.json');
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, `${JSON.stringify(report, null, 2)}\n`, 'utf8');
  log('');
  log(`звіт: ${target}`);

  // Цикл, у якому щось порахувалось не тим числом, — це не замір швидкості, а
  // замір чогось іншого. Такий прогін не має мовчки стати цифрою в `TASKS.md`.
  if (broken.length > 0) process.exitCode = 1;
}

main().then(
  () => process.exit(process.exitCode ?? 0),
  (error: unknown) => {
    log(`цикл зірвано: ${error instanceof Error ? error.stack : String(error)}`);
    process.exit(1);
  },
);
