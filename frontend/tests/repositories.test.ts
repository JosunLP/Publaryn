/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import {
  formatRepositoryKindLabel,
  formatRepositoryPackageCoverageLabel,
  formatRepositoryVisibilityLabel,
  REPOSITORY_KIND_OPTIONS,
  REPOSITORY_VISIBILITY_OPTIONS,
  resolveRepositoryOwnerSummary,
  selectRepositoryTransferTargets,
  selectTransferableRepositories,
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

  test('resolves repository owner summaries for org and user owners', () => {
    expect(
      resolveRepositoryOwnerSummary({
        ownerOrgName: 'Acme Corp',
        ownerOrgSlug: 'acme-corp',
      })
    ).toEqual({
      label: 'Acme Corp (@acme-corp)',
      href: '/orgs/acme-corp',
    });

    expect(
      resolveRepositoryOwnerSummary({
        ownerUsername: 'alice',
      })
    ).toEqual({
      label: 'alice',
      href: '/search?q=alice',
    });

    expect(resolveRepositoryOwnerSummary({})).toEqual({
      label: 'Unknown owner',
      href: null,
    });
  });

  test('filters repository transfer targets to admin roles outside the current owner organization', () => {
    const targets = selectRepositoryTransferTargets(
      [
        { slug: 'viewer-org', name: 'Viewer Org', role: 'viewer' },
        { slug: 'target-b', name: 'Zulu Org', role: 'owner' },
        { slug: 'source-org', name: 'Source Org', role: 'admin' },
        { slug: 'target-a', name: 'Alpha Org', role: 'admin' },
        { slug: null, name: 'Missing slug', role: 'owner' },
      ],
      'source-org'
    );

    expect(targets).toEqual([
      { slug: 'target-a', name: 'Alpha Org', role: 'admin' },
      { slug: 'target-b', name: 'Zulu Org', role: 'owner' },
    ]);
  });

  test('keeps only transferable repositories and sorts them by display label', () => {
    const repositories = selectTransferableRepositories([
      { slug: 'zeta-registry', name: 'Zulu Registry', can_transfer: true },
      { slug: 'alpha-registry', name: 'Alpha Registry', can_transfer: true },
      { slug: 'source-registry', name: 'Source Registry', can_transfer: false },
      { slug: '', name: 'Missing slug', can_transfer: true },
      { slug: 'beta-registry', name: null, can_transfer: true },
    ]);

    expect(repositories).toEqual([
      { slug: 'alpha-registry', name: 'Alpha Registry', can_transfer: true },
      { slug: 'beta-registry', name: null, can_transfer: true },
      { slug: 'zeta-registry', name: 'Zulu Registry', can_transfer: true },
    ]);
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
