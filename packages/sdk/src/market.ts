/**
 * Збірка інструкцій вторинного ринку — `create_offer`, `buy_offer`,
 * `cancel_offer` програми `daddys_market` (`FR-024`…`FR-027`).
 *
 * Порядок акаунтів — оголошення `CreateOffer`, `BuyOffer` і `CancelOffer` у
 * programs/daddys-market/src/instructions/market.rs. Переписаний він руками,
 * тому `market.test.ts` читає ці структури й звіряє з ними і імена, і порядок,
 * і прапорці `mut`/`Signer`. Переставлене поле в програмі падає там, а не
 * відмовою `ConstraintSeeds` на першій справжній транзакції.
 *
 * Усе, що програма прибиває через `has_one`, береться з декодованих акаунтів, а
 * не з рук викликача: мінт бонду й сховище погашення — з `Issue`, скарбниця й
 * валюта — з `ProtocolConfig`, продавець і сховище — з `Offer`. Адреса, яку
 * можна вивести, виводиться. Викликач приносить лише те, чого з ланцюга не
 * дістати: свої токен-рахунки.
 */

import { PublicKey, SystemProgram, TransactionInstruction } from '@solana/web3.js';
import { z } from 'zod';
import type { Issue, Offer, ProtocolConfig } from './accounts.js';
import {
  configPda,
  extraAccountMetasPda,
  holderPda,
  issuePda,
  MARKET_PROGRAM_ID,
  offerEscrowPda,
  offerPda,
  offerProceedsPda,
  PROGRAM_ID,
} from './pda.js';

/** Token-2022. Бонд і USDC обидва живуть у ньому: `token_program` у наборах один. */
export const TOKEN_2022_PROGRAM_ID = new PublicKey('TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb');

export type MarketInstruction = 'create_offer' | 'buy_offer' | 'cancel_offer';

/**
 * Дискримінатор інструкції Anchor — 8 байт `sha256("global:" + ім'я)`.
 * Прибиті числами з тієї ж причини, що й дискримінатори акаунтів у
 * `accounts.ts`: sha256 у браузері лише асинхронний. Тест рахує і звіряє.
 */
export const INSTRUCTION_DISCRIMINATORS: Readonly<Record<MarketInstruction, Uint8Array>> = {
  create_offer: new Uint8Array([237, 233, 192, 168, 248, 7, 249, 241]),
  buy_offer: new Uint8Array([23, 179, 13, 119, 90, 98, 100, 245]),
  cancel_offer: new Uint8Array([92, 203, 223, 40, 92, 89, 53, 119]),
};

/** Місце акаунта в наборі: ім'я поля в Rust і те, як його подає клієнт. */
export interface AccountSlot {
  readonly name: string;
  readonly writable: boolean;
  readonly signer: boolean;
}

const slot = (name: string, writable: boolean, signer = false): AccountSlot => ({
  name,
  writable,
  signer,
});

/** `CreateOffer`: продавець підписує й платить оренду оферти та сховища. */
export const CREATE_OFFER_ACCOUNTS: readonly AccountSlot[] = [
  slot('issue', false),
  slot('seller', true, true),
  slot('seller_bond', true),
  slot('bond_mint', false),
  slot('offer', true),
  slot('token_escrow', true),
  slot('holder_seller', true),
  slot('holder_escrow', true),
  slot('extra_account_meta_list', false),
  slot('club_program', false),
  slot('token_program', false),
  slot('system_program', false),
];

/** `BuyOffer`: продавець у наборі є, але не підписує — оферта вже його підпис. */
export const BUY_OFFER_ACCOUNTS: readonly AccountSlot[] = [
  slot('config', false),
  slot('issue', false),
  slot('offer', true),
  slot('token_escrow', true),
  slot('offer_proceeds', true),
  slot('seller', true),
  slot('seller_usdc', true),
  slot('buyer', true, true),
  slot('buyer_usdc', true),
  slot('buyer_bond', true),
  slot('holder_buyer', true),
  slot('holder_escrow', true),
  slot('escrow_vault', true),
  slot('fee_vault', true),
  slot('bond_mint', false),
  slot('usdc_mint', false),
  slot('extra_account_meta_list', false),
  slot('club_program', false),
  slot('token_program', false),
  slot('system_program', false),
];

/** `CancelOffer`: скарбниці комісій тут немає — передумати не угода (`FR-027`). */
export const CANCEL_OFFER_ACCOUNTS: readonly AccountSlot[] = [
  slot('issue', false),
  slot('offer', true),
  slot('token_escrow', true),
  slot('offer_proceeds', true),
  slot('seller', true, true),
  slot('seller_bond', true),
  slot('seller_usdc', true),
  slot('holder_seller', true),
  slot('holder_escrow', true),
  slot('escrow_vault', true),
  slot('bond_mint', false),
  slot('usdc_mint', false),
  slot('extra_account_meta_list', false),
  slot('club_program', false),
  slot('token_program', false),
  slot('system_program', false),
];

