import type { SearchPackage } from '../api/packages';

function normalizeSearchResultValue(
  value: string | null | undefined
): string {
  return typeof value === 'string' ? value.trim() : '';
}

export function formatSearchResultRepository(
  result: Pick<SearchPackage, 'repository_name' | 'repository_slug'>
): string {
  const repositoryName = normalizeSearchResultValue(result.repository_name);
  const repositorySlug = normalizeSearchResultValue(result.repository_slug);

  if (repositoryName && repositorySlug && repositoryName !== repositorySlug) {
    return `${repositoryName} (${repositorySlug})`;
  }

  return repositoryName || repositorySlug;
}
