import { describe, expect, test } from 'bun:test';

import { severityLevel } from '../src/api/packages';
import {
  normalizeSecuritySeverityCounts,
  securitySeverityRank,
  worstSecuritySeverityFromCounts,
} from '../src/utils/security';

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

  test('normalizes sparse severity counts into a complete map', () => {
    expect(
      normalizeSecuritySeverityCounts({
        critical: 2,
        medium: null,
        low: 1.8,
      })
    ).toEqual({
      critical: 2,
      high: 0,
      medium: 0,
      low: 1,
      info: 0,
    });
  });

  test('derives the worst severity from aggregated counts', () => {
    expect(
      worstSecuritySeverityFromCounts({
        high: 1,
        low: 4,
      })
    ).toBe('high');
    expect(worstSecuritySeverityFromCounts({ info: 3 })).toBe('info');
    expect(worstSecuritySeverityFromCounts({})).toBe('info');
  });

  test('ranks normalized severities consistently', () => {
    expect(securitySeverityRank('critical')).toBeGreaterThan(
      securitySeverityRank('high')
    );
    expect(securitySeverityRank('high')).toBeGreaterThan(
      securitySeverityRank('medium')
    );
    expect(securitySeverityRank('unknown')).toBe(0);
  });
});
