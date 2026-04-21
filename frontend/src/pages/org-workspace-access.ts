import type { OrganizationDetail } from '../api/orgs';

function resolveCapabilities(source: OrganizationDetail | null | undefined) {
  return source?.capabilities;
}

export function canManageOrgWorkspace(
  source: OrganizationDetail | null | undefined
): boolean {
  return resolveCapabilities(source)?.can_manage === true;
}

export function canManageOrgInvitations(
  source: OrganizationDetail | null | undefined
): boolean {
  return resolveCapabilities(source)?.can_manage_invitations === true;
}

export function canManageOrgMembers(
  source: OrganizationDetail | null | undefined
): boolean {
  return resolveCapabilities(source)?.can_manage_members === true;
}

export function canManageOrgTeams(
  source: OrganizationDetail | null | undefined
): boolean {
  return resolveCapabilities(source)?.can_manage_teams === true;
}

export function canManageOrgRepositories(
  source: OrganizationDetail | null | undefined
): boolean {
  return resolveCapabilities(source)?.can_manage_repositories === true;
}

export function canManageOrgNamespaces(
  source: OrganizationDetail | null | undefined
): boolean {
  return resolveCapabilities(source)?.can_manage_namespaces === true;
}

export function canViewOrgPeopleWorkspace(
  source: OrganizationDetail | null | undefined
): boolean {
  return resolveCapabilities(source)?.can_view_member_directory === true;
}

export function canViewOrgAuditWorkspace(
  source: OrganizationDetail | null | undefined
): boolean {
  return resolveCapabilities(source)?.can_view_audit_log === true;
}

export function canTransferOrgOwnership(
  source: OrganizationDetail | null | undefined
): boolean {
  return resolveCapabilities(source)?.can_transfer_ownership === true;
}
