/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import {
  buildEligibleTeamMemberOptions,
  buildNamespaceGrantOptions,
  buildPackageGrantOptions,
  buildRepositoryGrantOptions,
  formatTeamPermission,
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
});
