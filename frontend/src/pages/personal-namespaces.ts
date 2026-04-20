import type { NamespaceClaim } from '../api/namespaces';

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
