/**
 * The single source of data for the whole interface.
 * Nothing here is fetched, signed or derived at runtime — every figure
 * below is fixed and rendered as written. Today is 26 August 2026.
 */

export const TODAY = '26 Aug 2026';

export const PROTOCOL_SETTINGS = {
  originationFeePct: 1.5,
  secondaryFeePct: 0.5,
  maxRevenueSharePct: 50,
} as const;

export type IssueState = 'Subscribing' | 'Repaying' | 'Repaid' | 'Overdue' | 'Undersubscribed';

export const ISSUE_STATES: IssueState[] = [
  'Subscribing',
  'Repaying',
  'Repaid',
  'Overdue',
  'Undersubscribed',
];

export interface Issue {
  id: string;
  issuer: string;
  state: IssueState;
  face: number;
  couponPct: number;
  termDays: number;
  shareLabel: string;
  sharePct: number;
  matures: string;
  /** days to maturity from 26 Aug 2026; negative means past maturity */
  daysToMaturity: number | null;
  totalOwed: number | null;
  /** repayment or subscription progress, whichever the state calls for */
  progressPct: number;
  progressKind: 'repayment' | 'subscription';
  coverage: number | null;
  summary: string;
}

export const ISSUES: Issue[] = [
  {
    id: 'quillfin-swap',
    issuer: 'Quillfin Swap',
    state: 'Repaying',
    face: 250000,
    couponPct: 9.5,
    termDays: 90,
    shareLabel: '12%',
    sharePct: 12,
    matures: '18 Nov 2026',
    daysToMaturity: 84,
    totalOwed: 273750,
    progressPct: 61.5,
    progressKind: 'repayment',
    coverage: 1.63,
    summary: 'Fees are being split live. 61.5% of the obligation is repaid.',
  },
  {
    id: 'tanglewood-lend',
    issuer: 'Tanglewood Lend',
    state: 'Subscribing',
    face: 120000,
    couponPct: 7.0,
    termDays: 60,
    shareLabel: '8%',
    sharePct: 8,
    matures: '25 Oct 2026',
    daysToMaturity: 60,
    totalOwed: 128400,
    progressPct: 72.1,
    progressKind: 'subscription',
    coverage: 1.5,
    summary: 'Open for subscription. The split begins once the book closes.',
  },
  {
    id: 'copperline-dex',
    issuer: 'Copperline DEX',
    state: 'Repaid',
    face: 80000,
    couponPct: 6.5,
    termDays: 60,
    shareLabel: '15%',
    sharePct: 15,
    matures: '30 Jul 2026',
    daysToMaturity: null,
    totalOwed: 85200,
    progressPct: 100,
    progressKind: 'repayment',
    coverage: null,
    summary: 'Settled early. The revenue stream was released on 12 Jul 2026.',
  },
  {
    id: 'marrowbone-perps',
    issuer: 'Marrowbone Perps',
    state: 'Overdue',
    face: 400000,
    couponPct: 11.0,
    termDays: 75,
    shareLabel: '18% \u2192 50%',
    sharePct: 50,
    matures: '12 Aug 2026',
    daysToMaturity: -14,
    totalOwed: 444000,
    progressPct: 74.8,
    progressKind: 'repayment',
    coverage: null,
    summary: 'Past maturity. The pledged share rose automatically to the 50% ceiling.',
  },
  {
    id: 'saltmarsh-vaults',
    issuer: 'Saltmarsh Vaults',
    state: 'Undersubscribed',
    face: 150000,
    couponPct: 8.0,
    termDays: 90,
    shareLabel: '10%',
    sharePct: 10,
    matures: '\u2014',
    daysToMaturity: null,
    totalOwed: null,
    progressPct: 27.5,
    progressKind: 'subscription',
    coverage: null,
    summary: 'The window closed under target. Deposits are being returned in full.',
  },
];

export const findIssue = (id: string | undefined): Issue | undefined =>
  ISSUES.find((issue) => issue.id === id);

/* ---------------------------------------------------------------- */
/* Per-issue detail                                                  */
/* ---------------------------------------------------------------- */

