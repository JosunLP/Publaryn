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
