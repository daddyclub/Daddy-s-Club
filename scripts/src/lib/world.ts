/**
 * Демо-світ M1 на живому вузлі: від параметрів протоколу до випуску, який
 * погашається.
 *
 * Модуль обслуговує два скрипти, і кожен бере з нього своє. `measure-sc002.ts`
 * потрібен готовий світ одним викликом: критерій рахує час **від підтвердження
 * транзакції з комісією**, а комісія в цьому протоколі буває рівно одна — та,
 * яку `demo_issuer.swap` утримує й тут же розщеплює через CPI (`FR-004`), і щоб
 * такий своп відбувся, на вузлі мусить стояти весь ланцюжок. `demo.ts` навпаки
 * потребує світ **розібраним на кроки**: `SC-006` міряє цикл, у якому створення
 * випуску, дві підписки й виплата — окремі дії з окремим хронометражем.
 *
 * Звідси форма модуля: підготовка (`prepareWorld`) доводить вузол рівно до
 * межі, за якою починається цикл `SC-006`, а кожен крок циклу — окрема
 * експортована функція. `standUpWorld` лишається тим, чим був: складанням цих
 * кроків в один виклик для заміру `SC-002`.
 *
 * Усе тут — справжні інструкції обох програм. Підроблених акаунтів немає: стан,
 * підкладений у сховище, довів би лише те, що підробка узгоджена сама з собою.
 */

import {
  ComputeBudgetProgram,
  type Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SYSVAR_CLOCK_PUBKEY,
  SystemProgram,
  sendAndConfirmTransaction,
  Transaction,
  type TransactionInstruction,
} from '@solana/web3.js';
import { decodeProtocolConfig, decodeRevenueSource } from '../../../packages/sdk/src/accounts.ts';
import {
  configPda,
  extraAccountMetasPda,
  holderPda,
  issuePda,
  sourcePda,
} from '../../../packages/sdk/src/pda.ts';
import { type Cluster, detectCluster } from './cluster.ts';
import {
  Args,
  associatedTokenAddress,
  CLUB_PROGRAM,
  createAssociatedTokenAccountIdempotent,
  DEMO_PROGRAM,
  initializeAccount3,
  initializeMint2,
  instruction,
  MINT_SIZE,
  mintTo,
  ro,
  rw,
  SYSTEM_PROGRAM,
  signer,
  signerRw,
  TOKEN_2022,
  TOKEN_ACCOUNT_SIZE,
} from './encode.ts';
import { readTokenAmount } from './read.ts';

/** Розрахункова валюта демо-світу. Шість знаків, як у `tests/harness.rs`. */
export const USDC_DECIMALS = 6;
export const USDC = 10n ** BigInt(USDC_DECIMALS);

/** Умови випуску. Ті самі числа, що на демо-картці M0 — щоб показ був упізнаваний. */
export const FACE = 250_000n * USDC;
export const MIN_LOT = 1_000n * USDC;
export const COUPON_BPS = 950;
export const PLEDGE_BPS = 1_200;
const TENOR_SECS = 90n * 86_400n;
const SUBSCRIPTION_SECS = 7n * 86_400n;

/** Скільки USDC заходить в один своп. Комісія з нього — 0.3% (`demo_issuer`). */
export const SWAP_AMOUNT_IN = 100_000n * USDC;

/**
 * `history_threshold_secs` of a config this module creates (localnet). Short on
 * purpose: SPEC → Assumptions sets the demo threshold low and has the demo
 * issuer build the history beforehand, otherwise the three-minute cycle of
 * `SC-006` is out of reach by definition. Devnet carries the same value, set by
 * `update-config.ts`.
 */
export const DEMO_HISTORY_THRESHOLD_SECS = 120n;

/** Swaps run through a fresh source before its first issue (`FR-007`, `FR-028`). */
const HISTORY_SWAPS = 2;

/** How often the chain clock is read while the history threshold runs out. */
const CLOCK_POLL_MS = 5_000;

/**
 * Сума в USDC із розділювачем тисяч і двома знаками — **тільки для показу**.
 * Уся арифметика лишається в найменших одиницях: два знаки на екрані і два
 * знаки в розрахунку — різні речі, і плутати їх у демо про гроші не можна.
 */
