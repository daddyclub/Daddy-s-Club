/**
 * Дзеркало programs/daddys-club/src/math.rs.
 *
 * Дублювання навмисне: інтерфейс має рахувати претензію до виплати без
 * звернення до програми. Обидва файли покриваються спільними фікстурами з
 * fixtures/math.json — розбіжність між ними має бути червоним тестом, а не
 * сюрпризом на показі.
 *
 * Дзеркалиться не лише результат, а й арифметична модель Rust: `bigint` не
 * переповнюється сам, тому кожен крок явно звіряється з межею u128 — саме там,
 * де в math.rs стоїть `checked_*`. Інакше там, де Rust віддає `None`, тут
 * вийшло б велике число, і фікстури спіймали б розбіжність. Порядок перевірок
 * теж має значення: `pledged_share(u128::MAX, 10_000)` падає на множенні, хоча
 * ділення повернуло б результат у межі.
 *
 * `null` == `None` у Rust: переповнення, ділення на нуль і чекпоінт із
 * майбутнього — це не «неможливо», а помилки, які виклик мусить назвати на
 * своїй межі.
 */

/** Масштаб кумулятивного індексу виплати на одиницю бонду. */
export const SCALE = 1_000_000_000_000n;

export const BPS_DENOM = 10_000n;

/** Межа типу, в якому рахує програма. У TS її треба тримати руками. */
const U128_MAX = (1n << 128n) - 1n;

/** Межа типу bps у сигнатурах math.rs. */
const U16_MAX = 65_535;

/**
 * Як розійшлося одне надходження: скільки пішло в ескроу погашення і скільки
 * лишилося емітенту.
 */
export interface Split {
  readonly toEscrow: bigint;
  readonly toIssuer: bigint;
}

/** Чи вкладається значення у u128 — домен усіх аргументів-сум. */
function isU128(value: bigint): boolean {
  return value >= 0n && value <= U128_MAX;
}

/** Чи вкладається значення у u16 — домен усіх аргументів-bps. */
function isU16(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= U16_MAX;
}

function checkedMul(a: bigint, b: bigint): bigint | null {
  const product = a * b;
  return product <= U128_MAX ? product : null;
}

function checkedDiv(a: bigint, b: bigint): bigint | null {
  // Обидва операнди невід'ємні, тому ділення bigint (до нуля) і є округленням
  // вниз, як `checked_div` у Rust.
  return b === 0n ? null : a / b;
}

function checkedAdd(a: bigint, b: bigint): bigint | null {
  const sum = a + b;
  return sum <= U128_MAX ? sum : null;
}

function checkedSub(a: bigint, b: bigint): bigint | null {
  return a >= b ? a - b : null;
}

/**
 * Повне зобов'язання випуску: номінал + купон (`FR-018`).
 *
 * Рахується один раз при створенні випуску і більше не змінюється — купон не
 * залежить від того, як швидко надходить revenue.
 */
export function obligationTotal(face: bigint, couponBps: number): bigint | null {
  if (!isU128(face) || !isU16(couponBps)) return null;
  const scaled = checkedMul(face, BigInt(couponBps));
  if (scaled === null) return null;
  const coupon = checkedDiv(scaled, BPS_DENOM);
  if (coupon === null) return null;
  return checkedAdd(face, coupon);
}

/** Узгоджена частка від надходження, до врахування залишку зобов'язання. */
export function pledgedShare(amount: bigint, pledgeBps: number): bigint | null {
  if (!isU128(amount) || !isU16(pledgeBps)) return null;
  const scaled = checkedMul(amount, BigInt(pledgeBps));
  if (scaled === null) return null;
  return checkedDiv(scaled, BPS_DENOM);
}

/**
 * Розщеплення надходження в момент його виникнення (`FR-014`, `FR-019`,
 * `FR-020`).
 *
 * У сховище йде частка, але не більше за залишок зобов'язання: надлишок
 * повертається емітенту в тій самій транзакції. Коли залишок нульовий,
 * перехоплення припиняється саме собою — емітент отримує все.
 */
export function splitIntercept(
  amount: bigint,
  pledgeBps: number,
  remaining: bigint,
): Split | null {
  if (!isU128(remaining)) return null;
  const share = pledgedShare(amount, pledgeBps);
  if (share === null) return null;
  const toEscrow = share < remaining ? share : remaining;
  const toIssuer = checkedSub(amount, toEscrow);
  if (toIssuer === null) return null;
  return { toEscrow, toIssuer };
}

/**
 * Просування кумулятивного індексу виплати на одиницю бонду (`FR-015`).
 *
 * Вартість цієї операції не залежить від кількості власників — у цьому й є
 * сенс індексу. Ділення вниз лишає залишок у сховищі: він дістанеться
 * наступним claim'ам, а не зникне.
 */
export function advanceIndex(
  index: bigint,
  amount: bigint,
  bondSupply: bigint,
): bigint | null {
  if (!isU128(index) || !isU128(amount) || !isU128(bondSupply)) return null;
  if (bondSupply === 0n) return null;
  const scaled = checkedMul(amount, SCALE);
  if (scaled === null) return null;
  const perUnit = checkedDiv(scaled, bondSupply);
  if (perUnit === null) return null;
  return checkedAdd(index, perUnit);
}

/**
 * Скільки власник може забрати зараз (`FR-016`).
 *
 * Різниця індексу з моменту його чекпоінта, помножена на баланс, плюс те, що
 * вже нараховано гуком при передачах.
 */
export function claimable(
  index: bigint,
  checkpoint: bigint,
  balance: bigint,
  accrued: bigint,
): bigint | null {
  if (!isU128(index) || !isU128(checkpoint) || !isU128(balance) || !isU128(accrued)) {
    return null;
  }
  const delta = checkedSub(index, checkpoint);
  if (delta === null) return null;
  const scaled = checkedMul(delta, balance);
  if (scaled === null) return null;
  const earned = checkedDiv(scaled, SCALE);
  if (earned === null) return null;
  return checkedAdd(earned, accrued);
}
