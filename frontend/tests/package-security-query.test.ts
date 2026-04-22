import { describe, expect, test } from 'bun:test';

import {
  applyPackageSecurityViewToSearchParams,
  getPackageSecurityViewFromQuery,
  normalizePackageSecurityIncludeResolved,
} from '../src/pages/pkg-security-url';

describe('package security query helpers', () => {
  test('parses valid filter state from query params', () => {
    const view = getPackageSecurityViewFromQuery(
      new URLSearchParams(
        'security_focus=resolved&security_include_resolved=true&security_search=pub-2026-0007&security_severity=critical,high'
      )
    );

    expect(view).toEqual({
      focusMode: 'resolved',
      includeResolved: true,
      searchQuery: 'pub-2026-0007',
      severities: ['critical', 'high'],
    });
  });

  test('drops invalid query state back to safe defaults', () => {
    const view = getPackageSecurityViewFromQuery(
      new URLSearchParams(
        'security_focus=unknown&security_include_resolved=nope&security_search=%20%20&security_severity=catastrophic'
      )
    );

    expect(view).toEqual({
      focusMode: 'triage',
      includeResolved: false,
      searchQuery: '',
      severities: [],
    });
  });

  test('normalizes truthy include-resolved values', () => {
    expect(normalizePackageSecurityIncludeResolved('true')).toBe(true);
    expect(normalizePackageSecurityIncludeResolved('1')).toBe(true);
    expect(normalizePackageSecurityIncludeResolved('false')).toBe(false);
  });

  test('writes and clears package security query params', () => {
    const params = new URLSearchParams('tab=security&notice=kept');

    applyPackageSecurityViewToSearchParams(params, {
      focusMode: 'resolved',
      includeResolved: true,
      searchQuery: ' pub-2026-0007 ',
      severities: ['high', 'critical', 'high'],
    });

    expect(params.get('tab')).toBe('security');
    expect(params.get('notice')).toBe('kept');
    expect(params.get('security_focus')).toBe('resolved');
    expect(params.get('security_include_resolved')).toBe('true');
    expect(params.get('security_search')).toBe('pub-2026-0007');
    expect(params.get('security_severity')).toBe('critical,high');

    applyPackageSecurityViewToSearchParams(params, {
      focusMode: 'triage',
      includeResolved: false,
      searchQuery: '',
      severities: [],
    });

    expect(params.get('security_focus')).toBeNull();
    expect(params.get('security_include_resolved')).toBeNull();
    expect(params.get('security_search')).toBeNull();
    expect(params.get('security_severity')).toBeNull();
    expect(params.get('notice')).toBe('kept');
  });
});
