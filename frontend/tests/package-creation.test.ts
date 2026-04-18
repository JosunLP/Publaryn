/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import type { OrgRepositorySummary } from '../src/api/orgs';
import {
  formatPackageCreationRepositoryLabel,
  getAllowedPackageVisibilityOptions,
  isRepositoryEligibleForPackageCreation,
  selectCreatableRepositories,
} from '../src/utils/package-creation';

describe('package creation helpers', () => {
  test('filters repositories to package-creatable kinds and stable slugs', () => {
    const repositories: OrgRepositorySummary[] = [
      {
        name: 'Virtual Mirror',
        slug: 'virtual-mirror',
        kind: 'virtual',
        visibility: 'public',
      },
      {
        name: 'Release Packages',
        slug: 'release-packages',
        kind: 'release',
        visibility: 'private',
      },
      {
        name: 'Acme Public',
        slug: 'acme-public',
        kind: 'public',
        visibility: 'public',
      },
      {
        name: 'Proxy Cache',
        slug: 'proxy-cache',
        kind: 'proxy',
        visibility: 'public',
      },
      {
        name: 'Missing slug',
        kind: 'private',
        visibility: 'private',
      },
    ];

    expect(selectCreatableRepositories(repositories)).toEqual([
      {
        name: 'Acme Public',
        slug: 'acme-public',
        kind: 'public',
        visibility: 'public',
      },
      {
        name: 'Release Packages',
        slug: 'release-packages',
        kind: 'release',
        visibility: 'private',
      },
    ]);
  });

  test('formats repository labels for package-creation selects', () => {
    expect(
      formatPackageCreationRepositoryLabel({
        name: 'Release Packages',
        slug: 'release-packages',
        kind: 'release',
        visibility: 'internal_org',
      })
    ).toBe('Release Packages (@release-packages) · Release · Internal org');
  });

  test('allows all repository-safe visibilities for public org repositories', () => {
    expect(
      getAllowedPackageVisibilityOptions('public', {
        repositoryIsOrgOwned: true,
      })
    ).toEqual([
      { value: 'public', label: 'Public' },
      { value: 'private', label: 'Private' },
      { value: 'internal_org', label: 'Internal org' },
      { value: 'unlisted', label: 'Unlisted' },
      { value: 'quarantined', label: 'Quarantined' },
    ]);
  });

  test('narrows visibility options for unlisted, private, and quarantined repositories', () => {
    expect(
      getAllowedPackageVisibilityOptions('unlisted', {
        repositoryIsOrgOwned: true,
      })
    ).toEqual([
      { value: 'private', label: 'Private' },
      { value: 'internal_org', label: 'Internal org' },
      { value: 'unlisted', label: 'Unlisted' },
      { value: 'quarantined', label: 'Quarantined' },
    ]);

    expect(
      getAllowedPackageVisibilityOptions('private', {
        repositoryIsOrgOwned: true,
      })
    ).toEqual([
      { value: 'private', label: 'Private' },
      { value: 'internal_org', label: 'Internal org' },
      { value: 'quarantined', label: 'Quarantined' },
    ]);

    expect(
      getAllowedPackageVisibilityOptions('quarantined', {
        repositoryIsOrgOwned: true,
      })
    ).toEqual([{ value: 'quarantined', label: 'Quarantined' }]);
  });

  test('drops internal org visibility for non-organization-owned repositories', () => {
    expect(
      getAllowedPackageVisibilityOptions('public', {
        repositoryIsOrgOwned: false,
      }).map((option) => option.value)
    ).toEqual(['public', 'private', 'unlisted', 'quarantined']);
  });

  test('recognizes the supported repository kinds for direct package creation', () => {
    expect(isRepositoryEligibleForPackageCreation('public')).toBe(true);
    expect(isRepositoryEligibleForPackageCreation('release')).toBe(true);
    expect(isRepositoryEligibleForPackageCreation('proxy')).toBe(false);
    expect(isRepositoryEligibleForPackageCreation('virtual')).toBe(false);
  });
});
