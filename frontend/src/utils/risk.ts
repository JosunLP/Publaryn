import { titleCase } from './strings';

export type RiskBadgeSeverity = 'critical' | 'high' | 'medium' | 'low' | 'info';

function normalizeRiskLevel(level: string | null | undefined): string {
  switch ((level || '').trim().toLowerCase()) {
    case 'moderate':
      return 'medium';
    default:
      return (level || '').trim().toLowerCase();
  }
}

export function riskBadgeSeverity(
  level: string | null | undefined
): RiskBadgeSeverity {
  switch (normalizeRiskLevel(level)) {
    case 'critical':
      return 'critical';
    case 'high':
      return 'high';
    case 'medium':
      return 'medium';
    case 'low':
      return 'low';
    default:
      return 'info';
  }
}

export function riskLabel(level: string | null | undefined): string {
  const normalizedLevel = normalizeRiskLevel(level);
  return normalizedLevel ? `${titleCase(normalizedLevel)} risk` : 'Risk pending';
}
