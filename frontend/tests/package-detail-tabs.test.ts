/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import {
  buildPackageDetailPath,
  getPackageDetailTabFromQuery,
  normalizePackageDetailTab,
} from '../src/pages/package-detail-tabs';

describe('package detail tab helpers', () => {
  test('normalizes supported tabs and falls back to readme', () => {
    expect(normalizePackageDetailTab('security')).toBe('security');
    expect(normalizePackageDetailTab('settings')).toBe('settings');
    expect(normalizePackageDetailTab('  Versions  ')).toBe('versions');
    expect(normalizePackageDetailTab('unknown')).toBe('readme');
    expect(normalizePackageDetailTab(null)).toBe('readme');
  });

  test('reads the active tab from the query string', () => {
    expect(
      getPackageDetailTabFromQuery(new URLSearchParams('tab=security'))
    ).toBe('security');
    expect(
      getPackageDetailTabFromQuery(new URLSearchParams('tab=settings'))
    ).toBe('settings');
    expect(getPackageDetailTabFromQuery(new URLSearchParams('tab=nope'))).toBe(
      'readme'
    );
  });

  test('builds package detail paths while preserving unrelated query params', () => {
    const path = buildPackageDetailPath(
      'npm',
      '@acme/widget',
      { tab: 'settings' },
      '?foo=bar'
    );
    const url = new URL(path, 'https://example.test');

    expect(url.pathname).toBe('/packages/npm/%40acme%2Fwidget');
    expect(url.searchParams.get('foo')).toBe('bar');
    expect(url.searchParams.get('tab')).toBe('settings');
  });

  test('builds package detail paths with URL-backed security filters', () => {
    const path = buildPackageDetailPath(
      'npm',
      '@acme/widget',
      {
        tab: 'security',
        securityView: {
          focusMode: 'resolved',
          includeResolved: true,
          searchQuery: ' pub-2026-0007 ',
          severities: ['high', 'critical'],
        },
      },
      '?foo=bar'
    );
    const url = new URL(path, 'https://example.test');

    expect(url.pathname).toBe('/packages/npm/%40acme%2Fwidget');
    expect(url.searchParams.get('foo')).toBe('bar');
    expect(url.searchParams.get('tab')).toBe('security');
    expect(url.searchParams.get('security_focus')).toBe('resolved');
    expect(url.searchParams.get('security_include_resolved')).toBe('true');
    expect(url.searchParams.get('security_search')).toBe('pub-2026-0007');
    expect(url.searchParams.get('security_severity')).toBe('critical,high');
  });

  test('drops the tab query when switching back to the default readme view', () => {
    const path = buildPackageDetailPath(
      'cargo',
      'acme-widget',
      { tab: 'readme' },
      '?tab=security&notice=kept'
    );
    const url = new URL(path, 'https://example.test');

    expect(url.pathname).toBe('/packages/cargo/acme-widget');
    expect(url.searchParams.get('tab')).toBeNull();
    expect(url.searchParams.get('notice')).toBe('kept');
  });

  test('clears package security filters while preserving unrelated query params', () => {
    const path = buildPackageDetailPath(
      'cargo',
      'acme-widget',
      {
        tab: 'security',
        securityView: {
          focusMode: 'triage',
          includeResolved: false,
          searchQuery: '',
          severities: [],
        },
      },
      '?tab=security&notice=kept&security_focus=resolved&security_include_resolved=true&security_search=pub&security_severity=critical'
    );
    const url = new URL(path, 'https://example.test');

    expect(url.pathname).toBe('/packages/cargo/acme-widget');
    expect(url.searchParams.get('tab')).toBe('security');
    expect(url.searchParams.get('notice')).toBe('kept');
    expect(url.searchParams.get('security_focus')).toBeNull();
    expect(url.searchParams.get('security_include_resolved')).toBeNull();
    expect(url.searchParams.get('security_search')).toBeNull();
    expect(url.searchParams.get('security_severity')).toBeNull();
  });
});
