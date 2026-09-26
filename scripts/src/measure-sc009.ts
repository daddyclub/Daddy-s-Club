/// <reference lib="dom" />
/**
 * Замір `SC-009` на localnet через веб: від виставлення оферти до отримання
 * USDC продавцем — менше 30 секунд.
 *
 * **Що саме тут міряється.** Два браузерні контексти, два гаманці, один
 * екран — `apps/web/src/routes/offers.tsx`, той самий, що бачить людина.
 * Годинник запускається кліком «List the lot» у продавця і спиняється, коли
 * **на сторінці продавця** баланс USDC виріс — тобто коли пуш його рахунку
 * дійшов до екрана. Між цими двома точками: підпис і підтвердження
 * виставлення, поява оферти в стакані покупця (підписка на програму), клік
 * «Buy», підпис і підтвердження купівлі, пуш рахунку продавця.
 *
 * **Гаманці.** Розширення в безголовому Chrome немає, тому кожен контекст
 * реєструє гаманець Wallet Standard ще до завантаження сторінки
 * (`addInitScript`). Підписує він **справжнім ключем**: байти транзакції
 * йдуть у Node (`exposeFunction`), там `partialSign`, і назад. Валідатор
 * фейкового підпису не прийняв би.
 *
 * **Чого в числі немає.** Людини: покупець тисне «Buy», щойно кнопка
 * з'явилась, і гаманець не показує вікна підтвердження. Це машинна підлога
 * шляху, як і прогін `demo` без пауз. І нічого поза localnet: вузол, браузер і
 * веб стоять на одній машині. Повтор на devnet — `T039a`.
 *
 * Запуск (валідатор із трьома програмами і `vite` — див. README поруч):
 *   pnpm --filter @daddys-club/scripts measure:sc009
 */

// DOM-типи — для функцій, які Playwright виконує в сторінці (`registerWallet`,
// предикати `waitForFunction`).

import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';
import { Connection, Keypair, type PublicKey, Transaction } from '@solana/web3.js';
import { type BrowserContext, chromium, type Page } from 'playwright-core';
import { decodeProtocolConfig } from '../../packages/sdk/src/accounts.ts';
import { pledgedShare } from '../../packages/sdk/src/math.ts';
import { configPda } from '../../packages/sdk/src/pda.ts';
import { associatedTokenAddress } from '../../packages/sdk/src/token.ts';
import { CLUB_PROGRAM } from './lib/encode.ts';
import { readTokenAmount } from './lib/read.ts';
import {
  FACE,
  formatUsdc,
  issueProceeds,
  joinIssue,
  openIssue,
  prepareWorld,
  USDC,
} from './lib/world.ts';

const RPC_URL = process.env.RPC_URL ?? 'http://127.0.0.1:8899';
const WEB_URL = process.env.WEB_URL ?? 'http://localhost:5173';
const CHROME = process.env.CHROME_PATH ?? 'C:/Program Files/Google/Chrome/Application/chrome.exe';
const HEADED = process.env.SC009_HEADED === '1';
/** Куди класти знімки екрана після заміру; без змінної знімків немає. */
const SHOTS = process.env.SC009_SHOTS;

/** Бюджет вимоги. Перевищення — результат заміру, не аварія скрипта. */
const BUDGET_MS = 30_000;
/** Скільки чекати на кожен крок, перш ніж визнати прогін зірваним. */
const PATIENCE_MS = 90_000;

/** Лот — п'ятдесят тисяч номіналу з двохсот п'ятдесяти; ціна — 98% номіналу. */
const LOT_TEXT = '50000';
const PRICE_TEXT = '49000';
const LOT = 50_000n * USDC;
const PRICE = 49_000n * USDC;
/** Скільки USDC у покупця: вистачає на лот із запасом. */
const BUYER_CASH = 60_000n * USDC;

const log = (line: string): void => {
  process.stdout.write(`${line}\n`);
};

/** Що видно тесту зі сторінки. Мітки — `data-testid` на екрані оферт. */
const byTestId = (id: string): string => `[data-testid="${id}"]`;

