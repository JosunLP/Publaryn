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
    expect(normalizePackageDetailTab('  Versions  ')).toBe('versions');
    expect(normalizePackageDetailTab('unknown')).toBe('readme');
    expect(normalizePackageDetailTab(null)).toBe('readme');
  });

  test('reads the active tab from the query string', () => {
    expect(
      getPackageDetailTabFromQuery(new URLSearchParams('tab=security'))
    ).toBe('security');
    expect(getPackageDetailTabFromQuery(new URLSearchParams('tab=nope'))).toBe(
      'readme'
    );
  });

  test('builds package detail paths while preserving unrelated query params', () => {
    const path = buildPackageDetailPath(
      'npm',
      '@acme/widget',
      { tab: 'security' },
      '?foo=bar'
    );
    const url = new URL(path, 'https://example.test');

    expect(url.pathname).toBe('/packages/npm/%40acme%2Fwidget');
    expect(url.searchParams.get('foo')).toBe('bar');
    expect(url.searchParams.get('tab')).toBe('security');
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
});