export function formatUsdc(amount: bigint): string {
  const whole = (amount / USDC).toString().replace(/\B(?=(\d{3})+(?!\d))/g, ' ');
  const cents = ((amount % USDC) / 10_000n).toString().padStart(2, '0');
  return `${whole}.${cents}`;
}

/** PDA пулу демо-емітента — той ключ, чий **підпис** автентифікує перехоплення. */
export const POOL = PublicKey.findProgramAddressSync([Buffer.from('pool')], DEMO_PROGRAM)[0];

/** Гаманець інвестора разом із його рахунком розрахункової валюти. */
export interface InvestorWallet {
  readonly keypair: Keypair;
  /** Звідси йде внесок, сюди приходить виплата — один і той самий рахунок. */
  readonly usdc: PublicKey;
}

/**
 * Світ до початку циклу `SC-006`.
 *
 * Межа проведена там, де її проводить сама вимога: цикл починається зі
 * **створення випуску**, тому конфіг протоколу, розрахункова валюта, джерело
 * revenue і профінансовані гаманці — це підготовка, а не крок циклу. Хронометраж
 * `demo.ts` починається після `prepareWorld`, і саме це й треба сказати вголос,
 * показуючи число.
 */
export interface WorldStage {
  readonly connection: Connection;
  readonly cluster: Cluster;
  readonly admin: Keypair;
  readonly issuer: Keypair;
  readonly trader: Keypair;
  readonly investors: readonly InvestorWallet[];
  readonly config: PublicKey;
  readonly usdcMint: PublicKey;
  readonly feeVault: PublicKey;
  readonly baseMint: PublicKey;
  readonly source: PublicKey;
  readonly sourceVault: PublicKey;
  readonly poolUsdc: PublicKey;
  readonly poolBase: PublicKey;
  readonly issuerUsdc: PublicKey;
  readonly traderUsdc: PublicKey;
  readonly traderBase: PublicKey;
}

/** Випуск разом із трьома акаунтами, які створюються тією ж інструкцією. */
export interface IssueHandle {
  readonly issue: PublicKey;
  readonly bondMint: PublicKey;
  readonly escrowVault: PublicKey;
  readonly subscriptionVault: PublicKey;
}

/** Власник бонда: облік, рахунок бонду і скільки з його пропозиції прийняли. */
export interface HolderHandle {
  readonly investor: InvestorWallet;
  readonly holder: PublicKey;
  readonly bond: PublicKey;
  /** `FR-009`: прийнято `min(пропозиція, залишок)`, і це число читається з ланцюга. */
  readonly accepted: bigint;
}

export interface DemoWorld {
  readonly issue: PublicKey;
  readonly source: PublicKey;
  readonly bondMint: PublicKey;
  readonly escrowVault: PublicKey;
  readonly trader: Keypair;
  /** Своп, який утримує комісію й розщеплює її в тій самій транзакції. */
  swapInstruction(): TransactionInstruction;
}

/**
 * SOL for the one-off wallets of a run. On localnet it is airdropped; devnet's
 * faucet allows a couple of SOL a day, so there the admin pays by transfer and
 * gets the remainder back through `reclaim`. The amounts are what a wallet
 * spends on rent and fees in one run, with room to spare.
 */
const WALLET_SOL: Readonly<Record<'localnet' | 'remote', { issuer: number; other: number }>> = {
  localnet: { issuer: 10, other: 10 },
  remote: { issuer: 0.1, other: 0.05 },
};

async function fund(
  connection: Connection,
  cluster: Cluster,
  admin: Keypair,
  who: PublicKey,
  sol: number,
): Promise<void> {
  const lamports = Math.round(sol * LAMPORTS_PER_SOL);
  if (cluster === 'localnet') {
    const signature = await connection.requestAirdrop(who, lamports);
    const latest = await connection.getLatestBlockhash();
    await connection.confirmTransaction({ signature, ...latest }, 'confirmed');
    return;
  }
  await send(
    connection,
    [SystemProgram.transfer({ fromPubkey: admin.publicKey, toPubkey: who, lamports })],
    [admin],
  );
}

