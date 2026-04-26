import { describe, expect, test } from 'bun:test';

import { titleCase } from '../src/utils/strings';

describe('titleCase', () => {
  test('normalizes mixed-case segments before title casing', () => {
    expect(titleCase('mIXed_CASE-value')).toBe('Mixed Case Value');
  });

  test('preserves allowlisted acronyms in uppercase', () => {
    expect(titleCase('oci_registry')).toBe('OCI Registry');
  });
});
