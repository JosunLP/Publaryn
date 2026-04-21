import type { OrgAuditQuery, OrgSecurityQuery } from '../api/orgs';
import type { OrgAuditActorOption } from './org-audit-actors';
import { normalizeAuditActorUserId } from './org-audit-query';

export interface OrgAuditFilterSubmission {
  action: string;
  actorUserId: string;
  actorUsername: string;
  occurredFrom: string;
  occurredUntil: string;
  page: number;
}

export interface OrgSecurityFilterSubmission {
  severities: string[];
  ecosystem: string;
  packageQuery: string;
}

export interface TeamPackageAccessSubmission {
  ecosystem: string;
  name: string;
  permissions: string[];
}

export interface TeamRepositoryAccessSubmission {
  repositorySlug: string;
  permissions: string[];
}

export interface TeamNamespaceAccessSubmission {
  claimId: string;
  permissions: string[];
}

type SubmissionResult<T> = { ok: true; value: T } | { ok: false; error: string };

export function resolveAuditFilterSubmission(
  formData: FormData,
  auditActorOptions: OrgAuditActorOption[]
): SubmissionResult<OrgAuditFilterSubmission> {
  const occurredFrom = formData.get('occurred_from')?.toString().trim() || '';
  const occurredUntil = formData.get('occurred_until')?.toString().trim() || '';
  const actorQuery = formData.get('actor_query')?.toString().trim() || '';
  const normalizedActorUserId = normalizeAuditActorUserId(actorQuery) || '';
  const actorFromSelect =
    auditActorOptions.find(
      (option) =>
        option.userId === normalizedActorUserId ||
        option.username.toLowerCase() === actorQuery.toLowerCase()
    ) || null;
  const resolvedActorUserId = actorFromSelect
    ? actorFromSelect.userId
    : normalizedActorUserId;
  const resolvedActorUsername = actorFromSelect
    ? actorFromSelect.username
    : actorQuery;

  if (occurredFrom && occurredUntil && occurredFrom > occurredUntil) {
    return {
      ok: false,
      error: 'End date must be on or after the start date.',
    };
  }

  return {
    ok: true,
    value: {
      action: formData.get('action')?.toString().trim() || '',
      actorUserId: resolvedActorUserId,
      actorUsername: resolvedActorUsername,
      occurredFrom,
      occurredUntil,
      page: 1,
    },
  };
}

export function resolveSecurityFilterSubmission(
  formData: FormData
): OrgSecurityFilterSubmission {
  return {
    severities: formData
      .getAll('security_severity')
      .map((value) => value.toString().trim())
      .filter(Boolean),
    ecosystem: formData.get('security_ecosystem')?.toString().trim() || '',
    packageQuery: formData.get('security_package')?.toString().trim() || '',
  };
}

export function buildAuditExportQuery(view: {
  action: string;
  actorUserId: string;
  occurredFrom: string;
  occurredUntil: string;
}): OrgAuditQuery {
  return {
    action: view.action || undefined,
    actorUserId: view.actorUserId || undefined,
    occurredFrom: view.occurredFrom || undefined,
    occurredUntil: view.occurredUntil || undefined,
  };
}

export function buildSecurityExportQuery(view: {
  severities: string[];
  ecosystem: string;
  packageQuery: string;
}): OrgSecurityQuery {
  return {
    severities: view.severities.length > 0 ? view.severities : undefined,
    ecosystem: view.ecosystem || undefined,
    package: view.packageQuery || undefined,
  };
}

export function renderPackageSelectionValue(
  ecosystem: string | null | undefined,
  name: string | null | undefined
): string {
  return `${encodeURIComponent((ecosystem || '').trim())}:${encodeURIComponent((name || '').trim())}`;
}

export function decodePackageSelection(
  value: string
): { ecosystem: string; name: string } | null {
  const separatorIndex = value.indexOf(':');
  if (separatorIndex <= 0 || separatorIndex === value.length - 1) {
    return null;
  }

  return {
    ecosystem: decodeURIComponent(value.slice(0, separatorIndex)),
    name: decodeURIComponent(value.slice(separatorIndex + 1)),
  };
}

export function resolveTeamPackageAccessSubmission(
  formData: FormData
): SubmissionResult<TeamPackageAccessSubmission> {
  const packageKey = formData.get('package_key')?.toString().trim() || '';
  const packageTarget = decodePackageSelection(packageKey);

  if (!packageTarget) {
    return {
      ok: false,
      error: 'Select a package to manage access.',
    };
  }

  const permissions = resolveSelectedPermissions(formData);

  if (permissions.length === 0) {
    return {
      ok: false,
      error: 'Select at least one delegated package permission.',
    };
  }

  return {
    ok: true,
    value: {
      ecosystem: packageTarget.ecosystem,
      name: packageTarget.name,
      permissions,
    },
  };
}

export function resolveTeamRepositoryAccessSubmission(
  formData: FormData
): SubmissionResult<TeamRepositoryAccessSubmission> {
  const repositorySlug =
    formData.get('repository_slug')?.toString().trim() || '';

  if (!repositorySlug) {
    return {
      ok: false,
      error: 'Select a repository to manage access.',
    };
  }

  const permissions = resolveSelectedPermissions(formData);

  if (permissions.length === 0) {
    return {
      ok: false,
      error: 'Select at least one delegated repository permission.',
    };
  }

  return {
    ok: true,
    value: {
      repositorySlug,
      permissions,
    },
  };
}

export function resolveTeamNamespaceAccessSubmission(
  formData: FormData
): SubmissionResult<TeamNamespaceAccessSubmission> {
  const claimId = formData.get('claim_id')?.toString().trim() || '';

  if (!claimId) {
    return {
      ok: false,
      error: 'Select a namespace claim to manage access.',
    };
  }

  const permissions = resolveSelectedPermissions(formData);

  if (permissions.length === 0) {
    return {
      ok: false,
      error: 'Select at least one delegated namespace permission.',
    };
  }

  return {
    ok: true,
    value: {
      claimId,
      permissions,
    },
  };
}

function resolveSelectedPermissions(formData: FormData): string[] {
  return formData
    .getAll('permissions')
    .map((value) => value.toString().trim())
    .filter(Boolean);
}
