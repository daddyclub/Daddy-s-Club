/**
 * Гаманець застосунку поверх Wallet Standard. Склейка з React і нічого понад
 * те: що гаманець придатний і який акаунт брати, вирішує `lib/wallet.ts`.
 *
 * Транзакцію гаманець лише **підписує**, відправляємо ми — тим самим
 * з'єднанням, яке тримає підписки екрана. Так «транзакція підтверджена» і
 * «рахунок оновився» приходять від одного вузла, і `SC-009` міряється без
 * третьої сторони посередині.
 */

import {
  SolanaSignTransaction,
  type SolanaSignTransactionFeature,
} from '@solana/wallet-standard-features';
import {
  type Connection,
  PublicKey,
  Transaction,
  type TransactionInstruction,
} from '@solana/web3.js';
import { getWallets } from '@wallet-standard/app';
import type { Wallet, WalletAccount } from '@wallet-standard/base';
import {
  StandardConnect,
  type StandardConnectFeature,
  StandardDisconnect,
  type StandardDisconnectFeature,
  StandardEvents,
  type StandardEventsFeature,
} from '@wallet-standard/features';
import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react';
import type { Cluster } from '@/lib/env';
import { COMMITMENT } from '@/lib/rpc';
import { chainFor, isUsableWallet, solanaAccount } from '@/lib/wallet';

/** Під цим ключем пам'ятається обраний гаманець — зручність, не стан. */
const REMEMBERED = 'daddys-club:wallet';

function remember(name: string | null): void {
  try {
    if (name === null) localStorage.removeItem(REMEMBERED);
    else localStorage.setItem(REMEMBERED, name);
  } catch {
    // Приватне вікно або заблоковане сховище: просто не пам'ятаємо.
  }
}

function remembered(): string | null {
  try {
    return localStorage.getItem(REMEMBERED);
  } catch {
    return null;
  }
}

export interface WalletState {
  /** Придатні гаманці, які зареєструвались у цьому браузері. */
  readonly wallets: readonly Wallet[];
  readonly wallet: Wallet | null;
  readonly publicKey: PublicKey | null;
  readonly connecting: boolean;
  readonly error: string | null;
  connect(wallet: Wallet): Promise<void>;
  disconnect(): void;
  /** Підписати гаманцем, відправити й дочекатись `confirmed`. Повертає підпис. */
  submit(connection: Connection, instructions: readonly TransactionInstruction[]): Promise<string>;
}

const WalletContext = createContext<WalletState | null>(null);

export const WalletProvider = ({
  cluster,
  children,
}: {
  cluster: Cluster;
  children: ReactNode;
}) => {
  const [wallets, setWallets] = useState<readonly Wallet[]>([]);
  const [wallet, setWallet] = useState<Wallet | null>(null);
  const [account, setAccount] = useState<WalletAccount | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const chain = chainFor(cluster);

  useEffect(() => {
    const registry = getWallets();
    const refresh = () => setWallets(registry.get().filter(isUsableWallet));
    refresh();
    const offRegister = registry.on('register', refresh);
    const offUnregister = registry.on('unregister', refresh);
    return () => {
      offRegister();
      offUnregister();
    };
  }, []);

  const connectWith = useCallback(async (target: Wallet, silent: boolean) => {
    setConnecting(true);
    setError(null);
    try {
      const feature = target.features[
        StandardConnect
      ] as StandardConnectFeature[typeof StandardConnect];
      const { accounts } = await feature.connect(silent ? { silent: true } : undefined);
      const chosen = solanaAccount(accounts);
      if (chosen === null) {
        if (!silent) setError(`${target.name} has no Solana account to sign with.`);
        return;
      }
      setWallet(target);
      setAccount(chosen);
      remember(target.name);
    } catch (reason) {
      if (!silent) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setConnecting(false);
    }
  }, []);

  // Повернення на сторінку: той самий гаманець, без запиту, якщо він дозволяє.
  useEffect(() => {
    if (wallet !== null) return;
    const name = remembered();
    const known = wallets.find((candidate) => candidate.name === name);
    if (known !== undefined) void connectWith(known, true);
  }, [wallets, wallet, connectWith]);

  // Гаманець сам змінив акаунт або від'єднався.
  useEffect(() => {
    if (wallet === null || !(StandardEvents in wallet.features)) return;
    const events = wallet.features[StandardEvents] as StandardEventsFeature[typeof StandardEvents];
    return events.on('change', ({ accounts }) => {
      if (accounts === undefined) return;
      const next = solanaAccount(accounts);
      setAccount(next);
      if (next === null) setWallet(null);
    });
  }, [wallet]);

  const disconnect = useCallback(() => {
    if (wallet !== null && StandardDisconnect in wallet.features) {
      const feature = wallet.features[
        StandardDisconnect
      ] as StandardDisconnectFeature[typeof StandardDisconnect];
      void feature.disconnect().catch(() => undefined);
    }
    remember(null);
    setWallet(null);
    setAccount(null);
  }, [wallet]);

  const publicKey = useMemo(
    () => (account === null ? null : new PublicKey(account.publicKey)),
    [account],
  );

  const submit = useCallback(
    async (connection: Connection, instructions: readonly TransactionInstruction[]) => {
      if (wallet === null || account === null || publicKey === null) {
        throw new Error('Connect a wallet first.');
      }
      const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash(COMMITMENT);
      const transaction = new Transaction({
        feePayer: publicKey,
        blockhash,
        lastValidBlockHeight,
      }).add(...instructions);

      const feature = wallet.features[
        SolanaSignTransaction
      ] as SolanaSignTransactionFeature[typeof SolanaSignTransaction];
      const [signed] = await feature.signTransaction({
        account,
        chain,
        transaction: transaction.serialize({
          requireAllSignatures: false,
          verifySignatures: false,
        }),
      });
      if (signed === undefined) throw new Error('The wallet returned no signed transaction.');

      const signature = await connection.sendRawTransaction(signed.signedTransaction, {
        preflightCommitment: COMMITMENT,
      });
      const { value } = await connection.confirmTransaction(
        { signature, blockhash, lastValidBlockHeight },
        COMMITMENT,
      );
      if (value.err !== null) {
        throw new Error(`Transaction ${signature} failed on chain: ${JSON.stringify(value.err)}`);
      }
      return signature;
    },
    [wallet, account, publicKey, chain],
  );

  const state = useMemo<WalletState>(
    () => ({
      wallets,
      wallet,
      publicKey,
      connecting,
      error,
      connect: (target) => connectWith(target, false),
      disconnect,
      submit,
    }),
    [wallets, wallet, publicKey, connecting, error, connectWith, disconnect, submit],
  );

  return <WalletContext.Provider value={state}>{children}</WalletContext.Provider>;
};

export function useWallet(): WalletState {
  const state = useContext(WalletContext);
  if (state === null) throw new Error('useWallet поза WalletProvider');
  return state;
}