/**
 * Returns what is left on the one-off wallets of a run to the admin. A no-op on
 * localnet. Meant for a `finally`: a run that fails halfway would otherwise
 * strand the SOL on keys nobody keeps.
 */
export async function reclaim(stage: WorldStage, log: (line: string) => void): Promise<void> {
  if (stage.cluster === 'localnet') return;
  const fee = 5_000;
  let returned = 0;
  for (const wallet of [stage.issuer, stage.trader, ...stage.investors.map((i) => i.keypair)]) {
    const balance = await stage.connection.getBalance(wallet.publicKey, 'confirmed');
    if (balance <= fee) continue;
    await send(
      stage.connection,
      [
        SystemProgram.transfer({
          fromPubkey: wallet.publicKey,
          toPubkey: stage.admin.publicKey,
          lamports: balance - fee,
        }),
      ],
      [wallet],
    );
    returned += balance - fee;
  }
  log(`returned ${(returned / LAMPORTS_PER_SOL).toFixed(4)} SOL to the admin`);
}

/** Надіслати й дочекатись підтвердження. `confirmed` — те, що бачить гаманець. */
export async function send(
  connection: Connection,
  instructions: TransactionInstruction[],
  signers: Keypair[],
): Promise<string> {
  const transaction = new Transaction().add(...instructions);
  return sendAndConfirmTransaction(connection, transaction, signers, {
    commitment: 'confirmed',
    skipPreflight: false,
  });
}

/** Мінт Token-2022 без розширень: розрахункова валюта і другий бік свопу. */
async function createMint(
  connection: Connection,
  payer: Keypair,
  authority: PublicKey,
): Promise<PublicKey> {
  const mint = Keypair.generate();
  const lamports = await connection.getMinimumBalanceForRentExemption(MINT_SIZE);

  await send(
    connection,
    [
      SystemProgram.createAccount({
        fromPubkey: payer.publicKey,
        newAccountPubkey: mint.publicKey,
        lamports,
        space: MINT_SIZE,
        programId: TOKEN_2022,
      }),
      initializeMint2(mint.publicKey, USDC_DECIMALS, authority),
    ],
    [payer, mint],
  );

  return mint.publicKey;
}

/** Токен-акаунт на довільному власнику — там, де асоційований не годиться. */
async function createTokenAccount(
  connection: Connection,
  payer: Keypair,
  mint: PublicKey,
  owner: PublicKey,
): Promise<PublicKey> {
  const account = Keypair.generate();
  const lamports = await connection.getMinimumBalanceForRentExemption(TOKEN_ACCOUNT_SIZE);

  await send(
    connection,
    [
      SystemProgram.createAccount({
        fromPubkey: payer.publicKey,
        newAccountPubkey: account.publicKey,
        lamports,
        space: TOKEN_ACCOUNT_SIZE,
        programId: TOKEN_2022,
      }),
      initializeAccount3(account.publicKey, mint, owner),
    ],
    [payer, account],
  );

  return account.publicKey;
}

async function createAta(
  connection: Connection,
  payer: Keypair,
  owner: PublicKey,
  mint: PublicKey,
): Promise<PublicKey> {
  const ata = createAssociatedTokenAccountIdempotent(payer.publicKey, owner, mint);
  await send(connection, [ata.instruction], [payer]);
  return ata.address;
}

/**
 * Параметри протоколу. Singleton: якщо конфіг уже стоїть на цьому вузлі, він
 * береться як є — інакше повторний прогін вимагав би скидати ланцюг. Разом із
 * конфігом успадковується і його розрахункова валюта: `register_source` приймає
 * лише її (`FR-036`).
 */
