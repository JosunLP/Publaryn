import type { OrgAccessHistoryEntry } from '../api/orgs';
import { titleCase } from '../utils/strings';

const EVENT_LABELS: Record<string, string> = {
  granted: 'Granted',
  updated: 'Updated',
  revoked: 'Revoked',
};

const SCOPE_LABELS: Record<string, string> = {
  package: 'Package access',
  repository: 'Repository access',
  namespace: 'Namespace access',
};

export function formatAccessHistoryScope(scope?: string | null): string {
  const normalized = normalizeIdentifier(scope);
  return SCOPE_LABELS[normalized] || titleCase(normalized || 'access');
}

export function formatAccessHistoryEvent(event?: string | null): string {
  const normalized = normalizeIdentifier(event);
  return EVENT_LABELS[normalized] || titleCase(normalized || 'updated');
}

export function formatAccessHistoryTarget(
  entry: OrgAccessHistoryEntry
): string {
  const explicitLabel = entry.target_label?.trim();
  if (explicitLabel) {
    return explicitLabel;
  }

  const target = entry.target || {};
  const scope = normalizeIdentifier(entry.scope);

  if (scope === 'package') {
    const packageName = target.name?.trim() || target.normalized_name?.trim();
    const ecosystem = target.ecosystem?.trim();
    return ecosystem && packageName
      ? `${ecosystem} · ${packageName}`
      : packageName || 'selected package';
  }
  if (scope === 'repository') {
    return target.name?.trim() || target.slug?.trim() || 'selected repository';
  }
  if (scope === 'namespace') {
    const namespace = target.namespace?.trim();
    const ecosystem = target.ecosystem?.trim();
    return ecosystem && namespace
      ? `${ecosystem} · ${namespace}`
      : namespace || 'selected namespace';
  }

  return 'selected access target';
}

export function formatAccessHistoryTeam(entry: OrgAccessHistoryEntry): string {
  return entry.team_name?.trim() || entry.team_slug?.trim() || 'Unknown team';
}

export function formatAccessHistoryActor(
  entry: OrgAccessHistoryEntry
): string | null {
  const displayName = entry.actor_display_name?.trim();
  const username = entry.actor_username?.trim();

  if (displayName && username && displayName !== username) {
    return `${displayName} (@${username})`;
  }
  if (displayName) {
    return displayName;
  }
  if (username) {
    return `@${username}`;
  }

  return null;
}

export function formatPermissionList(permissions?: string[] | null): string {
  const values = normalizePermissionList(permissions);
  return values.length > 0
    ? values.map(titleCase).join(', ')
    : 'No delegated access';
}

export function formatAccessHistoryPermissionDelta(
  entry: Pick<OrgAccessHistoryEntry, 'previous_permissions' | 'permissions'>
): string {
  return `${formatPermissionList(entry.previous_permissions)} → ${formatPermissionList(entry.permissions)}`;
}

export function accessHistorySummary(entry: OrgAccessHistoryEntry): string {
  const summary = entry.summary?.trim();
  if (summary) {
    return summary;
  }

  const team = formatAccessHistoryTeam(entry);
  const target = formatAccessHistoryTarget(entry);
  const normalizedPermissions = normalizePermissionList(entry.permissions);
  const grantedPermissions =
    normalizedPermissions.length > 0
      ? `${normalizedPermissions.map(formatPermissionPhrase).join(', ')} access`
      : 'delegated access';
  const event = normalizeIdentifier(entry.event);

  switch (event) {
    case 'granted':
      return `Granted ${team} ${grantedPermissions} to ${target}.`;
    case 'revoked':
      return `Revoked delegated access from ${team} for ${target}.`;
    default:
      return `Changed ${team} access for ${target}.`;
  }
}

export function buildOrgAccessHistoryExportFilename(
  slug: string,
  exportedAt: Date
): string {
  const datePart = exportedAt.toISOString().slice(0, 10);
  const safeSlug = sanitizeExportFilenameSlug(slug);
  return `org-access-history-${safeSlug}-${datePart}.csv`;
}

function sanitizeExportFilenameSlug(slug?: string | null): string {
  const normalized = (slug || '')
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^[-_]+|[-_]+$/g, '');

  return normalized || 'organization';
}

function normalizePermissionList(permissions?: string[] | null): string[] {
  return Array.from(
    new Set(
      (permissions || []).map((permission) => permission.trim()).filter(Boolean)
    )
  ).sort((left, right) => left.localeCompare(right));
}

function formatPermissionPhrase(permission: string): string {
  return normalizeIdentifier(permission).replace(/_/g, ' ');
}

function normalizeIdentifier(value?: string | null): string {
  return value?.trim().toLowerCase().replace(/-/g, '_') || '';
}
