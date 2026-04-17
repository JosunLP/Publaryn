/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import {
  formatRepositoryKindLabel,
  formatRepositoryPackageCoverageLabel,
  formatRepositoryVisibilityLabel,
  REPOSITORY_KIND_OPTIONS,
  REPOSITORY_VISIBILITY_OPTIONS,
} from '../src/utils/repositories';

describe('repository option helpers', () => {
  test('formats known repository kind values', () => {
    expect(formatRepositoryKindLabel('release')).toBe('Release');
    expect(formatRepositoryKindLabel('proxy')).toBe('Proxy');
  });

  test('formats known repository visibility values', () => {
    expect(formatRepositoryVisibilityLabel('internal_org')).toBe(
      'Internal org'
    );
    expect(formatRepositoryVisibilityLabel('quarantined')).toBe('Quarantined');
  });

  test('falls back to title casing unknown values', () => {
    expect(formatRepositoryVisibilityLabel('partner_only')).toBe(
      'Partner Only'
    );
  });

  test('summarizes visible package coverage for repository package lists', () => {
    expect(formatRepositoryPackageCoverageLabel(0, 0)).toBe(
      'No visible packages in this repository yet.'
    );
    expect(formatRepositoryPackageCoverageLabel(1, 1)).toBe(
      'Showing 1 visible package.'
    );
    expect(formatRepositoryPackageCoverageLabel(3, 3)).toBe(
      'Showing 3 visible packages.'
    );
    expect(formatRepositoryPackageCoverageLabel(20, 24)).toBe(
      'Showing 20 of 24 visible packages.'
    );
  });

  test('exposes the supported option sets', () => {
    expect(REPOSITORY_KIND_OPTIONS.map((option) => option.value)).toEqual([
      'public',
      'private',
      'staging',
      'release',
      'proxy',
      'virtual',
    ]);
    expect(REPOSITORY_VISIBILITY_OPTIONS.map((option) => option.value)).toEqual(
      ['public', 'private', 'internal_org', 'unlisted', 'quarantined']
    );
  });
});
