/**
 * Демо-світ M1 на живому вузлі: від параметрів протоколу до випуску, який
 * погашається.
 *
 * Це не демо-скрипт (`T031`) і не його заготовка — це те, без чого `SC-002`
 * неможливо заміряти взагалі. Критерій рахує час **від підтвердження
 * транзакції з комісією**, а комісія в цьому протоколі буває рівно одна:
 * та, яку `demo_issuer.swap` утримує й тут же розщеплює через CPI (`FR-004`).
 * Щоб такий своп відбувся, на вузлі мусить стояти весь ланцюжок: конфіг,
 * джерело, випуск, підписка на повний номінал і видача — бо перехоплення
 * розщеплює лише в стані `Repaying` (`FR-012`).
 *
 * Усе тут — справжні інструкції обох програм. Підроблених акаунтів немає: стан,
 * підкладений у сховище, довів би лише те, що підробка узгоджена сама з собою.
 */

import {
  type Connection,
  ComputeBudgetProgram,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  sendAndConfirmTransaction,
  SystemProgram,
  Transaction,
  type TransactionInstruction,
} from '@solana/web3.js';
import { decodeProtocolConfig } from '../../../packages/sdk/src/accounts.ts';
import { configPda, holderPda, issuePda, sourcePda } from '../../../packages/sdk/src/pda.ts';
import { extraAccountMetasPda } from '../../../packages/sdk/src/pda.ts';
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
  signer,
  signerRw,
  SYSTEM_PROGRAM,
  TOKEN_2022,
  TOKEN_ACCOUNT_SIZE,
} from './encode.ts';

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

/** PDA пулу демо-емітента — той ключ, чий **підпис** автентифікує перехоплення. */
export const POOL = PublicKey.findProgramAddressSync([Buffer.from('pool')], DEMO_PROGRAM)[0];

export interface DemoWorld {
  readonly issue: PublicKey;
  readonly source: PublicKey;
  readonly bondMint: PublicKey;
  readonly escrowVault: PublicKey;
  readonly trader: Keypair;
  /** Своп, який утримує комісію й розщеплює її в тій самій транзакції. */
  swapInstruction(): TransactionInstruction;
}

async function fund(connection: Connection, who: PublicKey, sol: number): Promise<void> {
  const signature = await connection.requestAirdrop(who, sol * LAMPORTS_PER_SOL);
  const latest = await connection.getLatestBlockhash();
  await connection.confirmTransaction({ signature, ...latest }, 'confirmed');
}

