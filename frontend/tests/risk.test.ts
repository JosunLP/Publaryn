import { describe, expect, test } from 'bun:test';

import { riskBadgeSeverity, riskLabel } from '../src/utils/risk';

describe('risk helpers', () => {
  test('trims severity inputs before mapping badge severity', () => {
    expect(riskBadgeSeverity(' moderate ')).toBe('medium');
    expect(riskBadgeSeverity(' high ')).toBe('high');
    expect(riskBadgeSeverity(' unknown ')).toBe('info');
  });

  test('preserves trimmed labels for display', () => {
    expect(riskLabel(' moderate ')).toBe('Moderate risk');
    expect(riskLabel(' high ')).toBe('High risk');
    expect(riskLabel(' critical ')).toBe('Critical risk');
    expect(riskLabel(' unknown ')).toBe('Unknown risk');
    expect(riskLabel('   ')).toBe('Risk pending');
  });
});
