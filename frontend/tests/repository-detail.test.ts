/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import type { RepositoryDetail } from '../src/api/repositories';
import { deriveRepositoryDetailCapabilities } from '../src/utils/repository-detail';

describe('repository detail capabilities', () => {
  test('allows org-owned repositories to expose internal org package visibility', () => {
    const capabilities = deriveRepositoryDetailCapabilities({
      kind: 'public',
      visibility: 'public',
      owner_org_id: '123e4567-e89b-42d3-a456-426614174000',
      can_manage: true,
      can_create_packages: true,
    } satisfies RepositoryDetail);

    expect(capabilities.canManage).toBe(true);
    expect(capabilities.canCreatePackages).toBe(true);
    expect(capabilities.packageCreationEligible).toBe(true);
    expect(
      capabilities.packageVisibilityOptions.map((option) => option.value)
    ).toEqual(['public', 'private', 'internal_org', 'unlisted', 'quarantined']);
  });

  test('drops internal org visibility for user-owned repositories', () => {
    const capabilities = deriveRepositoryDetailCapabilities({
      kind: 'public',
      visibility: 'public',
      owner_org_id: null,
      can_manage: true,
      can_create_packages: true,
    } satisfies RepositoryDetail);

    expect(capabilities.repositoryIsOrgOwned).toBe(false);
    expect(
      capabilities.packageVisibilityOptions.map((option) => option.value)
    ).toEqual(['public', 'private', 'unlisted', 'quarantined']);
  });

  test('reports package-scope limitations separately from repository management', () => {
    const capabilities = deriveRepositoryDetailCapabilities({
      kind: 'release',
      visibility: 'private',
      owner_org_id: '123e4567-e89b-42d3-a456-426614174000',
      can_manage: true,
      can_create_packages: false,
    } satisfies RepositoryDetail);

    expect(capabilities.showPackageCreationSection).toBe(true);
    expect(capabilities.canManage).toBe(true);
    expect(capabilities.canCreatePackages).toBe(false);
    expect(capabilities.packageCreationMessage).toContain('packages:write');
  });

  test('rejects direct package creation on proxy and virtual repositories', () => {
    const capabilities = deriveRepositoryDetailCapabilities({
      kind: 'proxy',
      visibility: 'public',
      owner_org_id: '123e4567-e89b-42d3-a456-426614174000',
      can_manage: true,
      can_create_packages: true,
    } satisfies RepositoryDetail);

    expect(capabilities.packageCreationEligible).toBe(false);
    expect(capabilities.packageCreationMessage).toBe(
      'Proxy repositories do not support direct package creation.'
    );
    expect(capabilities.packageVisibilityOptions).toEqual([]);
  });

  test('hides package creation entirely for viewers without capabilities', () => {
    const capabilities = deriveRepositoryDetailCapabilities({
      kind: 'public',
      visibility: 'public',
      owner_org_id: '123e4567-e89b-42d3-a456-426614174000',
      can_manage: false,
      can_create_packages: false,
    } satisfies RepositoryDetail);

    expect(capabilities.showPackageCreationSection).toBe(false);
    expect(capabilities.packageCreationMessage).toBeNull();
  });
});
