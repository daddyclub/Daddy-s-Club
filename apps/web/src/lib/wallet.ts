/**
 * Гаманці — через Wallet Standard напряму (рішення 2026-09-26, T038):
 * `@solana/wallet-adapter-react` тягнув би `react-native` через мобільний
 * адаптер, а Phantom, Solflare і Backpack реєструються саме через стандарт.
 *
 * Тут — чисті перевірки, які ганяються тестом без браузера. React-обгортка —
 * `hooks/useWallet.tsx`.
 */

import { SolanaSignTransaction } from '@solana/wallet-standard-features';
import type { IdentifierString, Wallet, WalletAccount } from '@wallet-standard/base';
import { StandardConnect } from '@wallet-standard/features';
import type { Cluster } from './env';

/** Ланцюг стандарту для кластера збірки. `mainnet-beta` у стандарті зветься `mainnet`. */
export function chainFor(cluster: Cluster): IdentifierString {
  return cluster === 'mainnet-beta' ? 'solana:mainnet' : `solana:${cluster}`;
}

/**
 * Гаманець, яким можна користуватись тут: уміє підключитись і підписати
 * транзакцію, не відправляючи її. Відправляємо ми самі — через те саме
 * з'єднання, яке потім бачить пуш рахунку, — тож час до підтвердження
 * міряється на одному вузлі.
 */
export function isUsableWallet(wallet: Wallet): boolean {
  return (
    StandardConnect in wallet.features &&
    SolanaSignTransaction in wallet.features &&
    wallet.chains.some((chain) => chain.startsWith('solana:'))
  );
}

/** Перший акаунт, яким гаманець готовий підписувати на Solana. */
export function solanaAccount(accounts: readonly WalletAccount[]): WalletAccount | null {
  return (
    accounts.find(
      (account) =>
        account.chains.some((chain) => chain.startsWith('solana:')) &&
        account.features.includes(SolanaSignTransaction),
    ) ?? null
  );
}
