import { describe, expect, test } from 'bun:test';

import {
  REPOSITORY_KIND_OPTIONS,
  REPOSITORY_VISIBILITY_OPTIONS,
  formatRepositoryKindLabel,
  formatRepositoryVisibilityLabel,
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
