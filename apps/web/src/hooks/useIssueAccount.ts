import { useEffect, useState } from 'react';
import type { PublicKey } from '@solana/web3.js';
import { type IssueSnapshot, watchIssue } from '@/lib/issue-feed';
import type { AccountFeed } from '@/lib/rpc';

/**
 * Склейка з React, і нічого понад те. Уся робота — у `watchIssue`, який
 * ганяється тестом без браузера; тут лишається рівно те, за що відповідає
 * React: зміна адреси знімає стару підписку, розмонтування знімає останню.
 *
 * Адреса приймається об'єктом, тому викликач мусить її запам'ятати
 * (`useMemo` за рядком з URL). Новий `PublicKey` на кожен рендер
 * перепідписував би сокет по колу.
 */
export function useIssueAccount(
  feed: AccountFeed,
  address: PublicKey,
  programId: PublicKey,
): IssueSnapshot {
  const [snapshot, setSnapshot] = useState<IssueSnapshot>({ status: 'loading' });

  useEffect(() => {
    setSnapshot({ status: 'loading' });
    return watchIssue(feed, address, programId, setSnapshot);
  }, [feed, address, programId]);

  return snapshot;
}
