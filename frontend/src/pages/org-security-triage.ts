import type { SecurityFinding } from '../api/packages';
import { severityLevel } from '../api/packages';

export function buildOrgSecurityPackageKey(
  ecosystem?: string | null,
  name?: string | null
): string {
  return `${(ecosystem || '').trim().toLowerCase()}::${(name || '').trim()}`;
}

export function sortOrgSecurityFindings(
  findings: SecurityFinding[]
): SecurityFinding[] {
  return [...findings].sort((left, right) => {
    const severityDelta = severityLevel(right.severity) - severityLevel(left.severity);
    if (severityDelta !== 0) {
      return severityDelta;
    }

    return `${right.detected_at || ''}`.localeCompare(`${left.detected_at || ''}`);
  });
}

export function mergeUpdatedOrgSecurityFinding(
  findings: SecurityFinding[],
  updated: SecurityFinding,
  options: { includeResolved?: boolean } = {}
): SecurityFinding[] {
  const { includeResolved = true } = options;
  const nextFindings = findings.some((finding) => finding.id === updated.id)
    ? findings.map((finding) =>
        finding.id === updated.id ? { ...finding, ...updated } : finding
      )
    : [updated, ...findings];

  return sortOrgSecurityFindings(
    includeResolved
      ? nextFindings
      : nextFindings.filter((finding) => !finding.is_resolved)
  );
}
