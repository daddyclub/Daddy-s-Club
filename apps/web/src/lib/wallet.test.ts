import type { Wallet, WalletAccount } from '@wallet-standard/base';
import { describe, expect, it } from 'vitest';
import { chainFor, isUsableWallet, solanaAccount } from './wallet';

const wallet = (features: string[], chains: `${string}:${string}`[]): Wallet => ({
  version: '1.0.0',
  name: 'Test',
  icon: 'data:image/svg+xml;base64,',
  chains,
  features: Object.fromEntries(features.map((f) => [f, {}])),
  accounts: [],
});

const account = (
  features: `${string}:${string}`[],
  chains: `${string}:${string}`[],
): WalletAccount => ({
  address: 'x',
  publicKey: new Uint8Array(32),
  chains,
  features,
});

describe('wallet', () => {
  it('ланцюг стандарту: mainnet-beta зветься mainnet', () => {
    expect(chainFor('localnet')).toBe('solana:localnet');
    expect(chainFor('devnet')).toBe('solana:devnet');
    expect(chainFor('mainnet-beta')).toBe('solana:mainnet');
  });

  it('придатний — лише той, хто підключається й підписує транзакції на Solana', () => {
    expect(
      isUsableWallet(wallet(['standard:connect', 'solana:signTransaction'], ['solana:devnet'])),
    ).toBe(true);
    expect(isUsableWallet(wallet(['standard:connect'], ['solana:devnet']))).toBe(false);
    expect(
      isUsableWallet(wallet(['standard:connect', 'solana:signTransaction'], ['ethereum:1'])),
    ).toBe(false);
  });

  it('акаунт — перший, що підписує на Solana', () => {
    const evm = account(['solana:signTransaction'], ['ethereum:1']);
    const signOnly = account(['solana:signMessage'], ['solana:devnet']);
    const good = account(['solana:signTransaction'], ['solana:devnet']);
    expect(solanaAccount([evm, signOnly, good])).toBe(good);
    expect(solanaAccount([evm])).toBeNull();
  });
});