export async function ensureConfig(
  connection: Connection,
  admin: Keypair,
  cluster: Cluster,
  // On a public cluster the config is created once, deliberately, by
  // `init-devnet.ts` — never as a side effect of a run with a stray key.
  create: boolean = cluster === 'localnet',
): Promise<{ config: PublicKey; usdcMint: PublicKey; feeVault: PublicKey; fresh: boolean }> {
  const config = configPda(CLUB_PROGRAM).address;
  const existing = await connection.getAccountInfo(config, 'confirmed');

  if (existing !== null) {
    const decoded = decodeProtocolConfig(existing.data);
    // The settlement mint's authority is the admin who created the config; with
    // any other key the run would fail later, at the first mint, less clearly.
    if (!decoded.admin.equals(admin.publicKey)) {
      throw new Error(
        `config admin is ${decoded.admin.toBase58()}, not ${admin.publicKey.toBase58()} — ` +
          'pass that key in ADMIN_KEYPAIR, or on localnet restart the validator with --reset',
      );
    }
    return { config, usdcMint: decoded.usdcMint, feeVault: decoded.feeVault, fresh: false };
  }
  if (!create) {
    throw new Error(`no protocol config on ${cluster} — run init:devnet first`);
  }

  const usdcMint = await createMint(connection, admin, admin.publicKey);
  const feeVault = await createAta(connection, admin, admin.publicKey, usdcMint);

  const data = Args.forInstruction('init_protocol')
    .u16(150) // origination_fee_bps — `FR-034`, у межах 1…2%
    .u16(50) // trading_fee_bps — `FR-035`
    .u16(5_000) // max_pledge_bps — `FR-005`
    .i64(30n * 86_400n)
    .i64(180n * 86_400n)
    .i64(DEMO_HISTORY_THRESHOLD_SECS) // history_threshold_secs — `FR-007`
    .build();

  await send(
    connection,
    [
      instruction(
        CLUB_PROGRAM,
        [rw(config), signerRw(admin.publicKey), ro(usdcMint), ro(feeVault), ro(SYSTEM_PROGRAM)],
        data,
      ),
    ],
    [admin],
  );

  return { config, usdcMint, feeVault, fresh: true };
}

/**
 * Підготовка: усе, що мусить стояти на вузлі **до** того, як почнеться цикл.
 *
 * `funding` — скільки розрахункової валюти видати кожному інвесторові; довжина
 * масиву і є кількість гаманців. Видати треба саме тут: гроші в кишені
 * інвестора — це передумова підписки, а не її крок, і тягнути карбування в
 * хронометраж означало б міряти щедрість адміна.
 *
 * `mintAuthority` віддається адміну першого прогону: на повторному прогоні
 * конфіг уже стоїть, і карбувати USDC може лише той, хто створив мінт. Тому на
 * «брудному» вузлі підготовка просить того самого адміна — його ключ передається
 * через оточення, і саме тому це описано в README як частина процедури.
 */
export async function prepareWorld(
  connection: Connection,
  admin: Keypair,
  funding: readonly bigint[],
  log: (line: string) => void,
): Promise<WorldStage> {
  const cluster = await detectCluster(connection);
  if (cluster === 'mainnet-beta') throw new Error('the demo world is not for mainnet');
  const issuer = Keypair.generate();
  const trader = Keypair.generate();
  const wallets = funding.map(() => Keypair.generate());

  const sol = WALLET_SOL[cluster === 'localnet' ? 'localnet' : 'remote'];
  if (cluster === 'localnet') await fund(connection, cluster, admin, admin.publicKey, 50);
  await fund(connection, cluster, admin, issuer.publicKey, sol.issuer);
  for (const wallet of [trader, ...wallets]) {
    await fund(connection, cluster, admin, wallet.publicKey, sol.other);
  }
  log('гаманці профінансовані');

  const { config, usdcMint, feeVault } = await ensureConfig(connection, admin, cluster);
  log(`конфіг протоколу: ${config.toBase58()}`);

  const baseMint = await createMint(connection, admin, admin.publicKey);

  // Рахунки демо-емітента. Джерело і резерв — різні рахунки: перший наповнює
  // комісія, з другого йде вихід свопу.
  const sourceVault = await createTokenAccount(connection, admin, usdcMint, POOL);
  const poolUsdc = await createAta(connection, admin, POOL, usdcMint);
  const poolBase = await createAta(connection, admin, POOL, baseMint);

  const issuerUsdc = await createAta(connection, admin, issuer.publicKey, usdcMint);
  const traderUsdc = await createAta(connection, admin, trader.publicKey, usdcMint);
  const traderBase = await createAta(connection, admin, trader.publicKey, baseMint);

  const investors: InvestorWallet[] = [];
  for (const wallet of wallets) {
    investors.push({
      keypair: wallet,
      usdc: await createAta(connection, admin, wallet.publicKey, usdcMint),
    });
  }

  const mints = [
    mintTo(usdcMint, traderUsdc, admin.publicKey, 4_000_000n * USDC),
    mintTo(baseMint, poolBase, admin.publicKey, 4_000_000n * USDC),
  ];
  investors.forEach((investor, index) => {
    const amount = funding[index];
    if (amount === undefined) throw new Error('гаманець без суми фінансування');
    mints.push(mintTo(usdcMint, investor.usdc, admin.publicKey, amount));
  });
  await send(connection, mints, [admin]);
  log('токени роздані');

  // ── Джерело revenue (`FR-004`, `FR-028`) ────────────────────────────────────
  const source = sourcePda(issuer.publicKey, 0n, CLUB_PROGRAM).address;
  await send(
    connection,
    [
      instruction(
        CLUB_PROGRAM,
        [
          ro(config),
          rw(source),
          signerRw(issuer.publicKey),
          ro(POOL), // authority: PDA демо-емітента — його підпис і є доказ
          ro(usdcMint),
          ro(sourceVault),
          ro(SYSTEM_PROGRAM),
        ],
        Args.forInstruction('register_source').u64(0n).build(),
      ),
    ],
    [issuer],
  );
  log(`джерело: ${source.toBase58()}`);

  const stage: WorldStage = {
    connection,
    cluster,
    admin,
    issuer,
    trader,
    investors,
    config,
    usdcMint,
    feeVault,
    baseMint,
    source,
    sourceVault,
    poolUsdc,
    poolBase,
    issuerUsdc,
    traderUsdc,
    traderBase,
  };
  await buildHistory(stage, log);

  return stage;
}

