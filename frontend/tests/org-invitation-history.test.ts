import { describe, expect, test } from 'bun:test';

import type { OrgInvitation } from '../src/api/orgs';
import {
  countOrgInvitationStatuses,
  describeOrgInvitationEvent,
  formatOrgInvitationInvitee,
  formatOrgInvitationStatusLabel,
  normalizeOrgInvitationStatus,
  partitionOrgInvitations,
} from '../src/pages/org-invitation-history';

function makeInvitation(overrides: Partial<OrgInvitation> = {}): OrgInvitation {
  return {
    id: 'inv-1',
    invited_user: {
      id: '123e4567-e89b-42d3-a456-426614174100',
      username: 'bob',
      email: 'bob@example.test',
    },
    invited_by: {
      id: '123e4567-e89b-42d3-a456-426614174101',
      username: 'alice',
    },
    role: 'viewer',
    status: 'pending',
    created_at: '2026-04-19T12:00:00Z',
    expires_at: '2026-04-26T12:00:00Z',
    accepted_at: null,
    declined_at: null,
    revoked_at: null,
    ...overrides,
  };
}

describe('org invitation history helpers', () => {
  test('normalizes invitation statuses and falls back to pending', () => {
    expect(normalizeOrgInvitationStatus('accepted')).toBe('accepted');
    expect(normalizeOrgInvitationStatus('DECLINED')).toBe('declined');
    expect(normalizeOrgInvitationStatus('not-a-real-status')).toBe('pending');
    expect(normalizeOrgInvitationStatus(null)).toBe('pending');
  });

  test('partitions active invitations from historical ones', () => {
    const { active, history } = partitionOrgInvitations([
      makeInvitation({ id: 'inv-pending', status: 'pending' }),
      makeInvitation({ id: 'inv-accepted', status: 'accepted' }),
      makeInvitation({ id: 'inv-revoked', status: 'revoked' }),
    ]);

    expect(active.map((invitation) => invitation.id)).toEqual(['inv-pending']);
    expect(history.map((invitation) => invitation.id)).toEqual([
      'inv-accepted',
      'inv-revoked',
    ]);
  });

  test('counts invitation statuses across active and historical states', () => {
    expect(
      countOrgInvitationStatuses([
        makeInvitation({ status: 'pending' }),
        makeInvitation({ status: 'accepted' }),
        makeInvitation({ status: 'declined' }),
        makeInvitation({ status: 'revoked' }),
        makeInvitation({ status: 'expired' }),
        makeInvitation({ status: 'pending' }),
      ])
    ).toEqual({
      pending: 2,
      accepted: 1,
      declined: 1,
      revoked: 1,
      expired: 1,
    });
  });

  test('formats invitation labels and invitee fallbacks', () => {
    expect(formatOrgInvitationStatusLabel('accepted')).toBe('Accepted');
    expect(formatOrgInvitationStatusLabel('expired')).toBe('Expired');
    expect(
      formatOrgInvitationInvitee(
        makeInvitation({ invited_user: { username: 'charlie' } })
      )
    ).toBe('@charlie');
    expect(
      formatOrgInvitationInvitee(
        makeInvitation({
          invited_user: { username: null, email: 'ops@test.dev' },
        })
      )
    ).toBe('ops@test.dev');
    expect(
      formatOrgInvitationInvitee(
        makeInvitation({ invited_user: { username: null, email: null } })
      )
    ).toBe('Unknown invitee');
  });

  test('describes invitation timeline events from status-specific timestamps', () => {
    expect(describeOrgInvitationEvent(makeInvitation())).toEqual({
      label: 'Expires',
      occurredAt: '2026-04-26T12:00:00Z',
    });
    expect(
      describeOrgInvitationEvent(
        makeInvitation({
          status: 'accepted',
          accepted_at: '2026-04-20T08:15:00Z',
        })
      )
    ).toEqual({
      label: 'Accepted on',
      occurredAt: '2026-04-20T08:15:00Z',
    });
    expect(
      describeOrgInvitationEvent(
        makeInvitation({
          status: 'declined',
          declined_at: '2026-04-21T09:30:00Z',
        })
      )
    ).toEqual({
      label: 'Declined on',
      occurredAt: '2026-04-21T09:30:00Z',
    });
    expect(
      describeOrgInvitationEvent(
        makeInvitation({
          status: 'revoked',
          revoked_at: '2026-04-22T10:45:00Z',
        })
      )
    ).toEqual({
      label: 'Revoked on',
      occurredAt: '2026-04-22T10:45:00Z',
    });
    expect(
      describeOrgInvitationEvent(
        makeInvitation({
          status: 'expired',
          expires_at: '2026-04-23T11:00:00Z',
        })
      )
    ).toEqual({
      label: 'Expired on',
      occurredAt: '2026-04-23T11:00:00Z',
    });
  });
});
