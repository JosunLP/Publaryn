import { describe, expect, test } from 'bun:test';

import {
  canManageOrgInvitations,
  canManageOrgMembers,
  canManageOrgWorkspace,
  canTransferOrgOwnership,
  canViewOrgAuditWorkspace,
  canViewOrgPeopleWorkspace,
} from '../src/pages/org-workspace-access';

describe('org workspace access helpers', () => {
  test('reads explicit workspace capabilities from the payload', () => {
    expect(
      canViewOrgPeopleWorkspace({
        slug: 'acme',
        capabilities: {
          can_view_member_directory: true,
        },
      })
    ).toBe(true);
    expect(
      canManageOrgWorkspace({
        slug: 'acme',
        capabilities: {
          can_manage: true,
        },
      })
    ).toBe(true);
    expect(
      canManageOrgInvitations({
        slug: 'acme',
        capabilities: {
          can_manage_invitations: true,
        },
      })
    ).toBe(true);
    expect(
      canManageOrgMembers({
        slug: 'acme',
        capabilities: {
          can_manage_members: true,
        },
      })
    ).toBe(true);
    expect(
      canViewOrgAuditWorkspace({
        slug: 'acme',
        capabilities: {
          can_view_audit_log: true,
        },
      })
    ).toBe(true);
    expect(
      canTransferOrgOwnership({
        slug: 'acme',
        capabilities: {
          can_transfer_ownership: true,
        },
      })
    ).toBe(true);
  });

  test('hides people and team sections for visitors and unknown memberships', () => {
    expect(canViewOrgPeopleWorkspace(undefined)).toBe(false);
    expect(canViewOrgPeopleWorkspace(null)).toBe(false);
    expect(
      canViewOrgPeopleWorkspace({
        slug: 'acme',
        capabilities: {
          can_view_member_directory: false,
        },
      })
    ).toBe(false);
    expect(
      canManageOrgWorkspace({
        slug: 'acme',
        capabilities: {
          can_manage: false,
        },
      })
    ).toBe(false);
    expect(
      canManageOrgInvitations({
        slug: 'acme',
        capabilities: {
          can_manage_invitations: false,
        },
      })
    ).toBe(false);
    expect(
      canManageOrgMembers({
        slug: 'acme',
        capabilities: {
          can_manage_members: false,
        },
      })
    ).toBe(false);
    expect(
      canViewOrgAuditWorkspace({
        slug: 'acme',
        capabilities: {
          can_view_audit_log: false,
        },
      })
    ).toBe(false);
    expect(
      canTransferOrgOwnership({
        slug: 'acme',
        capabilities: {
          can_transfer_ownership: false,
        },
      })
    ).toBe(false);
  });
});
