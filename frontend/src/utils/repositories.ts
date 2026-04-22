export interface RepositoryOption {
  value: string;
  label: string;
}

export interface RepositoryOwnerSummary {
  label: string;
  href: string | null;
}

type NullableString = string | null | undefined;

interface RepositoryTransferOrganizationLike {
  slug?: NullableString;
  name?: NullableString;
  role?: NullableString;
}

interface TransferableRepositoryLike {
  slug?: NullableString;
  name?: NullableString;
  can_transfer?: boolean | null;
}

const REPOSITORY_TRANSFER_ADMIN_ROLES = new Set(['owner', 'admin']);

export const REPOSITORY_KIND_OPTIONS: RepositoryOption[] = [
  { value: 'public', label: 'Public' },
  { value: 'private', label: 'Private' },
  { value: 'staging', label: 'Staging' },
  { value: 'release', label: 'Release' },
];

export const REPOSITORY_VISIBILITY_OPTIONS: RepositoryOption[] = [
  { value: 'public', label: 'Public' },
  { value: 'private', label: 'Private' },
  { value: 'internal_org', label: 'Internal org' },
  { value: 'unlisted', label: 'Unlisted' },
  { value: 'quarantined', label: 'Quarantined' },
];

export function formatRepositoryKindLabel(
  value: string | null | undefined
): string {
  return findRepositoryOptionLabel(REPOSITORY_KIND_OPTIONS, value);
}

export function formatRepositoryVisibilityLabel(
  value: string | null | undefined
): string {
  return findRepositoryOptionLabel(REPOSITORY_VISIBILITY_OPTIONS, value);
}

export function formatRepositoryPackageCoverageLabel(
  loadedCount: number,
  totalCount: number | null | undefined
): string {
  const safeLoadedCount = Number.isFinite(loadedCount)
    ? Math.max(0, Math.trunc(loadedCount))
    : 0;
  const safeTotalCount =
    typeof totalCount === 'number' && Number.isFinite(totalCount)
      ? Math.max(0, Math.trunc(totalCount))
      : safeLoadedCount;

  if (safeTotalCount === 0) {
    return 'No visible packages in this repository yet.';
  }

  if (safeLoadedCount === 0) {
    return `${safeTotalCount} visible packages belong to this repository.`;
  }

  if (safeTotalCount > safeLoadedCount) {
    return `Showing ${safeLoadedCount} of ${safeTotalCount} visible packages.`;
  }

  return safeTotalCount === 1
    ? 'Showing 1 visible package.'
    : `Showing ${safeTotalCount} visible packages.`;
}

export function selectRepositoryTransferTargets<
  T extends RepositoryTransferOrganizationLike,
>(organizations: T[], currentOwnerOrgSlug?: NullableString): T[] {
  const normalizedOwnerSlug = normalizeSlug(currentOwnerOrgSlug);

  return [...organizations]
    .filter((organization) => {
      const slug = normalizeSlug(organization.slug);
      const role = normalizeRole(organization.role);

      return (
        Boolean(slug) &&
        REPOSITORY_TRANSFER_ADMIN_ROLES.has(role) &&
        slug !== normalizedOwnerSlug
      );
    })
    .sort((left, right) => {
      const leftLabel = (left.name || left.slug || '').toLowerCase();
      const rightLabel = (right.name || right.slug || '').toLowerCase();
      return leftLabel.localeCompare(rightLabel);
    });
}

export function selectTransferableRepositories<
  T extends TransferableRepositoryLike,
>(repositories: T[]): T[] {
  return [...repositories]
    .filter(
      (repository) =>
        repository.can_transfer === true &&
        Boolean(normalizeText(repository.slug))
    )
    .sort((left, right) => {
      const leftLabel = `${normalizeText(left.name) || normalizeText(left.slug)}:${normalizeText(left.slug)}`;
      const rightLabel = `${normalizeText(right.name) || normalizeText(right.slug)}:${normalizeText(right.slug)}`;
      return leftLabel.localeCompare(rightLabel);
    });
}

export function resolveRepositoryOwnerSummary({
  ownerOrgName,
  ownerOrgSlug,
  ownerUsername,
}: {
  ownerOrgName?: string | null;
  ownerOrgSlug?: string | null;
  ownerUsername?: string | null;
}): RepositoryOwnerSummary {
  const normalizedOrgSlug = ownerOrgSlug?.trim() || null;
  const normalizedOrgName = ownerOrgName?.trim() || null;
  const normalizedUsername = ownerUsername?.trim() || null;

  if (normalizedOrgSlug) {
    return {
      label: normalizedOrgName
        ? `${normalizedOrgName} (@${normalizedOrgSlug})`
        : `@${normalizedOrgSlug}`,
      href: `/orgs/${encodeURIComponent(normalizedOrgSlug)}`,
    };
  }

  if (normalizedUsername) {
    return {
      label: normalizedUsername,
      href: `/search?q=${encodeURIComponent(normalizedUsername)}`,
    };
  }

  return {
    label: 'Unknown owner',
    href: null,
  };
}

function findRepositoryOptionLabel(
  options: RepositoryOption[],
  value: string | null | undefined
): string {
  const normalizedValue = value?.trim().toLowerCase();
  if (!normalizedValue) {
    return 'Unknown';
  }

  const option = options.find(
    (candidate) => candidate.value === normalizedValue
  );
  if (option) {
    return option.label;
  }

  return normalizedValue
    .split(/[_-]+/)
    .filter(Boolean)
    .map((segment) => segment.charAt(0).toUpperCase() + segment.slice(1))
    .join(' ');
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
