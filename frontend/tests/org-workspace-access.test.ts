import { describe, expect, test } from 'bun:test';

import { canViewOrgPeopleWorkspace } from '../src/pages/org-workspace-access';

describe('org workspace access helpers', () => {
  test('allows org members to view people and team sections', () => {
    expect(
      canViewOrgPeopleWorkspace({
        slug: 'acme',
        role: 'viewer',
      })
    ).toBe(true);
  });

  test('hides people and team sections for visitors and unknown memberships', () => {
    expect(canViewOrgPeopleWorkspace(undefined)).toBe(false);
    expect(canViewOrgPeopleWorkspace(null)).toBe(false);
    expect(canViewOrgPeopleWorkspace({ slug: 'acme', role: '   ' })).toBe(false);
  });
});
