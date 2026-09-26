import {
  ATA_PROGRAM_ID,
  associatedTokenAddress,
  type Issue,
  issuePda,
  MARKET_PROGRAM_ID,
  type Offer,
  type ProtocolConfig,
} from '@daddys-club/sdk';
import { PublicKey, type TransactionInstruction } from '@solana/web3.js';
import { describe, expect, it } from 'vitest';
import {
  cancellationInstructions,
  explainFailure,
  listingInstructions,
  purchaseInstructions,
} from './market-tx';

const key = (seed: number): PublicKey => new PublicKey(new Uint8Array(32).fill(seed));
const SELLER = key(2);
const BUYER = key(3);
const BOND_MINT = key(4);
const USDC_MINT = key(6);

const issue: Issue = {
  source: key(1),
  bondMint: BOND_MINT,
  escrowVault: key(5),
  subscriptionVault: key(12),
  face: 1n,
  couponBps: 0,
  pledgeBps: 0,
  maturityTs: 0n,
  subscriptionEndTs: 0n,
  minLot: 1n,
  raised: 1n,
  obligationTotal: 1n,
  repaidTotal: 0n,
  payoutIndex: 0n,
  state: 'Repaying',
  seq: 0n,
  bump: 255,
};
const config: ProtocolConfig = {
  admin: key(13),
  originationFeeBps: 0,
  tradingFeeBps: 50,
  maxPledgeBps: 0,
  minTenorSecs: 0n,
  maxTenorSecs: 0n,
  historyThresholdSecs: 0n,
  usdcMint: USDC_MINT,
  feeVault: key(7),
  bump: 254,
};
const offer: Offer = {
  seller: SELLER,
  issue: issuePda(key(1), 0n).address,
  amount: 5n,
  price: 4n,
  tokenEscrow: key(9),
  nonce: 0n,
  bump: 253,
};

/** Що саме створює ATA-інструкція: (платник, власник, мінт). */
function ataCreation(ix: TransactionInstruction): [string, string, string] {
  expect(ix.programId.equals(ATA_PROGRAM_ID)).toBe(true);
  const [payer, , owner, mint] = ix.keys.map((k) => k.pubkey.toBase58());
  return [payer ?? '', owner ?? '', mint ?? ''];
}

const b58 = (k: PublicKey) => k.toBase58();

describe('склад транзакцій вторинки', () => {
  it('виставлення: USDC-рахунок продавця за його ж кошт, потім create_offer', () => {
    const [ata, create] = listingInstructions({
      issue,
      config,
      seller: SELLER,
      nonce: 0n,
      amount: 5n,
      price: 4n,
    });
    expect(ataCreation(ata as TransactionInstruction)).toEqual([
      b58(SELLER),
      b58(SELLER),
      b58(USDC_MINT),
    ]);
    expect(create?.programId.equals(MARKET_PROGRAM_ID)).toBe(true);
    expect(create?.keys[2]?.pubkey.equals(associatedTokenAddress(SELLER, BOND_MINT))).toBe(true);
  });

  it('купівля: бонд-рахунок покупця і USDC-рахунок продавця — обидва платить покупець', () => {
    const [bond, usdc, buy] = purchaseInstructions({ issue, config, offer, buyer: BUYER });
    expect(ataCreation(bond as TransactionInstruction)).toEqual([
      b58(BUYER),
      b58(BUYER),
      b58(BOND_MINT),
    ]);
    expect(ataCreation(usdc as TransactionInstruction)).toEqual([
      b58(BUYER),
      b58(SELLER),
      b58(USDC_MINT),
    ]);
    const slots = buy?.keys.map((k) => b58(k.pubkey)) ?? [];
    // seller_usdc — сьомий слот BuyOffer, buyer_usdc — дев'ятий.
    expect(slots[6]).toBe(b58(associatedTokenAddress(SELLER, USDC_MINT)));
    expect(slots[8]).toBe(b58(associatedTokenAddress(BUYER, USDC_MINT)));
  });

  it('скасування: обидва рахунки продавця за його кошт', () => {
    const [bond, usdc, cancel] = cancellationInstructions({ issue, config, offer });
    expect(ataCreation(bond as TransactionInstruction)).toEqual([
      b58(SELLER),
      b58(SELLER),
      b58(BOND_MINT),
    ]);
    expect(ataCreation(usdc as TransactionInstruction)).toEqual([
      b58(SELLER),
      b58(SELLER),
      b58(USDC_MINT),
    ]);
    expect(cancel?.programId.equals(MARKET_PROGRAM_ID)).toBe(true);
  });
});

describe('explainFailure', () => {
  it('повідомлення Anchor із логу — для будь-якої з програм', () => {
    const error = Object.assign(new Error('Simulation failed'), {
      logs: [
        'Program G29g… invoke [1]',
        'Program log: AnchorError occurred. Error Code: InsufficientBondBalance. Error Number: 6001. Error Message: Insufficient bond balance.',
        'Program G29g… failed: custom program error: 0x1771',
      ],
    });
    expect(explainFailure(error)).toBe('Insufficient bond balance.');
  });

  it('Token-2022 без грошей', () => {
    const error = {
      logs: [
        'Program log: Error: insufficient funds',
        'Program Tokenz… failed: custom program error: 0x1',
      ],
    };
    expect(explainFailure(error)).toBe('Not enough tokens on the paying account.');
  });

  it('гаманець відмовив', () => {
    expect(explainFailure(new Error('User rejected the request.'))).toBe(
      'The wallet declined to sign.',
    );
  });

  it('невідоме — як є, а не «щось пішло не так»', () => {
    expect(explainFailure(new Error('blockhash not found'))).toBe('blockhash not found');
  });
});
