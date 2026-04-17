export interface RepositoryOption {
  value: string;
  label: string;
}

export const REPOSITORY_KIND_OPTIONS: RepositoryOption[] = [
  { value: 'public', label: 'Public' },
  { value: 'private', label: 'Private' },
  { value: 'staging', label: 'Staging' },
  { value: 'release', label: 'Release' },
  { value: 'proxy', label: 'Proxy' },
  { value: 'virtual', label: 'Virtual' },
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
