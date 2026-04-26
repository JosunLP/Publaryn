import { describe, expect, test } from 'bun:test';

import type { OrgAuditLog } from '../src/api/orgs';
import {
  formatAuditActionLabel,
  formatAuditSummary,
  formatAuditTarget,
} from '../src/pages/org-audit-format';

function makeLog(
  action: string,
  metadata: Record<string, unknown> = {},
  overrides: Partial<OrgAuditLog> = {}
): OrgAuditLog {
  return {
    id: 'audit-log-1',
    action,
    actor_user_id: '123e4567-e89b-42d3-a456-426614174000',
    actor_username: 'alice',
    actor_display_name: 'Alice Example',
    actor_token_id: null,
    target_user_id: null,
    target_username: null,
    target_display_name: null,
    target_org_id: '123e4567-e89b-42d3-a456-426614174001',
    target_package_id: null,
    target_release_id: null,
    metadata,
    occurred_at: '2026-04-19T12:00:00Z',
    ...overrides,
  };
}

describe('org audit formatting helpers', () => {
  test('keeps human-readable labels for governance actions', () => {
    expect(formatAuditActionLabel('team_member_add')).toBe('Team member added');
    expect(formatAuditActionLabel('org_invitation_decline')).toBe(
      'Invitation declined'
    );
    expect(formatAuditActionLabel('namespace_claim_delete')).toBe(
      'Namespace claim deleted'
    );
    expect(formatAuditActionLabel('namespace_claim_transfer')).toBe(
      'Namespace claim transferred'
    );
    expect(formatAuditActionLabel('team_namespace_access_update')).toBe(
      'Namespace access updated'
    );
  });

  test('formats team creation summaries', () => {
    expect(
      formatAuditSummary(
        makeLog('team_create', {
          team_name: 'Release Engineering',
          team_slug: 'release-engineering',
          description: 'Owns package publication workflows',
        })
      )
    ).toBe(
      'Created team Release Engineering (Owns package publication workflows).'
    );
  });

  test('formats team update summaries for rename and description changes', () => {
    expect(
      formatAuditSummary(
        makeLog('team_update', {
          team_slug: 'release-engineering',
          previous_name: 'Release Engineering',
          previous_description: 'Owns package publication workflows',
          name: 'Release Operations',
          description: 'Coordinates releases and publication',
        })
      )
    ).toBe(
      'Updated team Release Operations: renamed from Release Engineering, updated the description.'
    );
  });

  test('formats team deletion summaries with cleanup counts', () => {
    expect(
      formatAuditSummary(
        makeLog('team_delete', {
          team_name: 'Release Operations',
          removed_member_count: 2,
          removed_package_access_count: 1,
          removed_repository_access_count: 3,
        })
      )
    ).toBe(
      'Deleted team Release Operations and removed 2 members, 1 package access grant, and 3 repository access grants.'
    );
  });

  test('formats team membership summaries and targets', () => {
    const addLog = makeLog('team_member_add', {
      username: 'bob',
      team_name: 'Release Operations',
    });
    const removeLog = makeLog('team_member_remove', {
      username: 'bob',
      team_name: 'Release Operations',
    });

    expect(formatAuditSummary(addLog)).toBe(
      'Added @bob to team Release Operations.'
    );
    expect(formatAuditSummary(removeLog)).toBe(
      'Removed @bob from team Release Operations.'
    );
    expect(
      formatAuditTarget(
        makeLog(
          'org_invitation_revoke',
          { role: 'viewer' },
          { target_username: 'dana' }
        )
      )
    ).toBe('target @dana');
  });

  test('formats invitation revoke summaries with the invitee and role', () => {
    expect(
      formatAuditSummary(
        makeLog(
          'org_invitation_revoke',
          { role: 'security_manager', invitation_id: 'inv-1' },
          { target_username: 'dana' }
        )
      )
    ).toBe('Revoked a security manager invitation for @dana.');
  });

  test('formats invitation accept and decline summaries', () => {
    expect(
      formatAuditSummary(
        makeLog('org_invitation_accept', {
          role: 'maintainer',
          org_name: 'Acme Corp',
          org_slug: 'acme-corp',
          invitation_id: 'inv-2',
        })
      )
    ).toBe('Accepted a maintainer invitation to Acme Corp.');

    expect(
      formatAuditSummary(
        makeLog('org_invitation_decline', {
          role: 'viewer',
          invitation_id: 'inv-3',
        })
      )
    ).toBe('Declined a viewer invitation.');
  });

  test('keeps package transfer summaries as a regression', () => {
    expect(
      formatAuditSummary(
        makeLog('package_transfer', {
          name: 'acme-widget',
          new_owner_org_name: 'Acme Platform',
        })
      )
    ).toBe('Transferred package acme-widget to organization Acme Platform.');
  });

  test('formats package visibility change summaries', () => {
    const log = makeLog('package_visibility_change', {
      ecosystem: 'npm',
      package_name: 'acme-widget',
      previous_visibility: 'private',
      visibility: 'unlisted',
    });

    expect(formatAuditActionLabel('package_visibility_change')).toBe(
      'Package visibility changed'
    );
    expect(formatAuditTarget(log)).toBe('package npm · acme-widget');
    expect(formatAuditSummary(log)).toBe(
      'Changed package visibility for acme-widget from Private to Unlisted.'
    );
  });

  test('formats namespace deletion summaries and targets', () => {
    const log = makeLog('namespace_claim_delete', {
      ecosystem: 'npm',
      namespace: '@acme',
    });

    expect(formatAuditTarget(log)).toBe('namespace npm · @acme');
    expect(formatAuditSummary(log)).toBe('Deleted namespace @acme.');
  });

  test('formats namespace transfer summaries as a regression', () => {
    expect(
      formatAuditSummary(
        makeLog('namespace_claim_transfer', {
          ecosystem: 'npm',
          namespace: '@acme',
          new_owner_org_name: 'Acme Platform',
        })
      )
    ).toBe('Transferred namespace @acme to organization Acme Platform.');
  });

  test('formats namespace delegation summaries as a regression', () => {
    expect(
      formatAuditSummary(
        makeLog('team_namespace_access_update', {
          namespace: '@acme',
          permissions: ['admin', 'transfer_ownership'],
        })
      )
    ).toBe('Updated namespace access for @acme: Admin, Transfer Ownership.');
  });
});
