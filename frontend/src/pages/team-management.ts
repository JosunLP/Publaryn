import type { NamespaceClaim } from '../api/namespaces';
import type {
  OrgMember,
  OrgPackageSummary,
  OrgRepositorySummary,
  TeamMember,
} from '../api/orgs';
import type { OrgMemberPickerOption } from './org-member-picker';
import { buildOrgMemberPickerOptions } from './org-member-picker';
import { renderPackageSelectionValue } from './org-workspace-actions';
import { sortNamespaceClaims } from './personal-namespaces';
import { ecosystemLabel } from '../utils/ecosystem';
import {
  formatRepositoryKindLabel,
  formatRepositoryVisibilityLabel,
} from '../utils/repositories';

export interface TeamAccessGrantTargetOption {
  value: string;
  label: string;
}

export interface TeamAccessPermissionOption {
  value: string;
  label: string;
  description: string;
}

export const TEAM_PERMISSION_OPTIONS: readonly TeamAccessPermissionOption[] = [
  {
    value: 'admin',
    label: 'Admin',
    description: 'Manage package administration workflows.',
  },
  {
    value: 'publish',
    label: 'Publish',
    description: 'Create releases and publish artifacts.',
  },
  {
    value: 'write_metadata',
    label: 'Write metadata',
    description: 'Update package readmes and metadata.',
  },
  {
    value: 'read_private',
    label: 'Read private',
    description: 'Read non-public package data.',
  },
  {
    value: 'security_review',
    label: 'Security review',
    description: 'Reserved for future security workflows.',
  },
  {
    value: 'transfer_ownership',
    label: 'Transfer ownership',
    description: 'Transfer a package to another owner.',
  },
] as const;

export const TEAM_NAMESPACE_PERMISSION_OPTIONS: readonly TeamAccessPermissionOption[] =
  [
    {
      value: 'admin',
      label: 'Admin',
      description: 'Delete organization-owned namespace claims.',
    },
    {
      value: 'transfer_ownership',
      label: 'Transfer ownership',
      description:
        'Transfer a namespace claim into another controlled organization.',
    },
  ] as const;

export function buildRepositoryGrantOptions(
  repositories: OrgRepositorySummary[]
): TeamAccessGrantTargetOption[] {
  return [...repositories]
    .sort((left, right) =>
      `${left.name || left.slug || ''}`.localeCompare(
        `${right.name || right.slug || ''}`
      )
    )
    .map((repository) => ({
      value: repository.slug || '',
      label: `${repository.name || repository.slug || ''} · ${formatRepositoryKindLabel(repository.kind)} · ${formatRepositoryVisibilityLabel(repository.visibility)}`,
    }));
}

export function buildPackageGrantOptions(
  packages: OrgPackageSummary[]
): TeamAccessGrantTargetOption[] {
  return [...packages]
    .sort((left, right) =>
      `${left.ecosystem || ''}:${left.name || ''}`.localeCompare(
        `${right.ecosystem || ''}:${right.name || ''}`
      )
    )
    .map((pkg) => ({
      value: renderPackageSelectionValue(pkg.ecosystem, pkg.name),
      label: `${pkg.ecosystem || ''} · ${pkg.name || ''}`,
    }));
}

export function buildNamespaceGrantOptions(
  claims: NamespaceClaim[]
): TeamAccessGrantTargetOption[] {
  return sortNamespaceClaims(claims)
    .filter(
      (claim): claim is NamespaceClaim & { id: string } =>
        typeof claim.id === 'string' && claim.id.trim().length > 0
    )
    .map((claim) => ({
      value: claim.id,
      label: `${claim.namespace || 'Unnamed claim'} · ${ecosystemLabel(claim.ecosystem)}`,
    }));
}

export function buildEligibleTeamMemberOptions(
  orgMembers: OrgMember[],
  teamMembers: TeamMember[]
): OrgMemberPickerOption[] {
  return buildOrgMemberPickerOptions(
    orgMembers,
    teamMembers.map((member) => member.username?.trim() || '').filter(Boolean)
  );
}

export function formatTeamPermission(permission: string): string {
  return permission
    .split('_')
    .filter(Boolean)
    .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
    .join(' ');
}
