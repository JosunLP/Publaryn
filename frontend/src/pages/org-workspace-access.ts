import type { OrganizationMembership } from '../api/orgs';

export function canViewOrgPeopleWorkspace(
  membership: OrganizationMembership | null | undefined
): boolean {
  return Boolean(membership?.role?.trim());
}
