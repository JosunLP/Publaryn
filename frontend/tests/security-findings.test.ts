import { describe, expect, test } from 'bun:test';

import { severityLevel } from '../src/api/packages';

describe('severityLevel', () => {
  test('maps known severity strings to numeric levels', () => {
    expect(severityLevel('critical')).toBe(4);
    expect(severityLevel('high')).toBe(3);
    expect(severityLevel('medium')).toBe(2);
    expect(severityLevel('low')).toBe(1);
    expect(severityLevel('info')).toBe(0);
  });

  test('is case-insensitive', () => {
    expect(severityLevel('Critical')).toBe(4);
    expect(severityLevel('HIGH')).toBe(3);
    expect(severityLevel('Medium')).toBe(2);
    expect(severityLevel('LOW')).toBe(1);
    expect(severityLevel('INFO')).toBe(0);
  });

  test('returns -1 for unknown severity strings', () => {
    expect(severityLevel('unknown')).toBe(-1);
    expect(severityLevel('')).toBe(-1);
    expect(severityLevel('emergency')).toBe(-1);
  });
});
