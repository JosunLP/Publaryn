import { describe, expect, test } from 'bun:test';

import type { SecurityFinding } from '../src/api/packages';
import {
  buildOrgSecurityPackageKey,
  mergeUpdatedOrgSecurityFinding,
  sortOrgSecurityFindings,
} from '../src/pages/org-security-triage';

function makeFinding(
  id: string,
  overrides: Partial<SecurityFinding> = {}
): SecurityFinding {
  return {
    id,
    kind: 'vulnerability',
    severity: 'medium',
    title: `Finding ${id}`,
    is_resolved: false,
    detected_at: '2026-04-20T12:00:00Z',
    ...overrides,
  };
}

describe('org security triage helpers', () => {
  test('builds stable package keys for dashboard state', () => {
    expect(buildOrgSecurityPackageKey('NPM', 'acme-widget')).toBe(
      'npm::acme-widget'
    );
    expect(buildOrgSecurityPackageKey(null, null)).toBe('::');
  });

  test('sorts findings by severity and recency', () => {
    const findings = sortOrgSecurityFindings([
      makeFinding('low-new', {
        severity: 'low',
        detected_at: '2026-04-20T13:00:00Z',
      }),
      makeFinding('critical-old', {
        severity: 'critical',
        detected_at: '2026-04-20T11:00:00Z',
      }),
      makeFinding('critical-new', {
        severity: 'critical',
        detected_at: '2026-04-20T14:00:00Z',
      }),
    ]);

    expect(findings.map((finding) => finding.id)).toEqual([
      'critical-new',
      'critical-old',
      'low-new',
    ]);
  });

  test('merges updated findings and can drop resolved rows when desired', () => {
    const updated = makeFinding('finding-1', {
      severity: 'critical',
      is_resolved: true,
      detected_at: '2026-04-20T15:00:00Z',
    });

    expect(
      mergeUpdatedOrgSecurityFinding([makeFinding('finding-1')], updated, {
        includeResolved: true,
      })
    ).toEqual([updated]);

    expect(
      mergeUpdatedOrgSecurityFinding([makeFinding('finding-1')], updated, {
        includeResolved: false,
      })
    ).toEqual([]);
  });
});
