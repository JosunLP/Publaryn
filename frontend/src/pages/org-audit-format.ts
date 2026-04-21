import type { OrgAuditLog } from '../api/orgs';

const ORG_AUDIT_ACTION_LABELS: Record<string, string> = {
  org_create: 'Organization created',
  org_update: 'Organization updated',
  package_update: 'Package updated',
  package_create: 'Package created',
  package_delete: 'Package archived',
  package_transfer: 'Package transferred',
  release_publish: 'Release published',
  release_yank: 'Release yanked',
  release_unyank: 'Release restored',
  release_deprecate: 'Release deprecated',
  trusted_publisher_create: 'Trusted publisher added',
  trusted_publisher_delete: 'Trusted publisher removed',
  security_finding_resolve: 'Security finding resolved',
  security_finding_reopen: 'Security finding reopened',
  namespace_claim_create: 'Namespace claim created',
  namespace_claim_transfer: 'Namespace claim transferred',
  namespace_claim_delete: 'Namespace claim deleted',
  org_member_add: 'Member added',
  org_role_change: 'Member role updated',
  org_member_remove: 'Member removed',
  org_ownership_transfer: 'Ownership transferred',
  org_invitation_create: 'Invitation sent',
  org_invitation_revoke: 'Invitation revoked',
  org_invitation_accept: 'Invitation accepted',
  org_invitation_decline: 'Invitation declined',
  team_create: 'Team created',
  team_update: 'Team updated',
  team_delete: 'Team deleted',
  team_member_add: 'Team member added',
  team_member_remove: 'Team member removed',
  team_package_access_update: 'Package access updated',
  team_repository_access_update: 'Repository access updated',
  team_namespace_access_update: 'Namespace access updated',
};

type AuditMetadata = Record<string, unknown>;

export function formatAuditActionLabel(action: string): string {
  return (
    ORG_AUDIT_ACTION_LABELS[action] ||
    formatIdentifierLabel(action || 'activity')
  );
}

export function formatAuditTarget(log: OrgAuditLog): string | null {
  const metadata = getAuditMetadata(log);
  const username =
    stringField(metadata.username) ||
    stringField(metadata.invited_username) ||
    stringField(metadata.new_owner_username) ||
    stringField(log.target_username) ||
    stringField(log.target_display_name);

  if (username) {
    return `target @${username}`;
  }

  const teamName =
    stringField(metadata.team_name) || stringField(metadata.team_slug);
  if (teamName) {
    return `team ${teamName}`;
  }

  const repositoryName =
    stringField(metadata.repository_name) ||
    stringField(metadata.repository_slug);
  if (repositoryName) {
    return `repository ${repositoryName}`;
  }

  const packageName =
    stringField(metadata.package_name) || stringField(metadata.name);
  const ecosystem = stringField(metadata.ecosystem);
  if (packageName && ecosystem) {
    return `package ${ecosystem} · ${packageName}`;
  }

  const namespace = stringField(metadata.namespace);
  if (namespace && ecosystem) {
    return `namespace ${ecosystem} · ${namespace}`;
  }

  return null;
}

