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

export interface OrgAuditView {
  action: string;
  actorUserId: string;
  actorUsername: string;
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
    page: Number.isFinite(parsedPage) && parsedPage > 0 ? parsedPage : 1,
  };
}

export function buildOrgAuditPath(
  slug: string,
  {
    action,
    actorUserId,
    actorUsername,
    page,
  }: {
    action: string | null | undefined;
    actorUserId: string | null | undefined;
    actorUsername?: string | null | undefined;
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
