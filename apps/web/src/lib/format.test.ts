import { describe, expect, it } from 'vitest';
import { calendarDay, clock } from './format';

describe('calendarDay', () => {
  it('prints the board day the way the demo board does', () => {
    expect(calendarDay(new Date(2026, 9, 7, 23, 59, 59))).toBe('07 Oct 2026');
    expect(calendarDay(new Date(2026, 7, 26, 0, 0, 0))).toBe('26 Aug 2026');
  });

  it('turns over together with the clock beside it', () => {
    const lastSecond = new Date(2026, 11, 31, 23, 59, 59);
    const next = new Date(lastSecond.getTime() + 1000);
    expect([calendarDay(lastSecond), clock(lastSecond)]).toEqual(['31 Dec 2026', '23:59:59']);
    expect([calendarDay(next), clock(next)]).toEqual(['01 Jan 2027', '00:00:00']);
  });
});
