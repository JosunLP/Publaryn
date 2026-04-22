import { describe, expect, test } from 'bun:test';

import type { SecurityFinding } from '../src/api/packages';
import {
  buildPackageSecurityFindingPath,
  buildPackageSecurityPath,
} from '../src/pages/package-security-links';

describe('package security link builders', () => {
  test('builds package security paths with normalized filter state', () => {
    const path = buildPackageSecurityPath('npm', '@acme/widget', {
      focusMode: 'resolved',
      includeResolved: true,
      searchQuery: ' pub-2026-0007 ',
      severities: ['high', 'critical'],
    });
    const url = new URL(path, 'https://example.test');

    expect(url.pathname).toBe('/packages/npm/%40acme%2Fwidget');
    expect(url.searchParams.get('tab')).toBe('security');
    expect(url.searchParams.get('security_focus')).toBe('resolved');
    expect(url.searchParams.get('security_include_resolved')).toBe('true');
    expect(url.searchParams.get('security_search')).toBe('pub-2026-0007');
    expect(url.searchParams.get('security_severity')).toBe('critical,high');
  });

  test('builds finding-specific security paths for resolved findings', () => {
    const finding = {
      id: 'finding-1',
      advisory_id: 'PUB-2026-0007',
      title: 'Prototype pollution',
      severity: 'critical',
      is_resolved: true,
    } satisfies Pick<
      SecurityFinding,
      'advisory_id' | 'id' | 'is_resolved' | 'severity' | 'title'
    >;

    const path = buildPackageSecurityFindingPath('npm', 'demo-widget', finding);
    const url = new URL(path, 'https://example.test');

    expect(url.pathname).toBe('/packages/npm/demo-widget');
    expect(url.searchParams.get('tab')).toBe('security');
    expect(url.searchParams.get('security_focus')).toBe('resolved');
    expect(url.searchParams.get('security_include_resolved')).toBe('true');
    expect(url.searchParams.get('security_search')).toBe('PUB-2026-0007');
    expect(url.searchParams.get('security_severity')).toBe('critical');
  });

  test('falls back to the finding title when advisory ids are absent', () => {
    const finding = {
      id: 'finding-2',
      title: 'Unsigned artifact',
      severity: 'low',
      is_resolved: false,
    } satisfies Pick<
      SecurityFinding,
      'advisory_id' | 'id' | 'is_resolved' | 'severity' | 'title'
    >;

    const path = buildPackageSecurityFindingPath('cargo', 'demo-widget', finding);
    const url = new URL(path, 'https://example.test');

    expect(url.searchParams.get('security_focus')).toBeNull();
    expect(url.searchParams.get('security_include_resolved')).toBeNull();
    expect(url.searchParams.get('security_search')).toBe('Unsigned artifact');
    expect(url.searchParams.get('security_severity')).toBe('low');
  });
});