/** `Clock.unix_timestamp`: the sysvar is slot, epoch start, epoch, leader epoch, then it. */
const CLOCK_UNIX_TIMESTAMP_OFFSET = 32;

/** The chain's clock — the one `create_issue` measures the history with, not this machine's. */
async function chainTime(connection: Connection): Promise<bigint> {
  const clock = await connection.getAccountInfo(SYSVAR_CLOCK_PUBKEY, 'confirmed');
  if (clock === null) throw new Error('clock sysvar is missing');
  return clock.data.readBigInt64LE(CLOCK_UNIX_TIMESTAMP_OFFSET);
}

/**
 * Admission (`FR-007`): before its first issue a source needs revenue that
 * actually went through the intercept, and the protocol's threshold of time
 * since it was registered. A demo source is born a second ago, so this is the
 * part SPEC → Assumptions calls "the history is filled in beforehand": a few
 * swaps with no issue behind them — the intercept only observes then — and a
 * wait on the chain clock until the threshold has passed. Both stay outside
 * the `SC-006` timing, like the rest of the preparation.
 */
async function buildHistory(stage: WorldStage, log: (line: string) => void): Promise<void> {
  for (let swap = 0; swap < HISTORY_SWAPS; swap += 1) {
    await send(stage.connection, [swapInstruction(stage, null)], [stage.trader]);
  }

  const [configInfo, sourceInfo] = await Promise.all([
    stage.connection.getAccountInfo(stage.config, 'confirmed'),
    stage.connection.getAccountInfo(stage.source, 'confirmed'),
  ]);
  if (configInfo === null || sourceInfo === null) throw new Error('config or source is missing');
  const threshold = decodeProtocolConfig(configInfo.data).historyThresholdSecs;
  const source = decodeRevenueSource(sourceInfo.data);
  if (source.totalObserved === 0n) throw new Error('history swaps left no revenue on the source');
  log(`history: ${formatUsdc(source.totalObserved)} USDC observed by the source`);

  const admittedAt = source.firstSeenTs + threshold;
  log(`history: waiting for the chain clock to pass the ${threshold} s threshold`);
  for (;;) {
    const now = await chainTime(stage.connection);
    if (now >= admittedAt) break;
    await new Promise((resolve) => setTimeout(resolve, CLOCK_POLL_MS));
  }
  log('history: the source is admitted');
}

