import { describe, expect, test } from 'bun:test';

import type { SecurityFinding } from '../src/api/packages';
import {
  buildPackageSecurityEmptyStateMessage,
  buildPackageSecurityFilterSummary,
  countPackageSecurityFindingsBySeverity,
  filterPackageSecurityFindings,
  normalizePackageSecurityFocusMode,
  normalizePackageSecuritySearchQuery,
  normalizePackageSecuritySeverityFilters,
} from '../src/pages/package-security';

const FINDINGS: SecurityFinding[] = [
  {
    id: 'finding-1',
    kind: 'vulnerability',
    severity: 'high',
    title: 'Prototype pollution',
    description: 'User-controlled merge input can pollute object prototypes.',
    advisory_id: 'CVE-2026-0001',
    is_resolved: false,
    detected_at: '2026-04-12T10:00:00Z',
    release_version: '1.2.3',
    artifact_filename: 'demo-widget-1.2.3.tgz',
  },
  {
    id: 'finding-2',
    kind: 'malware',
    severity: 'critical',
    title: 'Known malicious payload',
    description: 'Scanner detected a malicious embedded payload.',
    advisory_id: 'PUB-2026-0007',
    is_resolved: true,
    resolved_at: '2026-04-15T10:00:00Z',
    detected_at: '2026-04-14T10:00:00Z',
    release_version: '1.2.4',
    artifact_filename: 'demo-widget-1.2.4.tgz',
  },
  {
    id: 'finding-3',
    kind: 'policy_violation',
    severity: 'low',
    title: 'Unsigned artifact',
    description: 'Artifact was published without an attached signature.',
    is_resolved: false,
    detected_at: '2026-04-11T10:00:00Z',
    release_version: '1.2.2',
    artifact_filename: 'demo-widget-1.2.2.tgz',
  },
];

describe('package security helpers', () => {
  test('normalizes filter inputs to safe defaults', () => {
    expect(normalizePackageSecuritySearchQuery('  CVE-2026-0001  ')).toBe(
      'CVE-2026-0001'
    );
    expect(normalizePackageSecurityFocusMode('resolved')).toBe('resolved');
    expect(normalizePackageSecurityFocusMode('unknown')).toBe('triage');
    expect(
      normalizePackageSecuritySeverityFilters([' HIGH ', 'critical', 'high'])
    ).toEqual(['critical', 'high']);
  });

  test('filters findings by reviewer triage queue, severity, and search query', () => {
    expect(filterPackageSecurityFindings(FINDINGS)).toEqual([
      FINDINGS[0],
      FINDINGS[2],
    ]);

    expect(
      filterPackageSecurityFindings(FINDINGS, {
        focusMode: 'resolved',
      })
    ).toEqual([FINDINGS[1]]);

    expect(
      filterPackageSecurityFindings(FINDINGS, {
        focusMode: 'all',
        severities: ['critical'],
        searchQuery: 'pub-2026-0007',
      })
    ).toEqual([FINDINGS[1]]);
  });

  test('counts filtered findings by severity', () => {
    expect(countPackageSecurityFindingsBySeverity(FINDINGS)).toEqual({
      critical: 1,
      high: 1,
      medium: 0,
      low: 1,
      info: 0,
    });
  });

  test('builds reviewer-focused summaries and empty-state guidance', () => {
    expect(
      buildPackageSecurityFilterSummary({
        totalLoadedCount: 3,
        visibleCount: 1,
        includeResolvedFindings: true,
        filters: {
          focusMode: 'resolved',
          severities: ['critical'],
          searchQuery: 'pub-2026-0007',
        },
      })
    ).toBe(
      'Showing 1 of 3 loaded findings, from resolved history, filtered to Critical severity, matching "pub-2026-0007".'
    );

    expect(
      buildPackageSecurityFilterSummary({
        totalLoadedCount: 2,
        visibleCount: 0,
        includeResolvedFindings: false,
        filters: {
          focusMode: 'resolved',
        },
      })
    ).toBe(
      'Showing 0 of 2 loaded findings, from resolved history. Load resolved findings to review resolved history.'
    );

    expect(
      buildPackageSecurityEmptyStateMessage({
        totalLoadedCount: 2,
        includeResolvedFindings: false,
        filters: {
          focusMode: 'resolved',
        },
      })
    ).toBe('Load resolved findings to review resolved history for this package.');
  });
});