interface WalletSeed {
  readonly name: string;
  readonly address: string;
  readonly publicKey: number[];
}

/**
 * Реєстрація гаманця за Wallet Standard. Функція виконується **в сторінці**
 * до її скриптів, тож вона самодостатня: жодних імпортів, лише те, що
 * передано аргументом. Підпис — через міст `__daddysSign` у Node.
 */
function registerWallet(seed: WalletSeed): void {
  type Bridge = { __daddysSign(bytes: number[]): Promise<number[]> };
  const account = {
    address: seed.address,
    publicKey: new Uint8Array(seed.publicKey),
    chains: ['solana:localnet'],
    features: ['solana:signTransaction'],
    label: seed.name,
  };
  const wallet = {
    version: '1.0.0',
    name: seed.name,
    icon: 'data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciLz4=',
    chains: ['solana:localnet'],
    accounts: [account],
    features: {
      'standard:connect': { version: '1.0.0', connect: async () => ({ accounts: [account] }) },
      'standard:events': { version: '1.0.0', on: () => () => undefined },
      'solana:signTransaction': {
        version: '1.0.0',
        supportedTransactionVersions: ['legacy', 0],
        signTransaction: async (...inputs: { transaction: Uint8Array }[]) =>
          Promise.all(
            inputs.map(async (input) => ({
              signedTransaction: new Uint8Array(
                await (window as unknown as Bridge).__daddysSign(Array.from(input.transaction)),
              ),
            })),
          ),
      },
    },
  };
  const callback = (api: { register(w: unknown): unknown }) => api.register(wallet);
  window.addEventListener('wallet-standard:app-ready', (event) =>
    callback((event as CustomEvent<{ register(w: unknown): unknown }>).detail),
  );
  window.dispatchEvent(new CustomEvent('wallet-standard:register-wallet', { detail: callback }));
}

/** Контекст браузера з гаманцем, що підписує цим ключем і нічим іншим. */
async function walletContext(
  browser: Awaited<ReturnType<typeof chromium.launch>>,
  name: string,
  keypair: Keypair,
): Promise<BrowserContext> {
  const context = await browser.newContext();
  await context.exposeFunction('__daddysSign', (bytes: number[]) => {
    const transaction = Transaction.from(Uint8Array.from(bytes));
    if (transaction.feePayer?.equals(keypair.publicKey) !== true) {
      throw new Error(`${name}: платник транзакції — не цей гаманець`);
    }
    transaction.partialSign(keypair);
    return [...transaction.serialize({ requireAllSignatures: false })];
  });
  await context.addInitScript(registerWallet, {
    name,
    address: keypair.publicKey.toBase58(),
    publicKey: [...keypair.publicKey.toBytes()],
  } satisfies WalletSeed);
  return context;
}

async function usdcOnPage(page: Page): Promise<bigint> {
  const units = await page.getAttribute(byTestId('usdc-balance'), 'data-units');
  if (units === null || units === '') throw new Error('баланс USDC на сторінці ще не прочитано');
  return BigInt(units);
}

/** Чекає, доки дія на сторінці завершиться, і повертає її результат. */
async function settled(page: Page, what: string): Promise<void> {
  const status = page.locator(
    `${byTestId('action-status')}[data-kind="done"], ${byTestId('action-status')}[data-kind="failed"]`,
  );
  await status.waitFor({ timeout: PATIENCE_MS });
  if ((await status.getAttribute('data-kind')) === 'failed') {
    throw new Error(`${what}: ${await status.innerText()}`);
  }
}

async function openOffers(page: Page, issue: PublicKey, wallet: string): Promise<void> {
  await page.goto(`${WEB_URL}/live/issue/${issue.toBase58()}/offers`);
  await page.locator(byTestId(`connect-${wallet}`)).click({ timeout: PATIENCE_MS });
  await page.locator(byTestId('wallet-address')).waitFor({ timeout: PATIENCE_MS });
  await page.waitForFunction(
    (selector) => (document.querySelector(selector)?.getAttribute('data-units') ?? '') !== '',
    byTestId('usdc-balance'),
    { timeout: PATIENCE_MS },
  );
}

