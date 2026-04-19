import type { OrgInvitation } from '../api/orgs';

export type OrgInvitationStatus =
  | 'pending'
  | 'accepted'
  | 'declined'
  | 'revoked'
  | 'expired';

export interface OrgInvitationStatusCounts {
  pending: number;
  accepted: number;
  declined: number;
  revoked: number;
  expired: number;
}

export interface OrgInvitationEventDescriptor {
  label: string;
  occurredAt: string | null;
}

export function normalizeOrgInvitationStatus(
  status?: string | null
): OrgInvitationStatus {
  const normalized = stringField(status)?.toLowerCase().replace(/-/g, '_');

  switch (normalized) {
    case 'accepted':
    case 'declined':
    case 'revoked':
    case 'expired':
      return normalized;
    default:
      return 'pending';
  }
}

export function formatOrgInvitationStatusLabel(status?: string | null): string {
  switch (normalizeOrgInvitationStatus(status)) {
    case 'accepted':
      return 'Accepted';
    case 'declined':
      return 'Declined';
    case 'revoked':
      return 'Revoked';
    case 'expired':
      return 'Expired';
    default:
      return 'Pending';
  }
}

export function formatOrgInvitationInvitee(invitation: OrgInvitation): string {
  const username = stringField(invitation.invited_user?.username);
  if (username) {
    return `@${username}`;
  }

  const email = stringField(invitation.invited_user?.email);
  if (email) {
    return email;
  }

  return 'Unknown invitee';
}

export function describeOrgInvitationEvent(
  invitation: OrgInvitation
): OrgInvitationEventDescriptor | null {
  switch (normalizeOrgInvitationStatus(invitation.status)) {
    case 'accepted':
      return {
        label: 'Accepted on',
        occurredAt: stringField(invitation.accepted_at),
      };
    case 'declined':
      return {
        label: 'Declined on',
        occurredAt: stringField(invitation.declined_at),
      };
    case 'revoked':
      return {
        label: 'Revoked on',
        occurredAt: stringField(invitation.revoked_at),
      };
    case 'expired':
      return {
        label: 'Expired on',
        occurredAt: stringField(invitation.expires_at),
      };
    default:
      return {
        label: 'Expires',
        occurredAt: stringField(invitation.expires_at),
      };
  }
}

export function countOrgInvitationStatuses(
  invitations: OrgInvitation[]
): OrgInvitationStatusCounts {
  const counts: OrgInvitationStatusCounts = {
    pending: 0,
    accepted: 0,
    declined: 0,
    revoked: 0,
    expired: 0,
  };

  for (const invitation of invitations) {
    counts[normalizeOrgInvitationStatus(invitation.status)] += 1;
  }

  return counts;
}

export function partitionOrgInvitations(invitations: OrgInvitation[]): {
  active: OrgInvitation[];
  history: OrgInvitation[];
} {
  const active: OrgInvitation[] = [];
  const history: OrgInvitation[] = [];

  for (const invitation of invitations) {
    if (normalizeOrgInvitationStatus(invitation.status) === 'pending') {
      active.push(invitation);
    } else {
      history.push(invitation);
    }
  }

  return { active, history };
}

function stringField(value: unknown): string | null {
  return typeof value === 'string' && value.trim().length > 0
    ? value.trim()
    : null;
}
