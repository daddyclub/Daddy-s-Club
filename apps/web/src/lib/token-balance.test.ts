import { TOKEN_2022_PROGRAM_ID } from '@daddys-club/sdk';
import { PublicKey } from '@solana/web3.js';
import { describe, expect, it } from 'vitest';
import type { AccountFeed, AccountSnapshot } from './rpc';
import { type Balance, interpretBalance, spendable, watchTokenBalance } from './token-balance';

const ADDRESS = new PublicKey(new Uint8Array(32).fill(5));

function tokenData(amount: bigint): Uint8Array {
  const data = new Uint8Array(170);
  new DataView(data.buffer).setBigUint64(64, amount, true);
  return data;
}

const token = (amount: bigint, slot: number): AccountSnapshot => ({
  slot,
  account: { owner: TOKEN_2022_PROGRAM_ID, data: tokenData(amount) },
});

describe('interpretBalance', () => {
  it('рахунок Token-2022 — сума з поля amount', () => {
    expect(interpretBalance(token(4_875_500_000n, 7))).toEqual({
      status: 'live',
      amount: 4_875_500_000n,
      slot: 7,
    });
  });

  it('немає рахунку — «missing», чужий власник — «foreign»', () => {
    expect(interpretBalance({ slot: 3, account: null })).toEqual({ status: 'missing', slot: 3 });
    const foreign = { slot: 3, account: { owner: ADDRESS, data: tokenData(1n) } };
    expect(interpretBalance(foreign)).toEqual({ status: 'foreign', slot: 3 });
  });

  it('до витрат: відсутній рахунок — нуль, невідомий — null', () => {
    expect(spendable({ status: 'missing', slot: 1 })).toBe(0n);
    expect(spendable({ status: 'loading' })).toBeNull();
    expect(spendable({ status: 'live', amount: 9n, slot: 1 })).toBe(9n);
  });
});

describe('watchTokenBalance', () => {
  it('пуш зі свіжішого слота не затирається запізнілим читанням', async () => {
    let push: (s: AccountSnapshot) => void = () => {};
    let answer: (s: AccountSnapshot) => void = () => {};
    const feed: AccountFeed = {
      fetch: () => new Promise((resolve) => (answer = resolve)),
      watch: (_address, on) => {
        push = on;
        return () => {};
      },
    };
    const seen: Balance[] = [];
    watchTokenBalance(feed, ADDRESS, (b) => seen.push(b));

    push(token(10n, 20));
    answer(token(1n, 15));
    await new Promise((r) => setTimeout(r, 0));

    expect(seen).toEqual([{ status: 'live', amount: 10n, slot: 20 }]);
  });
});