export function formatAuditSummary(log: OrgAuditLog): string | null {
  const metadata = getAuditMetadata(log);

  switch (log.action) {
    case 'org_update':
      return 'Updated organization profile settings.';
    case 'package_update': {
      const changedFields = Array.isArray(metadata.changed_fields)
        ? metadata.changed_fields.filter(
            (item): item is string => typeof item === 'string'
          )
        : [];
      const packageName =
        stringField(metadata.package_name) || 'selected package';

      return changedFields.length > 0
        ? `Updated package settings for ${packageName}: ${changedFields.map((field) => formatIdentifierLabel(field)).join(', ')}.`
        : `Updated package settings for ${packageName}.`;
    }
    case 'release_publish':
    case 'release_yank':
    case 'release_unyank':
    case 'release_deprecate': {
      const packageName =
        stringField(metadata.package_name) ||
        stringField(metadata.name) ||
        'selected package';
      const version = stringField(metadata.version);
      const releaseLabel = version ? `${packageName} ${version}` : packageName;
      const reason = stringField(metadata.reason);
      const note = stringField(metadata.message);

      switch (log.action) {
        case 'release_publish':
          return `Published release ${releaseLabel}.`;
        case 'release_yank':
          return reason
            ? `Yanked release ${releaseLabel} (${reason}).`
            : `Yanked release ${releaseLabel}.`;
        case 'release_unyank':
          return `Restored release ${releaseLabel}.`;
        case 'release_deprecate':
          return note
            ? `Deprecated release ${releaseLabel} (${note}).`
            : `Deprecated release ${releaseLabel}.`;
        default:
          return null;
      }
    }
    case 'package_create': {
      const packageName =
        stringField(metadata.name) ||
        stringField(metadata.package_name) ||
        'selected package';
      const repositorySlug = stringField(metadata.repository_slug);
      return repositorySlug
        ? `Created package ${packageName} in repository ${repositorySlug}.`
        : `Created package ${packageName}.`;
    }
    case 'package_delete': {
      const packageName =
        stringField(metadata.name) ||
        stringField(metadata.package_name) ||
        'selected package';
      return `Archived package ${packageName}.`;
    }
    case 'package_transfer': {
      const packageName =
        stringField(metadata.name) ||
        stringField(metadata.package_name) ||
        'selected package';
      const newOwnerSlug = stringField(metadata.new_owner_org_slug);
      const newOwnerName = stringField(metadata.new_owner_org_name);
      const targetLabel = newOwnerName || newOwnerSlug;
      return targetLabel
        ? `Transferred package ${packageName} to organization ${targetLabel}.`
        : `Transferred package ${packageName}.`;
    }
    case 'trusted_publisher_create':
    case 'trusted_publisher_delete': {
      const issuer = stringField(metadata.issuer);
      const subject = stringField(metadata.subject);
      const repository = stringField(metadata.repository);
      const descriptor = issuer
        ? subject
          ? `${issuer} (${subject})`
          : issuer
        : 'trusted publisher';
      const repoSuffix = repository ? ` for ${repository}` : '';
      return log.action === 'trusted_publisher_create'
        ? `Added ${descriptor}${repoSuffix}.`
        : `Removed ${descriptor}${repoSuffix}.`;
    }
    case 'security_finding_resolve':
    case 'security_finding_reopen': {
      const packageName =
        stringField(metadata.package_name) || 'selected package';
      const releaseVersion = stringField(metadata.release_version);
      const note = stringField(metadata.note);
      const actionLabel =
        log.action === 'security_finding_resolve' ? 'Resolved' : 'Reopened';
      const packageLabel = releaseVersion
        ? `${packageName} ${releaseVersion}`
        : packageName;

      return note
        ? `${actionLabel} a security finding for ${packageLabel} (${note}).`
        : `${actionLabel} a security finding for ${packageLabel}.`;
    }
    case 'org_member_add':
      return stringField(metadata.username)
        ? `Granted ${formatRolePhrase(stringField(metadata.role)) || 'viewer'} to @${stringField(metadata.username) || ''}.`
        : null;
    case 'org_role_change':
      return stringField(metadata.username)
        ? `Changed @${stringField(metadata.username) || ''} to ${formatRolePhrase(stringField(metadata.role)) || 'viewer'}.`
        : null;
    case 'org_member_remove':
      return stringField(metadata.username)
        ? `Removed @${stringField(metadata.username) || ''} from the organization.`
        : null;
    case 'org_ownership_transfer':
      return stringField(metadata.new_owner_username)
        ? `Transferred ownership to @${stringField(metadata.new_owner_username) || ''}.`
        : 'Transferred organization ownership.';
    case 'namespace_claim_create':
      return stringField(metadata.namespace)
        ? `Created namespace ${stringField(metadata.namespace) || ''}.`
        : 'Created a namespace claim.';
    case 'namespace_claim_transfer': {
      const namespace = stringField(metadata.namespace);
      const newOwnerName = stringField(metadata.new_owner_org_name);
      const newOwnerSlug = stringField(metadata.new_owner_org_slug);
      const targetLabel = newOwnerName || newOwnerSlug;
      return namespace && targetLabel
        ? `Transferred namespace ${namespace} to organization ${targetLabel}.`
        : namespace
          ? `Transferred namespace ${namespace}.`
          : 'Transferred a namespace claim.';
    }
    case 'namespace_claim_delete':
      return stringField(metadata.namespace)
        ? `Deleted namespace ${stringField(metadata.namespace) || ''}.`
        : 'Deleted a namespace claim.';
    case 'org_invitation_create':
      return stringField(metadata.invited_username)
        ? `Sent an invitation to @${stringField(metadata.invited_username) || ''}.`
        : stringField(metadata.invited_email)
          ? `Sent an invitation to ${stringField(metadata.invited_email) || ''}.`
          : 'Sent an invitation.';
    case 'org_invitation_revoke': {
      const username =
        stringField(log.target_username) ||
        stringField(log.target_display_name);
      const role = formatRolePhrase(stringField(metadata.role));
      return username
        ? role
          ? `Revoked a ${role} invitation for @${username}.`
          : `Revoked an invitation for @${username}.`
        : role
          ? `Revoked a ${role} invitation.`
          : 'Revoked an invitation.';
    }
    case 'org_invitation_accept': {
      const role = formatRolePhrase(stringField(metadata.role));
      const orgName =
        stringField(metadata.org_name) || stringField(metadata.org_slug);
      return role && orgName
        ? `Accepted a ${role} invitation to ${orgName}.`
        : orgName
          ? `Accepted an invitation to ${orgName}.`
          : role
            ? `Accepted a ${role} invitation.`
            : 'Accepted an invitation.';
    }
    case 'org_invitation_decline': {
      const role = formatRolePhrase(stringField(metadata.role));
      return role
        ? `Declined a ${role} invitation.`
        : 'Declined an invitation.';
    }
    case 'team_create': {
      const teamName =
        stringField(metadata.team_name) ||
        stringField(metadata.team_slug) ||
        'selected team';
      const description = stringField(metadata.description);
      return description
        ? `Created team ${teamName} (${description}).`
        : `Created team ${teamName}.`;
    }
    case 'team_update': {
      const teamName =
        stringField(metadata.name) ||
        stringField(metadata.team_name) ||
        stringField(metadata.team_slug) ||
        'selected team';
      const previousName = stringField(metadata.previous_name);
      const description = stringField(metadata.description);
      const previousDescription = stringField(metadata.previous_description);
      const changes: string[] = [];

      if (previousName && previousName !== teamName) {
        changes.push(`renamed from ${previousName}`);
      }

      if (previousDescription !== description) {
        if (description && previousDescription) {
          changes.push('updated the description');
        } else if (description) {
          changes.push('added a description');
        } else if (previousDescription) {
          changes.push('cleared the description');
        }
      }

      return changes.length > 0
        ? `Updated team ${teamName}: ${changes.join(', ')}.`
        : `Updated team ${teamName}.`;
    }
    case 'team_delete': {
      const teamName =
        stringField(metadata.team_name) ||
        stringField(metadata.team_slug) ||
        'selected team';
      const removedItems = [
        countLabel(numberField(metadata.removed_member_count), 'member'),
        countLabel(
          numberField(metadata.removed_package_access_count),
          'package access grant'
        ),
        countLabel(
          numberField(metadata.removed_repository_access_count),
          'repository access grant'
        ),
      ].filter((value): value is string => Boolean(value));

      return removedItems.length > 0
        ? `Deleted team ${teamName} and removed ${joinWithAnd(removedItems)}.`
        : `Deleted team ${teamName}.`;
    }
    case 'team_member_add': {
      const username = stringField(metadata.username);
      const teamName =
        stringField(metadata.team_name) ||
        stringField(metadata.team_slug) ||
        'selected team';
      return username
        ? `Added @${username} to team ${teamName}.`
        : `Added a member to team ${teamName}.`;
    }
    case 'team_member_remove': {
      const username = stringField(metadata.username);
      const teamName =
        stringField(metadata.team_name) ||
        stringField(metadata.team_slug) ||
        'selected team';
      return username
        ? `Removed @${username} from team ${teamName}.`
        : `Removed a member from team ${teamName}.`;
    }
    case 'team_package_access_update': {
      const permissions = Array.isArray(metadata.permissions)
        ? metadata.permissions.filter(
            (item): item is string => typeof item === 'string'
          )
        : [];
      const packageName =
        stringField(metadata.package_name) || 'selected package';
      return permissions.length > 0
        ? `Updated delegated access for ${packageName}: ${permissions.map((permission) => formatPermission(permission)).join(', ')}.`
        : `Removed delegated access for ${packageName}.`;
    }
    case 'team_repository_access_update': {
      const permissions = Array.isArray(metadata.permissions)
        ? metadata.permissions.filter(
            (item): item is string => typeof item === 'string'
          )
        : [];
      const repositoryName =
        stringField(metadata.repository_name) ||
        stringField(metadata.repository_slug) ||
        'selected repository';
      return permissions.length > 0
        ? `Updated repository-wide access for ${repositoryName}: ${permissions.map((permission) => formatPermission(permission)).join(', ')}.`
        : `Removed repository-wide access for ${repositoryName}.`;
    }
    case 'team_namespace_access_update': {
      const permissions = Array.isArray(metadata.permissions)
        ? metadata.permissions.filter(
            (item): item is string => typeof item === 'string'
          )
        : [];
      const namespace = stringField(metadata.namespace) || 'selected namespace claim';
      return permissions.length > 0
        ? `Updated namespace access for ${namespace}: ${permissions.map((permission) => formatPermission(permission)).join(', ')}.`
        : `Removed namespace access for ${namespace}.`;
    }
    default:
      return null;
  }
}