const U64_MAX = (1n << 64n) - 1n;

/** Аргумент `u64` — little-endian, як borsh. Вихід за межі — помилка виклику. */
function u64Arg(value: bigint, what: string): Uint8Array {
  if (value < 0n || value > U64_MAX) {
    throw new RangeError(`${what} не вкладається в u64: ${value}`);
  }
  const bytes = new Uint8Array(8);
  new DataView(bytes.buffer).setBigUint64(0, value, true);
  return bytes;
}

function instruction(
  name: MarketInstruction,
  layout: readonly AccountSlot[],
  accounts: Readonly<Record<string, PublicKey>>,
  args: readonly Uint8Array[] = [],
): TransactionInstruction {
  const keys = layout.map(({ name: field, writable, signer }) => {
    const pubkey = accounts[field];
    if (pubkey === undefined) throw new Error(`${name}: акаунт ${field} не заданий`);
    return { pubkey, isSigner: signer, isWritable: writable };
  });

  const parts = [INSTRUCTION_DISCRIMINATORS[name], ...args];
  const data = new Uint8Array(parts.reduce((len, part) => len + part.length, 0));
  let at = 0;
  for (const part of parts) {
    data.set(part, at);
    at += part.length;
  }

  return new TransactionInstruction({
    programId: MARKET_PROGRAM_ID,
    keys,
    data: Buffer.from(data),
  });
}

/** Адреса випуску з самого акаунта: `Issue` несе і `source`, і `seq`. */
function issueAddress(issue: Issue): PublicKey {
  return issuePda(issue.source, issue.seq).address;
}

/**
 * Оферта — з її власних полів. Акаунт із чужою адресою сюди не пройде: те, що
 * він назве, не збіжиться з тим, що перевірить `seeds` програми.
 */
function offerAddress(offer: Offer): PublicKey {
  return offerPda(offer.issue, offer.seller, offer.nonce).address;
}

/** Випуск, переданий разом з офертою, мусить бути її випуском. */
function issueOf(offer: Offer, issue: Issue): PublicKey {
  const address = issueAddress(issue);
  if (!address.equals(offer.issue)) {
    throw new Error(
      `оферта належить випуску ${offer.issue.toBase58()}, а передано ${address.toBase58()}`,
    );
  }
  return address;
}

/** Акаунти гука й програм, спільні для всіх трьох наборів. */
function hookAndPrograms(issue: Issue): Record<string, PublicKey> {
  return {
    bond_mint: issue.bondMint,
    extra_account_meta_list: extraAccountMetasPda(issue.bondMint).address,
    club_program: PROGRAM_ID,
    token_program: TOKEN_2022_PROGRAM_ID,
    system_program: SystemProgram.programId,
  };
}

export interface CreateOfferParams {
  readonly issue: Issue;
  readonly seller: PublicKey;
  /** Рахунок бонду, з якого їде лот. Authority — сам продавець, не делегат. */
  readonly sellerBond: PublicKey;
  /** Слот оферти. Брати з `findFreeOfferNonce`, а не лічильником. */
  readonly nonce: bigint;
  /** Одиниці номіналу в лоті. */
  readonly amount: bigint;
  /** USDC за весь лот. */
  readonly price: bigint;
}

/** Виставляє лот на продаж (`FR-024`, `FR-025`). */
export function createOfferInstruction(params: CreateOfferParams): TransactionInstruction {
  const issue = issueAddress(params.issue);
  const offer = offerPda(issue, params.seller, params.nonce).address;

  return instruction(
    'create_offer',
    CREATE_OFFER_ACCOUNTS,
    {
      issue,
      seller: params.seller,
      seller_bond: params.sellerBond,
      offer,
      token_escrow: offerEscrowPda(offer).address,
      holder_seller: holderPda(issue, params.seller).address,
      holder_escrow: holderPda(issue, offer).address,
      ...hookAndPrograms(params.issue),
    },
    [u64Arg(params.nonce, 'nonce'), u64Arg(params.amount, 'amount'), u64Arg(params.price, 'price')],
  );
}

export interface BuyOfferParams {
  readonly config: ProtocolConfig;
  readonly issue: Issue;
  readonly offer: Offer;
  readonly buyer: PublicKey;
  /** Звідки платиться `offer.price`. */
  readonly buyerUsdc: PublicKey;
  /** Куди приходить лот. Рахунок мусить уже існувати. */
  readonly buyerBond: PublicKey;
  /**
   * USDC-рахунок продавця. В оферті його немає, тому покупцю його називає
   * викликач; програма перевіряє лише мінт і authority.
   */
  readonly sellerUsdc: PublicKey;
}

