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
