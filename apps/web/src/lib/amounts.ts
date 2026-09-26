/**
 * Суми, які вводить людина, у цілі одиниці ланцюга — без `number` посередині.
 *
 * `Number.parseFloat("0.1") * 1e6` дає 100000.00000000001, і після `BigInt`
 * такий лот або падає, або тихо стає іншим. Тому рядок розбирається цифрами:
 * ціла частина й дробова окремо, дробова — не довша за кількість знаків мінта.
 * Зайвий знак — відмова, а не округлення: продавець мусить побачити, що саме
 * піде в оферту (`FR-024`: ціну задає він, протокол її не рахує).
 */

import { USDC_DECIMALS } from './issue-feed';

/** Кома й крапка рівноправні: на кома-локалі «0.93» інакше не набрати. */
export function parseAmount(text: string, decimals: number = USDC_DECIMALS): bigint | null {
  const trimmed = text.trim().replace(',', '.');
  const match = /^(\d+)(?:\.(\d*))?$/.exec(trimmed) ?? /^()\.(\d+)$/.exec(trimmed);
  if (match === null) return null;

  const whole = match[1] ?? '';
  const fraction = match[2] ?? '';
  if (fraction.length > decimals) return null;

  return BigInt(`${whole || '0'}${fraction.padEnd(decimals, '0')}`);
}

/** Точне подання цілих одиниць: без групування й без втрати хвоста. */
export function formatUnits(value: bigint, decimals: number = USDC_DECIMALS): string {
  const negative = value < 0n;
  const digits = (negative ? -value : value).toString().padStart(decimals + 1, '0');
  const whole = digits.slice(0, digits.length - decimals);
  const fraction = digits.slice(digits.length - decimals).replace(/0+$/, '');
  return `${negative ? '-' : ''}${whole}${fraction === '' ? '' : `.${fraction}`}`;
}

/**
 * Ціна за одиницю номіналу — лише для показу. У ланцюгу її немає: оферта
 * зберігає суму за лот, і саме її платить покупець.
 */
export function pricePerFace(price: bigint, amount: bigint): number {
  if (amount === 0n) return 0;
  // Шість знаків точності — більше екран не показує.
  return Number((price * 1_000_000n) / amount) / 1_000_000;
}

/**
 * Сума угоди для екрана: групування тисяч, щонайменше два знаки, **без
 * округлення** — `4875.5` → `4,875.50`, `0.000001` → `0.000001`. `amount()` із
 * `format.ts` округлює до двох знаків, і комісія в 0,005 USDC показалась би як
 * 0,01 — більше, ніж утримають.
 */
export function showUnits(value: bigint, decimals: number = USDC_DECIMALS): string {
  const [whole = '0', fraction = ''] = formatUnits(value, decimals).split('.');
  const grouped = whole.replace(/\B(?=(\d{3})+(?!\d))/g, ',');
  return `${grouped}.${fraction.padEnd(2, '0')}`;
}
