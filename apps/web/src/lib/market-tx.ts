/**
 * Що саме йде в транзакції вторинки і як пояснити відмову.
 *
 * Збирачі інструкцій — у SDK; тут лише склад транзакції. Кожна транзакція
 * спершу ідемпотентно створює рахунки, без яких програма відмовить
 * (рішення 2026-09-26, T038): USDC-рахунок продавця — ATA, бо в `Offer` його
 * немає, і покупцеві його мусить назвати адреса, а не домовленість.
 * Виставлення створює його за рахунок продавця; купівля — ще раз, про всяк
 * випадок, якщо продавець його закрив. Ідемпотентна інструкція на наявному
 * рахунку нічого не коштує.
 */

import {
  associatedTokenAddress,
  buyOfferInstruction,
  cancelOfferInstruction,
  createAssociatedTokenAccountIdempotent,
  createOfferInstruction,
  type Issue,
  type Offer,
  type ProtocolConfig,
} from '@daddys-club/sdk';
import type { PublicKey, TransactionInstruction } from '@solana/web3.js';

export interface ListingParams {
  readonly issue: Issue;
  readonly config: ProtocolConfig;
  readonly seller: PublicKey;
  readonly nonce: bigint;
  readonly amount: bigint;
  readonly price: bigint;
}

/** Виставлення: USDC-рахунок продавця — наперед, щоб покупцеві було куди платити. */
export function listingInstructions(params: ListingParams): TransactionInstruction[] {
  const { issue, config, seller } = params;
  return [
    createAssociatedTokenAccountIdempotent(seller, seller, config.usdcMint).instruction,
    createOfferInstruction({
      issue,
      seller,
      sellerBond: associatedTokenAddress(seller, issue.bondMint),
      nonce: params.nonce,
      amount: params.amount,
      price: params.price,
    }),
  ];
}

export interface PurchaseParams {
  readonly issue: Issue;
  readonly config: ProtocolConfig;
  readonly offer: Offer;
  readonly buyer: PublicKey;
}

/**
 * Купівля: рахунок бонду покупця і USDC-рахунок продавця — ідемпотентно.
 * Облік покупця (`FR-038`) відкриває сама `buy_offer`.
 */
export function purchaseInstructions(params: PurchaseParams): TransactionInstruction[] {
  const { issue, config, offer, buyer } = params;
  const buyerBond = createAssociatedTokenAccountIdempotent(buyer, buyer, issue.bondMint);
  const sellerUsdc = createAssociatedTokenAccountIdempotent(buyer, offer.seller, config.usdcMint);
  return [
    buyerBond.instruction,
    sellerUsdc.instruction,
    buyOfferInstruction({
      config,
      issue,
      offer,
      buyer,
      buyerUsdc: associatedTokenAddress(buyer, config.usdcMint),
      buyerBond: buyerBond.address,
      sellerUsdc: sellerUsdc.address,
    }),
  ];
}

export interface CancellationParams {
  readonly issue: Issue;
  readonly config: ProtocolConfig;
  readonly offer: Offer;
}

/** Скасування: обидва рахунки продавця — ідемпотентно, бо лот і накопичене їдуть туди. */
export function cancellationInstructions(params: CancellationParams): TransactionInstruction[] {
  const { issue, config, offer } = params;
  const bond = createAssociatedTokenAccountIdempotent(offer.seller, offer.seller, issue.bondMint);
  const usdc = createAssociatedTokenAccountIdempotent(offer.seller, offer.seller, config.usdcMint);
  return [
    bond.instruction,
    usdc.instruction,
    cancelOfferInstruction({
      issue,
      offer,
      usdcMint: config.usdcMint,
      sellerBond: bond.address,
      sellerUsdc: usdc.address,
    }),
  ];
}

function logsOf(error: unknown): readonly string[] {
  if (typeof error !== 'object' || error === null) return [];
  const logs =
    (error as { logs?: unknown }).logs ?? (error as { transactionLogs?: unknown }).transactionLogs;
  return Array.isArray(logs) ? logs.filter((line): line is string => typeof line === 'string') : [];
}

/**
 * Відмова людською мовою. Anchor пише в лог і код, і повідомлення
 * (`AnchorError … Error Message: …`) — його й показуємо: так пояснення
 * однакові для ринку, ядра й гука без переписаних руками таблиць кодів.
 */
export function explainFailure(error: unknown): string {
  const logs = logsOf(error);

  for (const line of logs) {
    const anchor = /Error Message: (.+?)\.?$/.exec(line);
    if (anchor?.[1] !== undefined) return `${anchor[1]}.`;
  }
  if (logs.some((line) => /insufficient funds/i.test(line))) {
    return 'Not enough tokens on the paying account.';
  }
  const failed = logs.find((line) => / failed: /.test(line));
  if (failed !== undefined) return failed;

  const message = error instanceof Error ? error.message : String(error);
  if (/reject|denied|declined|cancel/i.test(message)) return 'The wallet declined to sign.';
  return message;
}