/** Викуповує оферту цілком (`FR-026`, `FR-035`, `FR-038`). */
export function buyOfferInstruction(params: BuyOfferParams): TransactionInstruction {
  const { config, offer } = params;
  const issue = issueOf(offer, params.issue);
  const address = offerAddress(offer);

  return instruction('buy_offer', BUY_OFFER_ACCOUNTS, {
    config: configPda().address,
    issue,
    offer: address,
    token_escrow: offer.tokenEscrow,
    offer_proceeds: offerProceedsPda(address).address,
    seller: offer.seller,
    seller_usdc: params.sellerUsdc,
    buyer: params.buyer,
    buyer_usdc: params.buyerUsdc,
    buyer_bond: params.buyerBond,
    holder_buyer: holderPda(issue, params.buyer).address,
    holder_escrow: holderPda(issue, address).address,
    escrow_vault: params.issue.escrowVault,
    fee_vault: config.feeVault,
    usdc_mint: config.usdcMint,
    ...hookAndPrograms(params.issue),
  });
}

export interface CancelOfferParams {
  readonly issue: Issue;
  readonly offer: Offer;
  /** Валюта протоколу — `ProtocolConfig.usdcMint`. */
  readonly usdcMint: PublicKey;
  /** Куди повертається лот. */
  readonly sellerBond: PublicKey;
  /** Куди йде те, що набігло, поки оферта стояла. */
  readonly sellerUsdc: PublicKey;
}

/** Скасовує оферту: лот назад цілим і без комісії (`FR-027`). */
export function cancelOfferInstruction(params: CancelOfferParams): TransactionInstruction {
  const { offer } = params;
  const issue = issueOf(offer, params.issue);
  const address = offerAddress(offer);

  return instruction('cancel_offer', CANCEL_OFFER_ACCOUNTS, {
    issue,
    offer: address,
    token_escrow: offer.tokenEscrow,
    offer_proceeds: offerProceedsPda(address).address,
    seller: offer.seller,
    seller_bond: params.sellerBond,
    seller_usdc: params.sellerUsdc,
    holder_seller: holderPda(issue, offer.seller).address,
    holder_escrow: holderPda(issue, address).address,
    escrow_vault: params.issue.escrowVault,
    usdc_mint: params.usdcMint,
    ...hookAndPrograms(params.issue),
  });
}

/**
 * Досить від `Connection`, щоб знайти вільний слот. Відповідь — `unknown`:
 * вона з мережі й проходить Zod.
 */
export interface AccountsReader {
  getMultipleAccountsInfo(keys: PublicKey[]): Promise<unknown>;
}

/** Скільки слотів питати за одну ходку. У продавця рідко більше кількох оферт. */
const NONCE_PAGE = 8;

const accountInfo = z
  .object({
    owner: z.instanceof(PublicKey),
    data: z.instanceof(Uint8Array),
  })
  .nullable();

/**
 * Слот вільний, якщо `init` на ньому пройде: акаунта немає або він порожній і
 * належить системній програмі (на адресу хтось закинув лампорти — Anchor такий
 * акаунт дофінансовує й забирає).
 */
function isFree(info: z.infer<typeof accountInfo>): boolean {
  return info === null || (info.owner.equals(SystemProgram.programId) && info.data.length === 0);
}

/**
 * Найменший вільний `nonce` продавця за випуском.
 *
 * `nonce` — номер слота, а не лічильник (`docs/PLAN.md` → «Модель даних» →
 * `Offer`). Облік сховища переживає свою оферту і прив'язаний до її адреси,
 * тому той самий `nonce` веде в той самий облік: повторне виставлення в слот
 * дешевше на його оренду. Випадковий або зростаючий `nonce` щоразу відкривав
 * би новий облік і лишав старий сиротою.
 */
export async function findFreeOfferNonce(
  reader: AccountsReader,
  issue: PublicKey,
  seller: PublicKey,
): Promise<bigint> {
  for (let first = 0n; first <= U64_MAX; first += BigInt(NONCE_PAGE)) {
    const nonces = Array.from({ length: NONCE_PAGE }, (_, i) => first + BigInt(i)).filter(
      (nonce) => nonce <= U64_MAX,
    );
    const keys = nonces.map((nonce) => offerPda(issue, seller, nonce).address);
    const infos = z
      .array(accountInfo)
      .length(keys.length)
      .parse(await reader.getMultipleAccountsInfo(keys));

    const free = infos.findIndex(isFree);
    const nonce = nonces[free];
    if (nonce !== undefined) return nonce;
  }
  throw new Error('у продавця зайняті всі слоти оферт');
}