/**
 * Крок циклу: створити випуск (`FR-001`…`FR-006`, `FR-013`).
 *
 * Одна транзакція піднімає мінт із гуком, два сховища і список акаунтів гука —
 * це помітно дорожче за типові 200 000 одиниць, тому в набір іде
 * `ComputeBudgetProgram`.
 */
export async function openIssue(
  stage: WorldStage,
  log: (line: string) => void,
): Promise<IssueHandle> {
  const issue = issuePda(stage.source, 0n, CLUB_PROGRAM).address;
  const bondMint = Keypair.generate();
  const subscriptionVault = Keypair.generate();
  const escrowVault = Keypair.generate();
  const extraMetas = extraAccountMetasPda(bondMint.publicKey, CLUB_PROGRAM).address;

  const now = BigInt(Math.floor(Date.now() / 1000));
  const data = Args.forInstruction('create_issue')
    .u64(0n)
    .u64(FACE)
    .u16(COUPON_BPS)
    .u16(PLEDGE_BPS)
    .i64(now + TENOR_SECS)
    .i64(now + SUBSCRIPTION_SECS)
    .u64(MIN_LOT)
    .build();

  await send(
    stage.connection,
    [
      ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
      instruction(
        CLUB_PROGRAM,
        [
          ro(stage.config),
          rw(stage.source),
          rw(issue),
          signerRw(stage.issuer.publicKey),
          ro(stage.usdcMint),
          signerRw(bondMint.publicKey),
          signerRw(subscriptionVault.publicKey),
          signerRw(escrowVault.publicKey),
          rw(extraMetas),
          ro(TOKEN_2022),
          ro(SYSTEM_PROGRAM),
          ro(CLUB_PROGRAM), // previous issue: none, the source is fresh (`FR-006`)
        ],
        data,
      ),
    ],
    [stage.issuer, bondMint, subscriptionVault, escrowVault],
  );
  log(`випуск: ${issue.toBase58()}`);

  return {
    issue,
    bondMint: bondMint.publicKey,
    escrowVault: escrowVault.publicKey,
    subscriptionVault: subscriptionVault.publicKey,
  };
}

/**
 * Крок циклу: один гаманець заходить у випуск (`FR-008`…`FR-010`, `FR-038`).
 *
 * Дві транзакції, і обидві належать інвесторові: перша відкриває облік і заводить
 * рахунок бонду, друга вносить гроші. Розділені вони не для зручності — облік
 * мусить існувати **до** появи бонд-токенів, інакше гук не знайде чекпоінта.
 *
 * `offer` — скільки інвестор **пропонує**. Прийнято буде `min(offer, залишок)`
 * (`FR-009`), і скільки саме — читається з ланцюга, а не припускається: рахунок
 * бонду після підписки і є прийнята сума.
 */
export async function joinIssue(
  stage: WorldStage,
  issue: IssueHandle,
  investor: InvestorWallet,
  offer: bigint,
  log: (line: string) => void,
): Promise<HolderHandle> {
  const owner = investor.keypair.publicKey;
  const holder = holderPda(issue.issue, owner, CLUB_PROGRAM).address;
  const bond = createAssociatedTokenAccountIdempotent(owner, owner, issue.bondMint);

  await send(
    stage.connection,
    [
      instruction(
        CLUB_PROGRAM,
        [ro(issue.issue), rw(holder), signerRw(owner), ro(owner), ro(SYSTEM_PROGRAM)],
        Args.forInstruction('open_position').build(),
      ),
      bond.instruction,
    ],
    [investor.keypair],
  );

  await send(
    stage.connection,
    [
      instruction(
        CLUB_PROGRAM,
        [
          rw(issue.issue),
          ro(holder),
          signer(owner),
          rw(investor.usdc),
          rw(issue.subscriptionVault),
          rw(issue.bondMint),
          rw(bond.address),
          ro(stage.usdcMint),
          ro(TOKEN_2022),
        ],
        Args.forInstruction('subscribe').u64(offer).build(),
      ),
    ],
    [investor.keypair],
  );

  const accepted = await readTokenAmount(stage.connection, bond.address);
  log(
    `${owner.toBase58()}: прийнято ${formatUsdc(accepted)} USDC ` +
      `із запропонованих ${formatUsdc(offer)}`,
  );

  return { investor, holder, bond: bond.address, accepted };
}

