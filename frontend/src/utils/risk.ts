import { titleCase } from './strings';

export type RiskBadgeSeverity = 'critical' | 'high' | 'medium' | 'low' | 'info';

export function riskBadgeSeverity(
  level: string | null | undefined
): RiskBadgeSeverity {
  switch ((level || '').toLowerCase()) {
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

export function riskLabel(level: string | null | undefined): string {
  return level ? `${titleCase(level)} risk` : 'Risk pending';
}