export const QUILLFIN = {
  id: 'quillfin-swap',
  totalOwed: 273750,
  repaidStart: 168430,
  repaidPct: 61.5,
  remaining: 105320,
  avgDailyRevenue: 41200,
  dailyToHolders: 4944,
  sharePct: 12,
  coverage: 1.63,
  daysToRelease: 21,
} as const;

export const TANGLEWOOD = {
  totalOwed: 128400,
  raised: 86500,
  target: 120000,
  raisedPct: 72.1,
  closesInSeconds: 2 * 24 * 3600 + 14 * 3600,
  avgDailyRevenue: 40000,
  coverage: 1.5,
} as const;

export const COPPERLINE = {
  totalOwed: 85200,
  repaidInDays: 43,
  plannedDays: 60,
  releasedOn: '12 Jul 2026',
} as const;

export const MARROWBONE = {
  totalOwed: 444000,
  repaid: 331900,
  repaidPct: 74.8,
  remaining: 112100,
  overdueDays: 14,
  shareBefore: 18,
  shareNow: 50,
  revenueBefore: 55000,
  revenueNow: 12400,
} as const;

export const SALTMARSH = {
  raised: 41200,
  target: 150000,
  raisedPct: 27.5,
  windowClosed: '20 Aug 2026',
} as const;

/* ---------------------------------------------------------------- */
/* Holder position + secondary market                                */
/* ---------------------------------------------------------------- */

export interface Position {
  issueId: string;
  units: number;
  claimedToDate: number;
  claimableNow: number;
}

export const POSITIONS: Position[] = [
  {
    issueId: 'quillfin-swap',
    units: 25000,
    claimedToDate: 15000.34,
    claimableNow: 1842.66,
  },
];

export const findPosition = (issueId: string): Position | undefined =>
  POSITIONS.find((position) => position.issueId === issueId);

export interface Offer {
  id: string;
  issueId: string;
  units: number;
  pricePerUnit: number;
}

export const OFFERS: Offer[] = [
  { id: 'offer-1', issueId: 'quillfin-swap', units: 12500, pricePerUnit: 0.94 },
  { id: 'offer-2', issueId: 'quillfin-swap', units: 40000, pricePerUnit: 0.915 },
  { id: 'offer-3', issueId: 'quillfin-swap', units: 5000, pricePerUnit: 0.96 },
];

export const offersFor = (issueId: string): Offer[] =>
  OFFERS.filter((offer) => offer.issueId === issueId);

/* ---------------------------------------------------------------- */
/* Quillfin Swap — last 30 days of fee revenue                       */
/* Sums to exactly 1,236,000.00 (41,200.00 per day average).         */
/* ---------------------------------------------------------------- */

export interface RevenueDay {
  date: string;
  revenue: number;
}

const DAILY_REVENUE: number[] = [
  48000, 37900, 42400, 36000, 43700, 45300, 40500, 39300, 44500, 34400, 42100, 38700, 46400, 37100,
  43100, 41900, 40000, 40300, 45800, 39100, 44100, 37300, 42700, 40900, 45100, 36600, 43300, 39700,
  41500, 38300,
];

const dayLabel = (index: number): string => {
  // 30-day window ending 26 Aug 2026, starting 28 Jul 2026.
  const julyDays = 4; // 28, 29, 30, 31 July
  if (index < julyDays) return `${28 + index} Jul`;
  return `${index - julyDays + 1} Aug`;
};

export const REVENUE_30D: RevenueDay[] = DAILY_REVENUE.map((revenue, index) => ({
  date: dayLabel(index),
  revenue,
}));

export const ISSUER_30D = {
  totalRevenue: 1236000,
  splitToHolders: 148320,
  kept: 1087680,
  remainingObligation: 105320,
  projectedRelease: '16 Sep 2026',
} as const;

/* Seed rows for the fee tape, oldest last. Running totals land on
   168,430.00 — where the live counter picks up. */
export interface TapeRow {
  id: number;
  time: string;
  feeEarned: number;
  toHolders: number;
  runningTotal: number;
}

export const TAPE_SEED_SPLITS: number[] = [128.4, 96.72, 211.08, 64.2, 172.56, 88.32, 145.2, 57.96];
