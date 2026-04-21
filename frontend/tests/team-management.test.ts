/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import {
  buildEligibleTeamMemberOptions,
  buildNamespaceGrantOptions,
  buildPackageGrantOptions,
  buildRepositoryGrantOptions,
  createTeamManagementController,
  formatTeamPermission,
  loadSingleTeamManagementState,
  loadTeamManagementStateMaps,
} from '../src/pages/team-management';

describe('team management helpers', () => {
  test('builds sorted delegated access options and formatted permission labels', () => {
    expect(
      buildRepositoryGrantOptions([
        {
          slug: 'repo-b',
          name: 'Repository B',
          kind: 'release',
          visibility: 'private',
        },
        {
          slug: 'repo-a',
          name: 'Repository A',
          kind: 'release',
          visibility: 'private',
        },
      ]).map((option) => option.value)
    ).toEqual(['repo-a', 'repo-b']);

    expect(
      buildPackageGrantOptions([
        { ecosystem: 'npm', name: 'zeta' },
        { ecosystem: 'cargo', name: 'alpha' },
      ]).map((option) => option.value)
    ).toEqual(['cargo:alpha', 'npm:zeta']);

    expect(
      buildNamespaceGrantOptions([
        { id: 'claim-2', ecosystem: 'npm', namespace: '@zeta', is_verified: true },
        { id: 'claim-1', ecosystem: 'npm', namespace: '@alpha', is_verified: true },
      ]).map((option) => option.value)
    ).toEqual(['claim-1', 'claim-2']);

    expect(formatTeamPermission('transfer_ownership')).toBe('Transfer Ownership');
  });

  test('excludes current team members from eligible member picker options', () => {
    const options = buildEligibleTeamMemberOptions(
      [
        {
          user_id: 'user-1',
          username: 'owner-user',
          display_name: 'Owner User',
        },
        {
          user_id: 'user-2',
          username: 'admin-user',
          display_name: 'Admin User',
        },
      ],
      [
        {
          username: 'owner-user',
          display_name: 'Owner User',
        },
      ]
    );

    expect(options).toHaveLength(1);
    expect(options[0]).toMatchObject({
      userId: 'user-2',
      username: 'admin-user',
    });
  });

  test('loads single-team and per-team state maps with shared shaping helpers', async () => {
    const loaders = {
      async listTeamMembers(_orgSlug: string, teamSlug: string) {
        if (teamSlug === 'release-engineering') {
          return {
            members: [{ username: 'owner-user', display_name: 'Owner User' }],
            load_error: null,
          };
        }

        throw new Error(`members ${teamSlug} failed`);
      },
      async listTeamPackageAccess(_orgSlug: string, teamSlug: string) {
        return {
          package_access:
            teamSlug === 'release-engineering'
              ? [{ ecosystem: 'npm', name: 'source-package', permissions: ['publish'] }]
              : [],
          load_error: null,
        };
      },
      async listTeamRepositoryAccess() {
        return {
          repository_access: [{ slug: 'repo-alpha', permissions: ['admin'] }],
          load_error: null,
        };
      },
      async listTeamNamespaceAccess() {
        return {
          namespace_access: [{ namespace_claim_id: 'claim-1', namespace: '@source' }],
          load_error: null,
        };
      },
    };

    const singleTeamState = await loadSingleTeamManagementState(
      'source-org',
      { slug: 'release-engineering', name: 'Release Engineering' },
      {
        includeRepositoryAccess: true,
        includeNamespaceAccess: false,
        toErrorMessage: (caughtError, fallback) =>
          caughtError instanceof Error ? caughtError.message : fallback,
        loaders,
      }
    );

    expect(singleTeamState).toMatchObject({
      members: [{ username: 'owner-user' }],
      packageAccess: [{ name: 'source-package' }],
      repositoryAccess: [{ slug: 'repo-alpha' }],
      namespaceAccess: [],
      namespaceAccessError: null,
    });

    const stateMaps = await loadTeamManagementStateMaps(
      'source-org',
      [
        { slug: 'release-engineering', name: 'Release Engineering' },
        { slug: 'security', name: 'Security' },
        { name: 'Missing Slug' },
      ],
      {
        includeMembers: true,
        includePackageAccess: true,
        includeRepositoryAccess: false,
        includeNamespaceAccess: true,
        toErrorMessage: (caughtError, fallback) =>
          caughtError instanceof Error ? caughtError.message : fallback,
        loaders,
      }
    );

    expect(Object.keys(stateMaps.teamMembersBySlug)).toEqual([
      'release-engineering',
      'security',
    ]);
    expect(stateMaps.teamMembersBySlug.security.load_error).toBe('members security failed');
    expect(stateMaps.teamPackageAccessBySlug['release-engineering'].grants).toHaveLength(1);
    expect(stateMaps.teamRepositoryAccessBySlug).toEqual({});
    expect(stateMaps.teamNamespaceAccessBySlug.security.grants).toHaveLength(1);
  });

  test('controller centralizes team mutation flows and reload messaging', async () => {
    const reloadCalls: Array<{ notice?: string | null; error?: string | null } | undefined> =
      [];
    const mutationCalls: Array<{ name: string; args: unknown[] }> = [];

    const controller = createTeamManagementController({
      getOrgSlug: () => 'source-org',
      reload: async (options) => {
        reloadCalls.push(options);
      },
      resolveEligibleTeamMemberOptions: () => [
        {
          userId: 'user-2',
          username: 'admin-user',
          label: 'Admin User',
        },
      ],
      toErrorMessage: (caughtError, fallback) =>
        caughtError instanceof Error ? caughtError.message : fallback,
      mutations: {
        async updateTeam(...args) {
          mutationCalls.push({ name: 'updateTeam', args });
        },
        async addTeamMember(...args) {
          mutationCalls.push({ name: 'addTeamMember', args });
        },
        async removeTeamMember(...args) {
          mutationCalls.push({ name: 'removeTeamMember', args });
        },
        async replaceTeamPackageAccess(...args) {
          mutationCalls.push({ name: 'replaceTeamPackageAccess', args });
        },
        async removeTeamPackageAccess(...args) {
          mutationCalls.push({ name: 'removeTeamPackageAccess', args });
        },
        async replaceTeamRepositoryAccess(...args) {
          mutationCalls.push({ name: 'replaceTeamRepositoryAccess', args });
        },
        async removeTeamRepositoryAccess(...args) {
          mutationCalls.push({ name: 'removeTeamRepositoryAccess', args });
        },
        async replaceTeamNamespaceAccess(...args) {
          mutationCalls.push({ name: 'replaceTeamNamespaceAccess', args });
          return {
            namespace_claim: {
              namespace: '@target',
            },
          };
        },
        async removeTeamNamespaceAccess(...args) {
          mutationCalls.push({ name: 'removeTeamNamespaceAccess', args });
        },
      },
    });

    const teamForm = document.createElement('form');
    teamForm.innerHTML =
      '<input name="name" value="Platform Releases"><textarea name="description">Owns release automation.</textarea>';
    await controller.updateTeam('release-engineering', {
      preventDefault() {},
      currentTarget: teamForm,
    } as SubmitEvent);

    const memberForm = document.createElement('form');
    memberForm.innerHTML = '<input name="username" value="admin-user">';
    let resetCalled = false;
    memberForm.reset = () => {
      resetCalled = true;
    };
    await controller.addTeamMember('release-engineering', {
      preventDefault() {},
      currentTarget: memberForm,
    } as SubmitEvent);

    const packageForm = document.createElement('form');
    packageForm.innerHTML = `
      <select name="package_key">
        <option value="npm:new-package" selected>npm:new-package</option>
      </select>
      <input type="checkbox" name="permissions" value="publish" checked>
    `;
    await controller.replaceTeamPackageAccess('release-engineering', {
      preventDefault() {},
      currentTarget: packageForm,
    } as SubmitEvent);

    const repositoryForm = document.createElement('form');
    repositoryForm.innerHTML = `
      <select name="repository_slug">
        <option value="repo-beta" selected>repo-beta</option>
      </select>
      <input type="checkbox" name="permissions" value="admin" checked>
    `;
    await controller.replaceTeamRepositoryAccess('release-engineering', {
      preventDefault() {},
      currentTarget: repositoryForm,
    } as SubmitEvent);

    const namespaceForm = document.createElement('form');
    namespaceForm.innerHTML = `
      <select name="claim_id">
        <option value="claim-2" selected>claim-2</option>
      </select>
      <input type="checkbox" name="permissions" value="transfer_ownership" checked>
    `;
    await controller.replaceTeamNamespaceAccess('release-engineering', {
      preventDefault() {},
      currentTarget: namespaceForm,
    } as SubmitEvent);

    await controller.removeTeamMember('release-engineering', 'admin-user');
    await controller.removeTeamPackageAccess(
      'release-engineering',
      'npm',
      'source-package'
    );
    await controller.removeTeamRepositoryAccess('release-engineering', 'repo-alpha');
    await controller.removeTeamNamespaceAccess(
      'release-engineering',
      'claim-1',
      '@source'
    );

    expect(resetCalled).toBe(true);
    expect(mutationCalls).toEqual([
      {
        name: 'updateTeam',
        args: [
          'source-org',
          'release-engineering',
          {
            name: 'Platform Releases',
            description: 'Owns release automation.',
          },
        ],
      },
      {
        name: 'addTeamMember',
        args: ['source-org', 'release-engineering', { username: 'admin-user' }],
      },
      {
        name: 'replaceTeamPackageAccess',
        args: [
          'source-org',
          'release-engineering',
          'npm',
          'new-package',
          { permissions: ['publish'] },
        ],
      },
      {
        name: 'replaceTeamRepositoryAccess',
        args: [
          'source-org',
          'release-engineering',
          'repo-beta',
          { permissions: ['admin'] },
        ],
      },
      {
        name: 'replaceTeamNamespaceAccess',
        args: [
          'source-org',
          'release-engineering',
          'claim-2',
          { permissions: ['transfer_ownership'] },
        ],
      },
      {
        name: 'removeTeamMember',
        args: ['source-org', 'release-engineering', 'admin-user'],
      },
      {
        name: 'removeTeamPackageAccess',
        args: ['source-org', 'release-engineering', 'npm', 'source-package'],
      },
      {
        name: 'removeTeamRepositoryAccess',
        args: ['source-org', 'release-engineering', 'repo-alpha'],
      },
      {
        name: 'removeTeamNamespaceAccess',
        args: ['source-org', 'release-engineering', 'claim-1'],
      },
    ]);
    expect(reloadCalls).toEqual([
      { notice: 'Saved changes to release-engineering.' },
      { notice: 'Added a member to release-engineering.' },
      { notice: 'Saved package access for new-package.' },
      { notice: 'Saved repository access for repo-beta.' },
      { notice: 'Saved namespace access for @target.' },
      { notice: 'Removed @admin-user from release-engineering.' },
      { notice: 'Revoked package access for source-package.' },
      { notice: 'Revoked repository access for repo-alpha.' },
      { notice: 'Revoked namespace access for @source.' },
    ]);
  });
});
