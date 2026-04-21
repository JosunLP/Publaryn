import type { OrganizationDetail } from '../api/orgs';

function resolveCapabilities(source: OrganizationDetail | null | undefined) {
  return source?.capabilities;
}

export function canManageOrgWorkspace(
  source: OrganizationDetail | null | undefined
): boolean {
  return resolveCapabilities(source)?.can_manage === true;
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
