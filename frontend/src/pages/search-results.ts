import type { SearchPackage } from '../api/packages';
import { titleCase } from '../utils/strings';

function normalizeSearchResultValue(value: string | null | undefined): string {
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

export function searchResultRiskBadgeSeverity(
  result: Pick<SearchPackage, 'discovery'>
): 'critical' | 'high' | 'medium' | 'low' | 'info' {
  switch ((result.discovery?.risk_level || '').toLowerCase()) {
    case 'critical':
      return 'critical';
    case 'high':
      return 'high';
    case 'moderate':
      return 'medium';
    case 'low':
      return 'low';
    default:
      return 'info';
  }
}

export function searchResultRiskLabel(
  result: Pick<SearchPackage, 'discovery'>
): string {
  const level = normalizeSearchResultValue(result.discovery?.risk_level);
  return level ? `${titleCase(level)} risk` : 'Risk pending';
}

export function searchResultDiscoverySignals(
  result: Pick<SearchPackage, 'discovery'>
): string[] {
  return (result.discovery?.signals || []).filter(
    (signal): signal is string =>
      typeof signal === 'string' && signal.trim().length > 0
  );
}
