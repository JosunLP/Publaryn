import { describe, expect, test } from 'bun:test';

import {
  formatSearchResultRepository,
  searchResultDiscoverySignals,
  searchResultHasDiscoveryDetails,
  searchResultRiskBadgeSeverity,
  searchResultRiskLabel,
} from '../src/pages/search-results';

describe('search result repository formatting', () => {
  test('includes repository name and slug when both are available', () => {
    expect(
      formatSearchResultRepository({
        repository_name: 'Release Packages',
        repository_slug: 'release-packages',
      })
    ).toBe('Release Packages (release-packages)');
  });

  test('falls back to whichever repository field is available', () => {
    expect(
      formatSearchResultRepository({
        repository_name: 'Release Packages',
        repository_slug: null,
      })
    ).toBe('Release Packages');

    expect(
      formatSearchResultRepository({
        repository_name: '  ',
        repository_slug: 'release-packages',
      })
    ).toBe('release-packages');

    expect(
      formatSearchResultRepository({
        repository_name: undefined,
        repository_slug: undefined,
      })
    ).toBe('');
  });
});

describe('search result discovery formatting', () => {
  test('formats risk labels and badges from discovery hints', () => {
    const result = {
      discovery: {
        risk_level: 'medium',
        signals: [
          '2 unresolved security findings',
          'Trusted publisher configured',
        ],
      },
    };

    expect(searchResultRiskLabel(result)).toBe('Medium risk');
    expect(searchResultRiskBadgeSeverity(result)).toBe('medium');
    expect(searchResultDiscoverySignals(result)).toEqual([
      '2 unresolved security findings',
      'Trusted publisher configured',
    ]);
  });

  test('falls back cleanly when discovery hints are missing', () => {
    expect(searchResultRiskLabel({ discovery: null })).toBe('Risk pending');
    expect(searchResultRiskBadgeSeverity({ discovery: null })).toBe('info');
    expect(searchResultDiscoverySignals({ discovery: null })).toEqual([]);
    expect(searchResultHasDiscoveryDetails({ discovery: null })).toBe(false);
  });

  test('treats blank discovery risk values as pending', () => {
    expect(
      searchResultRiskLabel({
        discovery: { risk_level: '   ', signals: [] },
      })
    ).toBe('Risk pending');
  });

  test('normalizes whitespace-padded discovery risk values for badge severity', () => {
    expect(
      searchResultRiskBadgeSeverity({
        discovery: { risk_level: ' moderate ', signals: [] },
      })
    ).toBe('medium');
  });

  test('only reports discovery details when meaningful signals exist', () => {
    expect(
      searchResultHasDiscoveryDetails({
        discovery: {
          risk_level: '   ',
          has_trusted_publisher: false,
          unresolved_security_finding_count: 0,
          signals: [],
        },
      })
    ).toBe(false);
    expect(
      searchResultHasDiscoveryDetails({
        discovery: {
          risk_level: null,
          has_trusted_publisher: true,
          unresolved_security_finding_count: 0,
          signals: [],
        },
      })
    ).toBe(true);
    expect(
      searchResultHasDiscoveryDetails({
        discovery: {
          risk_level: null,
          has_trusted_publisher: false,
          unresolved_security_finding_count: 2,
          signals: [],
        },
      })
    ).toBe(true);
  });
});