interface Lock {
  readonly id: string;
  readonly what: string;
  readonly ok: boolean;
  readonly detail: string;
}

async function main(): Promise<void> {
  const connection = new Connection(RPC_URL, 'confirmed');
  const admin = Keypair.generate();

  // ── Підготовка: поза секундоміром ──────────────────────────────────────────
  const stage = await prepareWorld(connection, admin, [FACE, BUYER_CASH], log);
  const [seller, buyer] = stage.investors;
  if (seller === undefined || buyer === undefined) throw new Error('світ без двох гаманців');
  const issue = await openIssue(stage, log);
  await joinIssue(stage, issue, seller, FACE, log);
  await issueProceeds(stage, issue, log);
  log(`випуск у погашенні: ${issue.issue.toBase58()}`);

  const configAccount = await connection.getAccountInfo(configPda(CLUB_PROGRAM).address);
  if (configAccount === null) throw new Error('конфігу протоколу немає');
  const feeBps = decodeProtocolConfig(configAccount.data).tradingFeeBps;
  const fee = pledgedShare(PRICE, feeBps);
  if (fee === null) throw new Error('комісія поза межами');

  const sellerKey = seller.keypair.publicKey;
  const buyerKey = buyer.keypair.publicKey;
  const buyerBond = associatedTokenAddress(buyerKey, issue.bondMint);
  const sellerBond = associatedTokenAddress(sellerKey, issue.bondMint);
  const before = {
    seller: await readTokenAmount(connection, seller.usdc),
    buyer: await readTokenAmount(connection, buyer.usdc),
    feeVault: await readTokenAmount(connection, stage.feeVault),
  };

  const browser = await chromium.launch({ executablePath: CHROME, headless: !HEADED });
  try {
    const sellerPage = await (await walletContext(browser, 'Seller', seller.keypair)).newPage();
    const buyerPage = await (await walletContext(browser, 'Buyer', buyer.keypair)).newPage();
    // Зелений гейт не дивиться на екран: помилка в консолі — теж результат.
    const consoleErrors: string[] = [];
    for (const [who, page] of [
      ['seller', sellerPage],
      ['buyer', buyerPage],
    ] as const) {
      page.on('console', (message) => {
        if (message.type() === 'error') {
          consoleErrors.push(`${who}: ${message.text()} (${message.location().url})`);
        }
      });
      page.on('pageerror', (error) => consoleErrors.push(`${who}: ${error.message}`));
    }
    await Promise.all([
      openOffers(sellerPage, issue.issue, 'Seller'),
      openOffers(buyerPage, issue.issue, 'Buyer'),
    ]);
    const seenBefore = await usdcOnPage(sellerPage);
    log(
      `сторінки відкриті, гаманці підключені; у продавця на екрані ${formatUsdc(seenBefore)} USDC`,
    );

    await sellerPage.locator(byTestId('list-face')).fill(LOT_TEXT);
    await sellerPage.locator(byTestId('list-price')).fill(PRICE_TEXT);
    const submit = sellerPage.locator(`${byTestId('list-submit')}:enabled`);
    await submit.waitFor({ timeout: PATIENCE_MS });

    // ── Секундомір ───────────────────────────────────────────────────────────
    const t0 = performance.now();
    const since = () => Math.round(performance.now() - t0);
    const marks: Record<string, number> = {};

    await submit.click();

    const listed = settled(sellerPage, 'виставлення').then(() => {
      marks.listingConfirmed = since();
    });

    const bought = (async () => {
      const buy = buyerPage.locator(`${byTestId('offer-row')} ${byTestId('buy')}:enabled`).first();
      await buy.waitFor({ timeout: PATIENCE_MS });
      marks.offerSeenByBuyer = since();
      await buy.click();
      await settled(buyerPage, 'купівля');
      marks.purchaseConfirmed = since();
    })();

    const received = sellerPage
      .waitForFunction(
        ({ selector, was }) => {
          const units = document.querySelector(selector)?.getAttribute('data-units') ?? '';
          return units !== '' && BigInt(units) > BigInt(was);
        },
        { selector: byTestId('usdc-balance'), was: seenBefore.toString() },
        { timeout: PATIENCE_MS },
      )
      .then(() => {
        marks.usdcSeenBySeller = since();
      });

    await Promise.all([listed, bought, received]);
    const totalMs = marks.usdcSeenBySeller ?? Number.NaN;
    // ── Секундомір спинено ───────────────────────────────────────────────────

    const seenAfter = await usdcOnPage(sellerPage);
    const after = {
      seller: await readTokenAmount(connection, seller.usdc),
      buyer: await readTokenAmount(connection, buyer.usdc),
      feeVault: await readTokenAmount(connection, stage.feeVault),
      buyerBond: await readTokenAmount(connection, buyerBond),
      sellerBond: await readTokenAmount(connection, sellerBond),
    };
    await Promise.all([
      sellerPage
        .locator(byTestId('offer-row'))
        .first()
        .waitFor({ state: 'detached', timeout: PATIENCE_MS }),
      buyerPage
        .locator(byTestId('offer-row'))
        .first()
        .waitFor({ state: 'detached', timeout: PATIENCE_MS }),
    ]).catch(() => undefined);
    const rowsLeft =
      (await sellerPage.locator(byTestId('offer-row')).count()) +
      (await buyerPage.locator(byTestId('offer-row')).count());

    // ── Після секундоміра: скасування через той самий екран ───────────────────
    // Замір проходить виставлення й купівлю; скасування (`FR-027`) — ні. Тут
    // продавець виставляє другий лот і забирає його назад кнопкою «Cancel».
    await sellerPage.locator(byTestId('list-face')).fill(LOT_TEXT);
    await sellerPage.locator(byTestId('list-price')).fill(PRICE_TEXT);
    await sellerPage.locator(`${byTestId('list-submit')}:enabled`).click({ timeout: PATIENCE_MS });
    await sellerPage
      .locator(`${byTestId('action-status')}[data-kind="busy"]`)
      .waitFor({ timeout: PATIENCE_MS });
    await settled(sellerPage, 'друге виставлення');
    const cancelButton = sellerPage.locator(
      `${byTestId('offer-row')} ${byTestId('cancel')}:enabled`,
    );
    await cancelButton.waitFor({ timeout: PATIENCE_MS });
    await buyerPage.locator(byTestId('offer-row')).first().waitFor({ timeout: PATIENCE_MS });

    // Сторінка з повним стаканом на телефоні: чи не ширша вона за вікно.
    const overflow: string[] = [];
    for (const [who, page] of [
      ['seller', sellerPage],
      ['buyer', buyerPage],
    ] as const) {
      await page.setViewportSize({ width: 375, height: 900 });
      const wide = await page.evaluate(() => document.documentElement.scrollWidth);
      if (wide > 375) overflow.push(`${who}: ${wide}px`);
      await page.setViewportSize({ width: 1280, height: 900 });
    }

    if (SHOTS !== undefined) {
      mkdirSync(SHOTS, { recursive: true });
      for (const [who, page] of [
        ['seller', sellerPage],
        ['buyer', buyerPage],
      ] as const) {
        for (const width of [1280, 375]) {
          await page.setViewportSize({ width, height: 900 });
          await page.screenshot({
            path: resolve(SHOTS, `sc009-${who}-${width}.png`),
            fullPage: true,
          });
        }
        await page.setViewportSize({ width: 1280, height: 900 });
      }
      log(`знімки: ${SHOTS}`);
    }

    const bondBeforeCancel = await readTokenAmount(connection, sellerBond);
    await cancelButton.click();
    await sellerPage
      .locator(`${byTestId('action-status')}[data-kind="busy"]`)
      .waitFor({ timeout: PATIENCE_MS });
    await settled(sellerPage, 'скасування');
    const bondAfterCancel = await readTokenAmount(connection, sellerBond);
    await sellerPage
      .locator(byTestId('offer-row'))
      .first()
      .waitFor({ state: 'detached', timeout: PATIENCE_MS });

    const toSeller = PRICE - fee;
    const locks: Lock[] = [
      {
        id: 'L1',
        what: 'продавець отримав ціну мінус комісію',
        ok: after.seller - before.seller === toSeller,
        detail: `+${formatUsdc(after.seller - before.seller)}, очікувано ${formatUsdc(toSeller)}`,
      },
      {
        id: 'L2',
        what: 'скарбниця отримала рівно комісію за ставкою протоколу',
        ok: after.feeVault - before.feeVault === fee,
        detail: `+${formatUsdc(after.feeVault - before.feeVault)} за ${feeBps} bps`,
      },
      {
        id: 'L3',
        what: 'покупець заплатив рівно ціну оферти',
        ok: before.buyer - after.buyer === PRICE,
        detail: `−${formatUsdc(before.buyer - after.buyer)}`,
      },
      {
        id: 'L4',
        what: 'лот у покупця, решта в продавця',
        ok: after.buyerBond === LOT && after.sellerBond === FACE - LOT,
        detail: `покупець ${formatUsdc(after.buyerBond)}, продавець ${formatUsdc(after.sellerBond)} номіналу`,
      },
      {
        id: 'L5',
        what: 'екран продавця показує той самий баланс, що й ланцюг',
        ok: seenAfter === after.seller,
        detail: `екран ${formatUsdc(seenAfter)}, ланцюг ${formatUsdc(after.seller)}`,
      },
      {
        id: 'L6',
        what: 'викуплена оферта зникла з обох стаканів',
        ok: rowsLeft === 0,
        detail: `рядків лишилось: ${rowsLeft}`,
      },
      {
        id: 'L8',
        what: 'скасування через екран повернуло лот цілим',
        ok: bondAfterCancel - bondBeforeCancel === LOT,
        detail: `+${formatUsdc(bondAfterCancel - bondBeforeCancel)} номіналу`,
      },
      {
        id: 'L9',
        what: 'жодної помилки в консолі обох сторінок',
        ok: consoleErrors.length === 0,
        detail: consoleErrors.length === 0 ? 'чисто' : consoleErrors.join(' | '),
      },
      {
        id: 'L10',
        what: 'на 375 px сторінка не ширша за вікно',
        ok: overflow.length === 0,
        detail: overflow.length === 0 ? 'без горизонтальної прокрутки' : overflow.join(', '),
      },
      {
        id: 'L7',
        what: `уклалось у бюджет SC-009 (${BUDGET_MS / 1000} с)`,
        ok: totalMs < BUDGET_MS,
        detail: `${totalMs} мс`,
      },
    ];

    log('');
    log(`SC-009: від кліку «List» до USDC на екрані продавця — ${totalMs} мс`);
    for (const [name, value] of Object.entries(marks)) log(`  ${name.padEnd(20)} ${value} мс`);
    for (const lock of locks) log(`${lock.ok ? '✓' : '✗'} ${lock.id} ${lock.what}: ${lock.detail}`);

    const out = resolve(dirname(fileURLToPath(import.meta.url)), '../out/sc009.json');
    mkdirSync(dirname(out), { recursive: true });
    writeFileSync(
      out,
      `${JSON.stringify(
        {
          criterion: 'SC-009',
          budgetMs: BUDGET_MS,
          totalMs,
          marks,
          lot: LOT.toString(),
          price: PRICE.toString(),
          feeBps,
          issue: issue.issue.toBase58(),
          rpc: RPC_URL,
          web: WEB_URL,
          locks,
          at: new Date().toISOString(),
        },
        null,
        2,
      )}\n`,
      'utf8',
    );
    log(`звіт: ${out}`);

    if (locks.some((lock) => !lock.ok)) process.exitCode = 1;
  } finally {
    await browser.close();
  }
}

main().catch((error: unknown) => {
  process.stderr.write(
    `${error instanceof Error ? (error.stack ?? error.message) : String(error)}\n`,
  );
  process.exitCode = 1;
});
