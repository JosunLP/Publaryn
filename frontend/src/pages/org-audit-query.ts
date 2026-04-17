export const ORG_AUDIT_ACTION_VALUES = [
  'org_create',
  'org_update',
  'namespace_claim_create',
  'org_member_add',
  'org_role_change',
  'org_member_remove',
  'org_ownership_transfer',
  'org_invitation_create',
  'org_invitation_revoke',
  'org_invitation_accept',
  'org_invitation_decline',
  'team_create',
  'team_update',
  'team_delete',
  'team_member_add',
  'team_member_remove',
  'team_package_access_update',
] as const;

const ORG_AUDIT_ACTION_SET = new Set<string>(ORG_AUDIT_ACTION_VALUES);
const AUDIT_ACTOR_USER_ID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const AUDIT_DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

export interface OrgAuditView {
  action: string;
  actorUserId: string;
  actorUsername: string;
  occurredFrom: string;
  occurredUntil: string;
  page: number;
}

export function normalizeAuditAction(value: string | null | undefined): string {
  if (typeof value !== 'string') {
    return '';
  }

  const trimmed = value.trim();
  return trimmed && ORG_AUDIT_ACTION_SET.has(trimmed) ? trimmed : '';
}

export function normalizeAuditActorUserId(
  value: string | null | undefined
): string {
  if (typeof value !== 'string') {
    return '';
  }

  const trimmed = value.trim();
  return AUDIT_ACTOR_USER_ID_PATTERN.test(trimmed) ? trimmed.toLowerCase() : '';
}

export function normalizeAuditActorUsername(
  value: string | null | undefined
): string {
  if (typeof value !== 'string') {
    return '';
  }

  return value.trim();
}

export function normalizeAuditDateValue(
  value: string | null | undefined
): string {
  if (typeof value !== 'string') {
    return '';
  }

  const trimmed = value.trim();
  if (!AUDIT_DATE_PATTERN.test(trimmed)) {
    return '';
  }

  const parsedDate = new Date(`${trimmed}T00:00:00Z`);
  if (Number.isNaN(parsedDate.getTime())) {
    return '';
  }

  return parsedDate.toISOString().slice(0, 10) === trimmed ? trimmed : '';
}

export function formatAuditActorQueryLabel(
  actorUsername: string | null | undefined
): string {
  const normalizedUsername = normalizeAuditActorUsername(actorUsername);
  return normalizedUsername ? `@${normalizedUsername}` : 'the selected actor';
}

export function getAuditViewFromQuery(query: URLSearchParams): OrgAuditView {
  const actorUserId = normalizeAuditActorUserId(query.get('actor_user_id'));
  const parsedPage = Number.parseInt(query.get('page') ?? '1', 10);

  return {
    action: normalizeAuditAction(query.get('action')),
    actorUserId,
    actorUsername: actorUserId
      ? normalizeAuditActorUsername(query.get('actor_username'))
      : '',
    occurredFrom: normalizeAuditDateValue(query.get('occurred_from')),
    occurredUntil: normalizeAuditDateValue(query.get('occurred_until')),
    page: Number.isFinite(parsedPage) && parsedPage > 0 ? parsedPage : 1,
  };
}

export function buildOrgAuditPath(
  slug: string,
  {
    action,
    actorUserId,
    actorUsername,
    occurredFrom,
    occurredUntil,
    page,
  }: {
    action: string | null | undefined;
    actorUserId: string | null | undefined;
    actorUsername?: string | null | undefined;
    occurredFrom?: string | null | undefined;
    occurredUntil?: string | null | undefined;
    page: number;
  },
  currentSearch: string | URLSearchParams = ''
): string {
  const params =
    currentSearch instanceof URLSearchParams
      ? new URLSearchParams(currentSearch)
      : new URLSearchParams(currentSearch);
  const normalizedAction = normalizeAuditAction(action);
  const normalizedActorUserId = normalizeAuditActorUserId(actorUserId);
  const normalizedActorUsername = normalizedActorUserId
    ? normalizeAuditActorUsername(actorUsername)
    : '';
  const normalizedOccurredFrom = normalizeAuditDateValue(occurredFrom);
  const normalizedOccurredUntil = normalizeAuditDateValue(occurredUntil);

  if (normalizedAction) {
    params.set('action', normalizedAction);
  } else {
    params.delete('action');
  }

  if (normalizedActorUserId) {
    params.set('actor_user_id', normalizedActorUserId);
  } else {
    params.delete('actor_user_id');
  }

  if (normalizedActorUsername) {
    params.set('actor_username', normalizedActorUsername);
  } else {
    params.delete('actor_username');
  }

  if (normalizedOccurredFrom) {
    params.set('occurred_from', normalizedOccurredFrom);
  } else {
    params.delete('occurred_from');
  }

  if (normalizedOccurredUntil) {
    params.set('occurred_until', normalizedOccurredUntil);
  } else {
    params.delete('occurred_until');
  }

  if (page > 1) {
    params.set('page', String(page));
  } else {
    params.delete('page');
  }

  const queryString = params.toString();
  const encodedSlug = encodeURIComponent(slug);
  return queryString
    ? `/orgs/${encodedSlug}?${queryString}`
    : `/orgs/${encodedSlug}`;
}

export function buildOrgAuditExportFilename(
  slug: string,
  {
    action,
    actorUsername,
    occurredFrom,
    occurredUntil,
  }: {
    action?: string | null | undefined;
    actorUsername?: string | null | undefined;
    occurredFrom?: string | null | undefined;
    occurredUntil?: string | null | undefined;
  },
  exportedAt: Date = new Date()
): string {
  const normalizedSlug =
    normalizeAuditExportFilenamePart(slug) || 'organization';
  const normalizedAction = normalizeAuditAction(action);
  const normalizedActorUsername = normalizeAuditExportFilenamePart(
    normalizeAuditActorUsername(actorUsername)
  );
  const normalizedOccurredFrom = normalizeAuditDateValue(occurredFrom);
  const normalizedOccurredUntil = normalizeAuditDateValue(occurredUntil);
  const exportDate = Number.isNaN(exportedAt.getTime())
    ? new Date().toISOString().slice(0, 10)
    : exportedAt.toISOString().slice(0, 10);
  const parts = [`org-audit-${normalizedSlug}`];

  if (normalizedAction) {
    parts.push(normalizedAction);
  }

  if (normalizedActorUsername) {
    parts.push(`actor-${normalizedActorUsername}`);
  }

  if (normalizedOccurredFrom && normalizedOccurredUntil) {
    parts.push(`${normalizedOccurredFrom}_to_${normalizedOccurredUntil}`);
  } else if (normalizedOccurredFrom) {
    parts.push(`from_${normalizedOccurredFrom}`);
  } else if (normalizedOccurredUntil) {
    parts.push(`until_${normalizedOccurredUntil}`);
  }

  parts.push(exportDate);

  return `${parts.join('--')}.csv`;
}

function normalizeAuditExportFilenamePart(
  value: string | null | undefined
): string {
  if (typeof value !== 'string') {
    return '';
  }

  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '');
}
