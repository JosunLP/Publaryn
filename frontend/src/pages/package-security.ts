import type { SecurityFinding } from '../api/packages';
import { severityLevel } from '../api/packages';
import {
  SECURITY_SEVERITIES,
  normalizeSecuritySeverity,
  normalizeSecuritySeverityCounts,
  type SecuritySeverity,
  type SecuritySeverityCounts,
} from '../utils/security';

export type PackageSecurityFocusMode = 'triage' | 'all' | 'resolved';

export interface PackageSecurityFilters {
  searchQuery?: string | null;
  severities?: string[] | null;
  focusMode?: PackageSecurityFocusMode | null;
}

export function normalizePackageSecuritySearchQuery(
  value: string | null | undefined
): string {
  return value?.trim() || '';
}

export function normalizePackageSecurityFocusMode(
  value: string | null | undefined
): PackageSecurityFocusMode {
  return value === 'all' || value === 'resolved' ? value : 'triage';
}

export function normalizePackageSecuritySeverityFilters(
  values: readonly string[] | null | undefined
): SecuritySeverity[] {
  const selected = new Set(
    (values || []).map((value) => normalizeSecuritySeverity(value))
  );

  return SECURITY_SEVERITIES.filter((severity) => selected.has(severity));
}

export function sortPackageSecurityFindings(
  findings: SecurityFinding[]
): SecurityFinding[] {
  return [...findings].sort((left, right) => {
    const severityDelta = severityLevel(right.severity) - severityLevel(left.severity);
    if (severityDelta !== 0) {
      return severityDelta;
    }

    const detectedAtDelta = `${right.detected_at || ''}`.localeCompare(
      `${left.detected_at || ''}`
    );
    if (detectedAtDelta !== 0) {
      return detectedAtDelta;
    }

    return `${left.title || ''}`.localeCompare(`${right.title || ''}`);
  });
}

export function filterPackageSecurityFindings(
  findings: SecurityFinding[],
  filters: PackageSecurityFilters = {}
): SecurityFinding[] {
  const searchQuery = normalizePackageSecuritySearchQuery(filters.searchQuery).toLowerCase();
  const severities = normalizePackageSecuritySeverityFilters(filters.severities);
  const focusMode = normalizePackageSecurityFocusMode(filters.focusMode);
  const allowedSeverities = new Set(severities);

  return sortPackageSecurityFindings(findings).filter((finding) => {
    if (focusMode === 'triage' && finding.is_resolved) {
      return false;
    }

    if (focusMode === 'resolved' && !finding.is_resolved) {
      return false;
    }

    if (
      allowedSeverities.size > 0 &&
      !allowedSeverities.has(normalizeSecuritySeverity(finding.severity))
    ) {
      return false;
    }

    if (!searchQuery) {
      return true;
    }

    return buildPackageSecuritySearchableText(finding).includes(searchQuery);
  });
}

export function countPackageSecurityFindingsBySeverity(
  findings: SecurityFinding[]
): SecuritySeverityCounts {
  const counts: Partial<Record<SecuritySeverity, number>> = {};

  for (const finding of findings) {
    const severity = normalizeSecuritySeverity(finding.severity);
    counts[severity] = (counts[severity] || 0) + 1;
  }

  return normalizeSecuritySeverityCounts(counts);
}

export function buildPackageSecurityFilterSummary({
  totalLoadedCount,
  visibleCount,
  includeResolvedFindings,
  filters,
}: {
  totalLoadedCount: number;
  visibleCount: number;
  includeResolvedFindings: boolean;
  filters?: PackageSecurityFilters;
}): string {
  const searchQuery = normalizePackageSecuritySearchQuery(filters?.searchQuery);
  const severities = normalizePackageSecuritySeverityFilters(filters?.severities);
  const focusMode = normalizePackageSecurityFocusMode(filters?.focusMode);
  const summaryParts = [
    `Showing ${visibleCount} of ${totalLoadedCount} loaded finding${totalLoadedCount === 1 ? '' : 's'}`,
  ];

  switch (focusMode) {
    case 'resolved':
      summaryParts.push('from resolved history');
      break;
    case 'all':
      summaryParts.push('across all loaded findings');
      break;
    default:
      summaryParts.push('in the unresolved triage queue');
      break;
  }

  if (severities.length > 0) {
    summaryParts.push(
      `filtered to ${severities.map(formatPackageSecuritySeverityLabel).join(', ')} severit${severities.length === 1 ? 'y' : 'ies'}`
    );
  }

  if (searchQuery) {
    summaryParts.push(`matching "${searchQuery}"`);
  }

  let summary = `${summaryParts.join(', ')}.`;

  if (!includeResolvedFindings && focusMode === 'resolved') {
    summary += ' Load resolved findings to review resolved history.';
  }

  return summary;
}

export function buildPackageSecurityEmptyStateMessage({
  totalLoadedCount,
  includeResolvedFindings,
  filters,
}: {
  totalLoadedCount: number;
  includeResolvedFindings: boolean;
  filters?: PackageSecurityFilters;
}): string {
  if (totalLoadedCount === 0) {
    return includeResolvedFindings
      ? 'No loaded findings are available for this package.'
      : 'No unresolved findings are currently loaded for this package.';
  }

  const searchQuery = normalizePackageSecuritySearchQuery(filters?.searchQuery);
  const severities = normalizePackageSecuritySeverityFilters(filters?.severities);
  const focusMode = normalizePackageSecurityFocusMode(filters?.focusMode);

  if (!includeResolvedFindings && focusMode === 'resolved') {
    return 'Load resolved findings to review resolved history for this package.';
  }

  const activeFilters: string[] = [];
  if (focusMode === 'triage') {
    activeFilters.push('the unresolved triage queue');
  } else if (focusMode === 'resolved') {
    activeFilters.push('resolved history');
  }
  if (severities.length > 0) {
    activeFilters.push(
      `${severities.map(formatPackageSecuritySeverityLabel).join(', ')} severit${severities.length === 1 ? 'y' : 'ies'}`
    );
  }
  if (searchQuery) {
    activeFilters.push(`matches for "${searchQuery}"`);
  }

  return activeFilters.length > 0
    ? `Try adjusting or clearing ${activeFilters.join(', ')}.`
    : 'Try loading resolved findings or clearing the current filters.';
}

function buildPackageSecuritySearchableText(finding: SecurityFinding): string {
  return [
    finding.title,
    finding.description,
    finding.advisory_id,
    finding.release_version,
    finding.artifact_filename,
    finding.kind,
    finding.severity,
  ]
    .filter((value): value is string => typeof value === 'string' && value.trim().length > 0)
    .join(' ')
    .toLowerCase();
}

function formatPackageSecuritySeverityLabel(severity: SecuritySeverity): string {
  return severity.charAt(0).toUpperCase() + severity.slice(1);
}
