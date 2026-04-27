import { describe, expect, test } from 'bun:test';

import { titleCase } from '../src/utils/strings';

describe('titleCase', () => {
  test('handles mixed delimiters consistently', () => {
    expect(titleCase('foo_bar-baz')).toBe('Foo Bar Baz');
  });

  test('normalizes mixed-case segments before title casing', () => {
    expect(titleCase('mIXed_CASE-value')).toBe('Mixed Case Value');
  });

  test('preserves allowlisted acronyms in uppercase', () => {
    expect(titleCase('oci_registry')).toBe('OCI Registry');
  });

  test('returns an empty string for empty or whitespace-only input', () => {
    expect(titleCase('')).toBe('');
    expect(titleCase('   \t\n  ')).toBe('');
  });
});