function getAuditMetadata(log: OrgAuditLog): AuditMetadata {
  return log.metadata &&
    typeof log.metadata === 'object' &&
    !Array.isArray(log.metadata)
    ? log.metadata
    : {};
}

function stringField(value: unknown): string | null {
  return typeof value === 'string' && value.trim().length > 0
    ? value.trim()
    : null;
}

function numberField(value: unknown): number | null {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return null;
  }

  return Math.max(0, Math.trunc(value));
}

function formatIdentifierLabel(value: string): string {
  return value
    .split('_')
    .filter(Boolean)
    .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
    .join(' ');
}

function formatPermission(permission: string): string {
  return formatIdentifierLabel(permission);
}

function formatRolePhrase(role: string | null): string | null {
  return role ? formatIdentifierLabel(role).toLowerCase() : null;
}

function countLabel(
  value: number | null,
  singular: string,
  plural = `${singular}s`
): string | null {
  if (value == null || value <= 0) {
    return null;
  }

  return `${value} ${value === 1 ? singular : plural}`;
}

function joinWithAnd(values: string[]): string {
  if (values.length <= 1) {
    return values[0] || '';
  }

  if (values.length === 2) {
    return `${values[0]} and ${values[1]}`;
  }

  return `${values.slice(0, -1).join(', ')}, and ${values[values.length - 1]}`;
}
