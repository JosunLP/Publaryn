export const SECURITY_SEVERITIES = [
  'critical',
  'high',
  'medium',
  'low',
  'info',
] as const;

export type SecuritySeverity = (typeof SECURITY_SEVERITIES)[number];

export type SecuritySeverityCountInput =
  | Partial<Record<SecuritySeverity, number | null | undefined>>
  | null
  | undefined;

export interface SecuritySeverityCounts {
  critical: number;
  high: number;
  medium: number;
  low: number;
  info: number;
}

export function normalizeSecuritySeverity(
  value: string | null | undefined
): SecuritySeverity {
  const normalizedValue = value?.trim().toLowerCase();

  return SECURITY_SEVERITIES.includes(normalizedValue as SecuritySeverity)
    ? (normalizedValue as SecuritySeverity)
    : 'info';
}

export function securitySeverityRank(value: string | null | undefined): number {
  switch (normalizeSecuritySeverity(value)) {
    case 'critical':
      return 4;
    case 'high':
      return 3;
    case 'medium':
      return 2;
    case 'low':
      return 1;
    default:
      return 0;
  }
}

export function normalizeSecuritySeverityCounts(
  counts: SecuritySeverityCountInput
): SecuritySeverityCounts {
  return {
    critical: normalizeSecuritySeverityCount(counts?.critical),
    high: normalizeSecuritySeverityCount(counts?.high),
    medium: normalizeSecuritySeverityCount(counts?.medium),
    low: normalizeSecuritySeverityCount(counts?.low),
    info: normalizeSecuritySeverityCount(counts?.info),
  };
}

export function totalSecuritySeverityCounts(
  counts: SecuritySeverityCountInput
): number {
  const normalizedCounts = normalizeSecuritySeverityCounts(counts);

  return SECURITY_SEVERITIES.reduce(
    (total, severity) => total + normalizedCounts[severity],
    0
  );
}

export function worstSecuritySeverityFromCounts(
  counts: SecuritySeverityCountInput
): SecuritySeverity {
  const normalizedCounts = normalizeSecuritySeverityCounts(counts);

  for (const severity of SECURITY_SEVERITIES) {
    if (normalizedCounts[severity] > 0) {
      return severity;
    }
  }

  return 'info';
}

function normalizeSecuritySeverityCount(
  value: number | null | undefined
): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return 0;
  }

  return Math.max(0, Math.trunc(value));
}
