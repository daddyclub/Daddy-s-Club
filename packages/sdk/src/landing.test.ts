/**
 * The landing page reads the standing devnet issue straight from RPC in the
 * visitor's browser, with no build step and no access to this package. It
 * therefore carries its own copy of the `Issue` layout: the account length,
 * the discriminator, the owning program and the byte offsets of the two
 * figures it prints. A layout change here would leave that copy printing wrong
 * numbers without a sound, so this test reads the copy out of the page and
 * holds it against `decodeIssue`.
 */

import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  ACCOUNT_SPACE,
  DISCRIMINATOR_LEN,
  DISCRIMINATORS,
  decodeIssue,
  ISSUE_STATES,
} from './accounts.js';

const page = readFileSync(new URL('../../../apps/landing/index.html', import.meta.url), 'utf8');
const anchorToml = readFileSync(new URL('../../../Anchor.toml', import.meta.url), 'utf8');

interface LandingLayout {
  readonly program: string;
  readonly length: number;
  readonly discriminator: readonly number[];
  readonly obligationAt: number;
  readonly repaidAt: number;
}

function landingLayout(): LandingLayout {
  const match = /const ISSUE_LAYOUT = (\{[^;]*\});/.exec(page);
  if (!match?.[1]) throw new Error('ISSUE_LAYOUT not found in apps/landing/index.html');
  return JSON.parse(match[1]) as LandingLayout;
}

/** Reads a little-endian u64 the way the page does, not the way the SDK does. */
function u64At(bytes: Uint8Array, offset: number): bigint {
  let value = 0n;
  for (let i = 7; i >= 0; i -= 1) value = (value << 8n) | BigInt(bytes[offset + i] ?? 0);
  return value;
}

/** An Issue account with every byte distinct from its neighbours. */
function scrambledIssue(): Uint8Array {
  const bytes = new Uint8Array(DISCRIMINATOR_LEN + ACCOUNT_SPACE.Issue);
  for (let i = 0; i < bytes.length; i += 1) bytes[i] = (i * 37 + 11) % 256;
  bytes.set(DISCRIMINATORS.Issue, 0);
  bytes[DISCRIMINATOR_LEN + 204] = ISSUE_STATES.indexOf('Repaying');
  return bytes;
}

describe('landing page copy of the Issue layout', () => {
  const layout = landingLayout();

  it('has the account length and discriminator of Issue', () => {
    expect(layout.length).toBe(DISCRIMINATOR_LEN + ACCOUNT_SPACE.Issue);
    expect(layout.discriminator).toEqual(Array.from(DISCRIMINATORS.Issue));
  });

  it('reads the same two figures as decodeIssue', () => {
    const bytes = scrambledIssue();
    const issue = decodeIssue(bytes);
    expect(u64At(bytes, layout.obligationAt)).toBe(issue.obligationTotal);
    expect(u64At(bytes, layout.repaidAt)).toBe(issue.repaidTotal);
  });

  it('expects the owner the core program is deployed under on devnet', () => {
    const devnet = /\[programs\.devnet\][^[]*?daddys_club\s*=\s*"(\w+)"/.exec(anchorToml);
    expect(devnet?.[1]).toBeDefined();
    expect(layout.program).toBe(devnet?.[1]);
  });
});
