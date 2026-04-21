import type { NamespaceClaim } from '../api/namespaces';

type NullableString = string | null | undefined;

interface NamespaceTransferOrganizationLike {
  slug?: NullableString;
  name?: NullableString;
  role?: NullableString;
}

const NAMESPACE_TRANSFER_ADMIN_ROLES = new Set(['owner', 'admin']);

export function sortNamespaceClaims(
  claims: NamespaceClaim[] | null | undefined
): NamespaceClaim[] {
  return [...(claims || [])].sort((left, right) =>
    `${left.ecosystem || ''}:${left.namespace || ''}`.localeCompare(
      `${right.ecosystem || ''}:${right.namespace || ''}`
    )
  );
}

export function formatNamespaceClaimStatusLabel(
  claim: NamespaceClaim | null | undefined
): string {
  return claim?.is_verified ? 'Verified' : 'Pending verification';
}

export function selectNamespaceTransferTargets<
  T extends NamespaceTransferOrganizationLike,
>(organizations: T[], currentOwnerOrgSlug?: NullableString): T[] {
  const normalizedOwnerSlug = normalizeSlug(currentOwnerOrgSlug);

  return [...organizations]
    .filter((organization) => {
      const slug = normalizeSlug(organization.slug);
      const role = normalizeRole(organization.role);

      return (
        Boolean(slug) &&
        NAMESPACE_TRANSFER_ADMIN_ROLES.has(role) &&
        slug !== normalizedOwnerSlug
      );
    })
    .sort((left, right) => {
      const leftLabel = (left.name || left.slug || '').toLowerCase();
      const rightLabel = (right.name || right.slug || '').toLowerCase();
      return leftLabel.localeCompare(rightLabel);
    });
}

function normalizeSlug(value: NullableString): string {
  return normalizeText(value).toLowerCase();
}

function normalizeRole(value: NullableString): string {
  return normalizeText(value).toLowerCase();
}

function normalizeText(value: NullableString): string {
  return typeof value === 'string' ? value.trim() : '';
}
