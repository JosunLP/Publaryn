import type { SecuritySeverity } from '../utils/security';

import type { PackageSecurityFocusMode } from './package-security';
import {
  normalizePackageSecurityFocusMode,
  normalizePackageSecuritySearchQuery,
  normalizePackageSecuritySeverityFilters,
} from './package-security';

const PACKAGE_SECURITY_QUERY_KEYS = {
  focus: 'security_focus',
  includeResolved: 'security_include_resolved',
  search: 'security_search',
  severity: 'security_severity',
} as const;

export interface PackageSecurityQueryView {
  focusMode: PackageSecurityFocusMode;
  includeResolved: boolean;
  searchQuery: string;
  severities: SecuritySeverity[];
}

export function normalizePackageSecurityIncludeResolved(
  value: string | null | undefined
): boolean {
  if (typeof value !== 'string') {
    return false;
  }

  const normalizedValue = value.trim().toLowerCase();
  return normalizedValue === 'true' || normalizedValue === '1';
}

export function getPackageSecurityViewFromQuery(
  query: URLSearchParams
): PackageSecurityQueryView {
  return {
    focusMode: normalizePackageSecurityFocusMode(
      query.get(PACKAGE_SECURITY_QUERY_KEYS.focus)
    ),
    includeResolved: normalizePackageSecurityIncludeResolved(
      query.get(PACKAGE_SECURITY_QUERY_KEYS.includeResolved)
    ),
    searchQuery: normalizePackageSecuritySearchQuery(
      query.get(PACKAGE_SECURITY_QUERY_KEYS.search)
    ),
    severities: normalizePackageSecuritySeverityFilters(
      query.getAll(PACKAGE_SECURITY_QUERY_KEYS.severity)
    ),
  };
}

export function applyPackageSecurityViewToSearchParams(
  params: URLSearchParams,
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
  }
): void {
  const normalizedFocusMode = normalizePackageSecurityFocusMode(focusMode);
  const normalizedIncludeResolved = Boolean(includeResolved);
  const normalizedSearchQuery = normalizePackageSecuritySearchQuery(searchQuery);
  const normalizedSeverities = normalizePackageSecuritySeverityFilters(
    Array.isArray(severities)
      ? severities
      : typeof severities === 'string'
        ? severities.split(',')
        : []
  );

  if (normalizedFocusMode === 'triage') {
    params.delete(PACKAGE_SECURITY_QUERY_KEYS.focus);
  } else {
    params.set(PACKAGE_SECURITY_QUERY_KEYS.focus, normalizedFocusMode);
  }

  if (normalizedIncludeResolved) {
    params.set(PACKAGE_SECURITY_QUERY_KEYS.includeResolved, 'true');
  } else {
    params.delete(PACKAGE_SECURITY_QUERY_KEYS.includeResolved);
  }

  if (normalizedSearchQuery) {
    params.set(PACKAGE_SECURITY_QUERY_KEYS.search, normalizedSearchQuery);
  } else {
    params.delete(PACKAGE_SECURITY_QUERY_KEYS.search);
  }

  if (normalizedSeverities.length > 0) {
    params.set(PACKAGE_SECURITY_QUERY_KEYS.severity, normalizedSeverities.join(','));
  } else {
    params.delete(PACKAGE_SECURITY_QUERY_KEYS.severity);
  }
}