async function send(
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
 * береться як є — інакше повторний прогін заміру вимагав би скидати ланцюг.
 * Разом із конфігом успадковується і його розрахункова валюта: `register_source`
 * приймає лише її (`FR-036`).
 */
async function ensureConfig(
  connection: Connection,
  admin: Keypair,
): Promise<{ config: PublicKey; usdcMint: PublicKey; feeVault: PublicKey; fresh: boolean }> {
  const config = configPda(CLUB_PROGRAM).address;
  const existing = await connection.getAccountInfo(config, 'confirmed');

  if (existing !== null) {
    const decoded = decodeProtocolConfig(existing.data);
    return { config, usdcMint: decoded.usdcMint, feeVault: decoded.feeVault, fresh: false };
  }

  const usdcMint = await createMint(connection, admin, admin.publicKey);
  const feeVault = await createAta(connection, admin, admin.publicKey, usdcMint);

  const data = Args.forInstruction('init_protocol')
    .u16(150) // origination_fee_bps — `FR-034`, у межах 1…2%
    .u16(50) // trading_fee_bps — `FR-035`
    .u16(5_000) // max_pledge_bps — `FR-005`
    .i64(30n * 86_400n)
    .i64(180n * 86_400n)
    .i64(86_400n) // history_threshold_secs — `FR-007`
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
 * Піднімає світ і доводить випуск до `Repaying`.
 *
 * `mintAuthority` віддається адміну першого прогону: на повторному прогоні
 * конфіг уже стоїть, і карбувати USDC може лише той, хто створив мінт. Тому
 * замір на «брудному» вузлі просить свіжого адміна — і саме тому скидання
 * ланцюга описане у звіті як частина процедури.
 */
export async function standUpWorld(
  connection: Connection,
  admin: Keypair,
  log: (line: string) => void,
): Promise<DemoWorld> {
  const issuer = Keypair.generate();
  const investor = Keypair.generate();
  const trader = Keypair.generate();

  await fund(connection, admin.publicKey, 50);
  for (const wallet of [issuer, investor, trader]) {
    await fund(connection, wallet.publicKey, 10);
  }
  log('гаманці профінансовані');

  const { config, usdcMint, feeVault } = await ensureConfig(connection, admin);
  log(`конфіг протоколу: ${config.toBase58()}`);

  const baseMint = await createMint(connection, admin, admin.publicKey);

  // Рахунки демо-емітента. Джерело і резерв — різні рахунки: перший наповнює
  // комісія, з другого йде вихід свопу.
  const sourceVault = await createTokenAccount(connection, admin, usdcMint, POOL);
  const poolUsdc = await createAta(connection, admin, POOL, usdcMint);
  const poolBase = await createAta(connection, admin, POOL, baseMint);

  const issuerUsdc = await createAta(connection, admin, issuer.publicKey, usdcMint);
  const investorUsdc = await createAta(connection, admin, investor.publicKey, usdcMint);
  const traderUsdc = await createAta(connection, admin, trader.publicKey, usdcMint);
  const traderBase = await createAta(connection, admin, trader.publicKey, baseMint);

  await send(
    connection,
    [
      mintTo(usdcMint, investorUsdc, admin.publicKey, FACE),
      mintTo(usdcMint, traderUsdc, admin.publicKey, 4_000_000n * USDC),
      mintTo(baseMint, poolBase, admin.publicKey, 4_000_000n * USDC),
    ],
    [admin],
  );
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

  // ── Випуск (`FR-001`…`FR-006`, `FR-013`) ────────────────────────────────────
  const issue = issuePda(source, 0n, CLUB_PROGRAM).address;
  const bondMint = Keypair.generate();
  const subscriptionVault = Keypair.generate();
  const escrowVault = Keypair.generate();
  const extraMetas = extraAccountMetasPda(bondMint.publicKey, CLUB_PROGRAM).address;

  const now = BigInt(Math.floor(Date.now() / 1000));
  const issueData = Args.forInstruction('create_issue')
    .u64(0n)
    .u64(FACE)
    .u16(COUPON_BPS)
    .u16(PLEDGE_BPS)
    .i64(now + TENOR_SECS)
    .i64(now + SUBSCRIPTION_SECS)
    .u64(MIN_LOT)
    .build();

  await send(
    connection,
    [
      // Створення випуску підіймає мінт із гуком, два сховища і список акаунтів
      // гука — це помітно дорожче за типові 200 000 одиниць.
      ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }),
      instruction(
        CLUB_PROGRAM,
        [
          ro(config),
          rw(source),
          rw(issue),
          signerRw(issuer.publicKey),
          ro(usdcMint),
          signerRw(bondMint.publicKey),
          signerRw(subscriptionVault.publicKey),
          signerRw(escrowVault.publicKey),
          rw(extraMetas),
          ro(TOKEN_2022),
          ro(SYSTEM_PROGRAM),
        ],
        issueData,
      ),
    ],
    [issuer, bondMint, subscriptionVault, escrowVault],
  );
  log(`випуск: ${issue.toBase58()}`);

  // ── Позиція інвестора і підписка на повний номінал (`FR-008`…`FR-010`) ───────
  const holder = holderPda(issue, investor.publicKey, CLUB_PROGRAM).address;
  const investorBond = createAssociatedTokenAccountIdempotent(
    investor.publicKey,
    investor.publicKey,
    bondMint.publicKey,
  );

  await send(
    connection,
    [
      instruction(
        CLUB_PROGRAM,
        [
          ro(issue),
          rw(holder),
          signerRw(investor.publicKey),
          ro(investor.publicKey),
          ro(SYSTEM_PROGRAM),
        ],
        Args.forInstruction('open_position').build(),
      ),
      investorBond.instruction,
    ],
    [investor],
  );

  await send(
    connection,
    [
      instruction(
        CLUB_PROGRAM,
        [
          rw(issue),
          ro(holder),
          signer(investor.publicKey),
          rw(investorUsdc),
          rw(subscriptionVault.publicKey),
          rw(bondMint.publicKey),
          rw(investorBond.address),
          ro(usdcMint),
          ro(TOKEN_2022),
        ],
        Args.forInstruction('subscribe').u64(FACE).build(),
      ),
    ],
    [investor],
  );
  log('номінал зібрано повністю');

  // ── Видача: звідси випуск виходить у `Repaying` (`FR-012`, `FR-034`) ─────────
  await send(
    connection,
    [
      instruction(
        CLUB_PROGRAM,
        [
          ro(config),
          rw(issue),
          ro(source),
          signer(issuer.publicKey),
          rw(issuerUsdc),
          rw(subscriptionVault.publicKey),
          rw(feeVault),
          ro(usdcMint),
          ro(TOKEN_2022),
        ],
        Args.forInstruction('withdraw_proceeds').build(),
      ),
    ],
    [issuer],
  );
  log('видача пройшла — випуск у погашенні');

  const swapData = Args.forInstruction('swap').u64(SWAP_AMOUNT_IN).build();

  return {
    issue,
    source,
    bondMint: bondMint.publicKey,
    escrowVault: escrowVault.publicKey,
    trader,
    swapInstruction: () =>
      instruction(
        DEMO_PROGRAM,
        [
          ro(POOL),
          signer(trader.publicKey),
          rw(traderUsdc),
          rw(traderBase),
          rw(poolUsdc),
          rw(poolBase),
          rw(sourceVault),
          rw(source),
          rw(issue),
          rw(escrowVault.publicKey),
          ro(bondMint.publicKey),
          ro(usdcMint),
          ro(baseMint),
          ro(TOKEN_2022),
          ro(CLUB_PROGRAM),
        ],
        swapData,
      ),
  };
}

/** Адреса асоційованого рахунка — реекспорт, щоб замір не тягнув увесь модуль. */
export { associatedTokenAddress };