/**
 * Крок циклу: видача (`FR-012`, `FR-034`). Звідси випуск виходить у `Repaying` —
 * і рівно звідси перехоплення починає розщеплювати потік.
 */
export async function issueProceeds(
  stage: WorldStage,
  issue: IssueHandle,
  log: (line: string) => void,
): Promise<void> {
  await send(
    stage.connection,
    [
      instruction(
        CLUB_PROGRAM,
        [
          ro(stage.config),
          rw(issue.issue),
          ro(stage.source),
          signer(stage.issuer.publicKey),
          rw(stage.issuerUsdc),
          rw(issue.subscriptionVault),
          rw(stage.feeVault),
          ro(stage.usdcMint),
          ro(TOKEN_2022),
        ],
        Args.forInstruction('withdraw_proceeds').build(),
      ),
    ],
    [stage.issuer],
  );
  log('видача пройшла — випуск у погашенні');
}

/**
 * Своп демо-емітента: утримує комісію й розщеплює її в тій самій транзакції.
 *
 * With no issue (`null`) the three issue slots carry the demo program's own id —
 * Anchor's empty optional account — and the intercept only adds to the history.
 */
export function swapInstruction(
  stage: WorldStage,
  issue: IssueHandle | null,
): TransactionInstruction {
  return instruction(
    DEMO_PROGRAM,
    [
      ro(POOL),
      signer(stage.trader.publicKey),
      rw(stage.traderUsdc),
      rw(stage.traderBase),
      rw(stage.poolUsdc),
      rw(stage.poolBase),
      rw(stage.sourceVault),
      rw(stage.source),
      issue === null ? ro(DEMO_PROGRAM) : rw(issue.issue),
      issue === null ? ro(DEMO_PROGRAM) : rw(issue.escrowVault),
      issue === null ? ro(DEMO_PROGRAM) : ro(issue.bondMint),
      ro(stage.usdcMint),
      ro(stage.baseMint),
      ro(TOKEN_2022),
      ro(CLUB_PROGRAM),
    ],
    Args.forInstruction('swap').u64(SWAP_AMOUNT_IN).build(),
  );
}

/**
 * Виплата власникові (`FR-015`, `FR-016`).
 *
 * Стан випуску тут нічого не вирішує — власник приходить у будь-який момент, а
 * сума є різницею індексів, помноженою на його баланс. Рахунок призначення
 * прибитий до підписанта самою інструкцією, тому «забрати на чужий рахунок»
 * не існує як можливість.
 */
export function claimInstruction(
  stage: WorldStage,
  issue: IssueHandle,
  holder: HolderHandle,
): TransactionInstruction {
  return instruction(
    CLUB_PROGRAM,
    [
      ro(issue.issue),
      rw(holder.holder),
      signer(holder.investor.keypair.publicKey),
      rw(holder.investor.usdc),
      rw(issue.escrowVault),
      ro(holder.bond),
      ro(issue.bondMint),
      ro(stage.usdcMint),
      ro(TOKEN_2022),
    ],
    Args.forInstruction('claim').build(),
  );
}

/**
 * Світ одним викликом: один інвестор бере повний номінал, випуск виходить у
 * погашення. Це те, що потрібно заміру `SC-002`, — там цикл не міряється, там
 * міряється затримка після нього.
 */
export async function standUpWorld(
  connection: Connection,
  admin: Keypair,
  log: (line: string) => void,
): Promise<DemoWorld> {
  const stage = await prepareWorld(connection, admin, [FACE], log);
  const investor = stage.investors[0];
  if (investor === undefined) throw new Error('світ без інвестора');

  const issue = await openIssue(stage, log);
  await joinIssue(stage, issue, investor, FACE, log);
  log('номінал зібрано повністю');
  await issueProceeds(stage, issue, log);

  return {
    issue: issue.issue,
    source: stage.source,
    bondMint: issue.bondMint,
    escrowVault: issue.escrowVault,
    trader: stage.trader,
    swapInstruction: () => swapInstruction(stage, issue),
  };
}

/** Адреса асоційованого рахунка — реекспорт, щоб замір не тягнув увесь модуль. */
export { associatedTokenAddress };
