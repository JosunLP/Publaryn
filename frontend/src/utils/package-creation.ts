import type { OrgRepositorySummary } from '../api/orgs';

import {
  REPOSITORY_VISIBILITY_OPTIONS,
  formatRepositoryKindLabel,
  formatRepositoryVisibilityLabel,
  type RepositoryOption,
} from './repositories';

const CREATABLE_REPOSITORY_KINDS = new Set([
  'public',
  'private',
  'staging',
  'release',
]);

const VISIBILITY_SCOPE_RANKS: Record<string, number> = {
  public: 2,
  unlisted: 1,
  private: 0,
  internal_org: 0,
  quarantined: 0,
};

export function isRepositoryEligibleForPackageCreation(
  kind: string | null | undefined
): boolean {
  const normalizedKind = normalizeValue(kind);
  return normalizedKind
    ? CREATABLE_REPOSITORY_KINDS.has(normalizedKind)
    : false;
}

export function selectCreatableRepositories(
  repositories: OrgRepositorySummary[]
): Array<OrgRepositorySummary & { slug: string }> {
  return repositories
    .filter(hasRepositorySlug)
    .filter((repository) =>
      isRepositoryEligibleForPackageCreation(repository.kind)
    )
    .sort((left, right) => {
      const leftKey = `${left.name || left.slug}`.toLowerCase();
      const rightKey = `${right.name || right.slug}`.toLowerCase();
      return leftKey.localeCompare(rightKey);
    });
}

export function formatPackageCreationRepositoryLabel(
  repository: Pick<
    OrgRepositorySummary,
    'name' | 'slug' | 'kind' | 'visibility'
  >
): string {
  const slug = repository.slug?.trim() || 'unknown';
  const name = repository.name?.trim();
  const heading = name ? `${name} (@${slug})` : `@${slug}`;

  return `${heading} · ${formatRepositoryKindLabel(repository.kind)} · ${formatRepositoryVisibilityLabel(repository.visibility)}`;
}

export function getAllowedPackageVisibilityOptions(
  repositoryVisibility: string | null | undefined,
  { repositoryIsOrgOwned = true }: { repositoryIsOrgOwned?: boolean } = {}
): RepositoryOption[] {
  const normalizedVisibility = normalizeValue(repositoryVisibility);
  if (!normalizedVisibility) {
    return [];
  }

  if (normalizedVisibility === 'quarantined') {
    return REPOSITORY_VISIBILITY_OPTIONS.filter(
      (option) => option.value === 'quarantined'
    );
  }

  const repositoryRank = visibilityScopeRank(normalizedVisibility);
  if (repositoryRank < 0) {
    return [];
  }

  return REPOSITORY_VISIBILITY_OPTIONS.filter((option) => {
    if (!repositoryIsOrgOwned && option.value === 'internal_org') {
      return false;
    }

    return visibilityScopeRank(option.value) <= repositoryRank;
  });
}

function hasRepositorySlug(
  repository: OrgRepositorySummary
): repository is OrgRepositorySummary & { slug: string } {
  return (
    typeof repository.slug === 'string' && repository.slug.trim().length > 0
  );
}

function visibilityScopeRank(visibility: string): number {
  return VISIBILITY_SCOPE_RANKS[visibility] ?? -1;
}

function normalizeValue(value: string | null | undefined): string {
  return value?.trim().toLowerCase().replace(/-/g, '_') || '';
}
