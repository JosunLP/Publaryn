import { describe, expect, test } from 'bun:test';

import {
  buildOrgSecurityExportFilename,
  buildOrgSecurityPath,
  getOrgSecurityViewFromQuery,
  normalizeOrgSecurityEcosystem,
  normalizeOrgSecuritySeverityValues,
} from '../src/pages/org-security-query';

describe('org security query helpers', () => {
  test('parses valid severity, ecosystem, and package filters from query params', () => {
    const view = getOrgSecurityViewFromQuery(
      new URLSearchParams(
        'security_severity=critical,high&security_ecosystem=pypi&security_package=widget'
      )
    );

    expect(view).toEqual({
      severities: ['critical', 'high'],
      ecosystem: 'pypi',
      packageQuery: 'widget',
    });
  });

  test('drops invalid security filters back to safe defaults', () => {
    const view = getOrgSecurityViewFromQuery(
      new URLSearchParams(
        'security_severity=catastrophic&security_ecosystem=not-real&security_package=%20%20%20'
      )
    );

    expect(view).toEqual({
      severities: [],
      ecosystem: '',
      packageQuery: '',
    });
  });

  test('normalizes bun ecosystem aliases to npm', () => {
    expect(normalizeOrgSecurityEcosystem('bun')).toBe('npm');
  });

  test('deduplicates and orders supported severity filters', () => {
    expect(
      normalizeOrgSecuritySeverityValues(['low,critical', 'critical', 'info'])
    ).toEqual(['critical', 'low', 'info']);
  });

  test('builds org security paths while preserving unrelated query params', () => {
    const path = buildOrgSecurityPath(
      'acme-corp',
      {
        severities: ['critical', 'high'],
        ecosystem: 'npm',
        packageQuery: 'widget',
      },
      '?tab=security&page=2'
    );

    const url = new URL(path, 'https://example.test');

    expect(url.pathname).toBe('/orgs/acme-corp');
    expect(url.searchParams.get('tab')).toBe('security');
    expect(url.searchParams.get('page')).toBe('2');
    expect(url.searchParams.get('security_severity')).toBe('critical,high');
    expect(url.searchParams.get('security_ecosystem')).toBe('npm');
    expect(url.searchParams.get('security_package')).toBe('widget');
  });

  test('clears security filters while keeping unrelated query params intact', () => {
    const path = buildOrgSecurityPath(
      'acme-corp',
      {
        severities: [],
        ecosystem: '',
        packageQuery: '',
      },
      '?tab=security&security_severity=critical&security_ecosystem=npm&security_package=widget'
    );

    const url = new URL(path, 'https://example.test');

    expect(url.pathname).toBe('/orgs/acme-corp');
    expect(url.searchParams.get('tab')).toBe('security');
    expect(url.searchParams.get('security_severity')).toBeNull();
    expect(url.searchParams.get('security_ecosystem')).toBeNull();
    expect(url.searchParams.get('security_package')).toBeNull();
  });

  test('builds stable CSV export filenames from applied filters', () => {
    expect(
      buildOrgSecurityExportFilename(
        'acme-corp',
        {
          severities: ['critical', 'high'],
          ecosystem: 'npm',
          packageQuery: 'Acme Widget',
        },
        new Date('2026-04-19T10:30:00Z')
      )
    ).toBe(
      'org-security-acme-corp--critical_high--npm--package-acme-widget--2026-04-19.csv'
    );
  });

  test('falls back to a minimal security export filename when filters are empty', () => {
    expect(
      buildOrgSecurityExportFilename(
        'acme-corp',
        {
          severities: [],
          ecosystem: '',
          packageQuery: '',
        },
        new Date('2026-04-19T10:30:00Z')
      )
    ).toBe('org-security-acme-corp--2026-04-19.csv');
  });
});
