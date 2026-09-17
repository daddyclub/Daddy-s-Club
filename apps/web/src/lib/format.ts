export const amount = (value: number): string =>
  value.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });

export const usdc = (value: number): string => `${amount(value)} USDC`;

export const percent = (value: number): string => `${value.toFixed(1)}%`;

export const units = (value: number): string =>
  value.toLocaleString('en-US', { minimumFractionDigits: 0, maximumFractionDigits: 0 });

export const price = (value: number): string => value.toFixed(3);

export const ratio = (value: number): string => `${value.toFixed(2)}\u00d7`;

export const clock = (date: Date): string =>
  [date.getHours(), date.getMinutes(), date.getSeconds()]
    .map((part) => part.toString().padStart(2, '0'))
    .join(':');

/**
 * Мітка часу з ланцюга (`i64`, секунди) у дату. UTC навмисно: `maturity_ts`
 * порівнюється з годинником ланцюга, а не з тим, у якому поясі стоїть глядач.
 */
export const instant = (seconds: bigint): string =>
  `${new Date(Number(seconds) * 1000).toISOString().slice(0, 16).replace('T', ' ')} UTC`;

/** Bps так, як їх бачить глядач: 1200 → «12.00%». */
export const bps = (value: number): string => `${(value / 100).toFixed(2)}%`;

/**
 * Частка, вже порахована в `bigint` і вже округлена вниз. Тут вона тільки
 * друкується — `percent` з одним знаком округлив би її вгору й показав більше
 * виплаченого, ніж є.
 */
export const share = (value: number): string => `${value.toFixed(2)}%`;
