import type { SecurityFinding } from '../api/packages';

import { buildPackageDetailPath } from './package-detail-tabs';

export function buildPackageDetailsPath(
  ecosystem: string,
  name: string
): string {
  return buildPackageDetailPath(ecosystem, name);
}

export function buildPackageSecurityPath(
  ecosystem: string,
  name: string,
  {
    focusMode,
    includeResolved,
    searchQuery,
    severities,
  }: {
    focusMode?: string | null | undefined;
    includeResolved?: boolean | null | undefined;
    searchQuery?: string | null | undefined;
    severities?: string | readonly string[] | null | undefined;
  } = {},
  currentSearch: string | URLSearchParams = ''
): string {
  return buildPackageDetailPath(
    ecosystem,
    name,
    {
      tab: 'security',
      securityView: {
        focusMode,
        includeResolved,
        searchQuery,
        severities,
      },
    },
    currentSearch
  );
}

export function buildPackageSecurityFindingPath(
  ecosystem: string,
  name: string,
  finding: Pick<
    SecurityFinding,
    'advisory_id' | 'id' | 'is_resolved' | 'severity' | 'title'
  >,
  currentSearch: string | URLSearchParams = ''
): string {
  return buildPackageSecurityPath(
    ecosystem,
    name,
    {
      focusMode: finding.is_resolved ? 'resolved' : 'triage',
      includeResolved: finding.is_resolved,
      searchQuery: buildPackageSecurityFindingSearchQuery(finding),
      severities:
        typeof finding.severity === 'string' && finding.severity.trim()
          ? [finding.severity]
          : [],
    },
    currentSearch
  );
}

function buildPackageSecurityFindingSearchQuery(
  finding: Pick<SecurityFinding, 'advisory_id' | 'id' | 'title'>
): string {
  if (typeof finding.advisory_id === 'string' && finding.advisory_id.trim()) {
    return finding.advisory_id.trim();
  }

  if (typeof finding.title === 'string' && finding.title.trim()) {
    return finding.title.trim();
  }

  return finding.id;
}
