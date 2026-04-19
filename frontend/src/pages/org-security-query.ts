import { SECURITY_SEVERITIES, type SecuritySeverity } from '../utils/security';

const SECURITY_SEVERITY_SET = new Set<string>(SECURITY_SEVERITIES);
const ORG_SECURITY_QUERY_KEYS = {
  severity: 'security_severity',
  ecosystem: 'security_ecosystem',
  package: 'security_package',
} as const;

export const ORG_SECURITY_ECOSYSTEM_VALUES = [
  'npm',
  'pypi',
  'cargo',
  'nuget',
  'rubygems',
  'maven',
  'composer',
  'oci',
] as const;

const ORG_SECURITY_ECOSYSTEM_SET = new Set<string>(
  ORG_SECURITY_ECOSYSTEM_VALUES
);

export interface OrgSecurityView {
  severities: SecuritySeverity[];
  ecosystem: string;
  packageQuery: string;
}

export function normalizeOrgSecuritySeverityValues(
  value: string | readonly string[] | null | undefined
): SecuritySeverity[] {
  const rawValues = Array.isArray(value)
    ? value.flatMap((entry) => entry.split(','))
    : typeof value === 'string'
      ? value.split(',')
      : [];
  const selected = new Set<SecuritySeverity>();

  for (const rawValue of rawValues) {
    const normalizedValue = rawValue.trim().toLowerCase();
    if (!SECURITY_SEVERITY_SET.has(normalizedValue)) {
      continue;
    }

    selected.add(normalizedValue as SecuritySeverity);
  }

  return SECURITY_SEVERITIES.filter((severity) => selected.has(severity));
}

export function normalizeOrgSecurityEcosystem(
  value: string | null | undefined
): string {
  if (typeof value !== 'string') {
    return '';
  }

  const normalizedValue = value.trim().toLowerCase();
  if (!normalizedValue) {
    return '';
  }

  if (normalizedValue === 'bun') {
    return 'npm';
  }

  return ORG_SECURITY_ECOSYSTEM_SET.has(normalizedValue) ? normalizedValue : '';
}

export function normalizeOrgSecurityPackageQuery(
  value: string | null | undefined
): string {
  return typeof value === 'string' ? value.trim() : '';
}

export function getOrgSecurityViewFromQuery(
  query: URLSearchParams
): OrgSecurityView {
  return {
    severities: normalizeOrgSecuritySeverityValues(
      query.getAll(ORG_SECURITY_QUERY_KEYS.severity)
    ),
    ecosystem: normalizeOrgSecurityEcosystem(
      query.get(ORG_SECURITY_QUERY_KEYS.ecosystem)
    ),
    packageQuery: normalizeOrgSecurityPackageQuery(
      query.get(ORG_SECURITY_QUERY_KEYS.package)
    ),
  };
}

export function buildOrgSecurityPath(
  slug: string,
  {
    severities,
    ecosystem,
    packageQuery,
  }: {
    severities?: string | readonly string[] | null | undefined;
    ecosystem?: string | null | undefined;
    packageQuery?: string | null | undefined;
  },
  currentSearch: string | URLSearchParams = ''
): string {
  const params =
    currentSearch instanceof URLSearchParams
      ? new URLSearchParams(currentSearch)
      : new URLSearchParams(currentSearch);
  const normalizedSeverities = normalizeOrgSecuritySeverityValues(severities);
  const normalizedEcosystem = normalizeOrgSecurityEcosystem(ecosystem);
  const normalizedPackageQuery = normalizeOrgSecurityPackageQuery(packageQuery);

  if (normalizedSeverities.length > 0) {
    params.set(
      ORG_SECURITY_QUERY_KEYS.severity,
      normalizedSeverities.join(',')
    );
  } else {
    params.delete(ORG_SECURITY_QUERY_KEYS.severity);
  }

  if (normalizedEcosystem) {
    params.set(ORG_SECURITY_QUERY_KEYS.ecosystem, normalizedEcosystem);
  } else {
    params.delete(ORG_SECURITY_QUERY_KEYS.ecosystem);
  }

  if (normalizedPackageQuery) {
    params.set(ORG_SECURITY_QUERY_KEYS.package, normalizedPackageQuery);
  } else {
    params.delete(ORG_SECURITY_QUERY_KEYS.package);
  }

  const queryString = params.toString();
  const encodedSlug = encodeURIComponent(slug);
  return queryString
    ? `/orgs/${encodedSlug}?${queryString}`
    : `/orgs/${encodedSlug}`;
}

export function buildOrgSecurityExportFilename(
  slug: string,
  {
    severities,
    ecosystem,
    packageQuery,
  }: {
    severities?: string | readonly string[] | null | undefined;
    ecosystem?: string | null | undefined;
    packageQuery?: string | null | undefined;
  },
  exportedAt: Date = new Date()
): string {
  const normalizedSlug =
    normalizeOrgSecurityFilenamePart(slug) || 'organization';
  const normalizedSeverities = normalizeOrgSecuritySeverityValues(severities);
  const normalizedEcosystem = normalizeOrgSecurityEcosystem(ecosystem);
  const normalizedPackageQuery = normalizeOrgSecurityFilenamePart(packageQuery);
  const exportDate = Number.isNaN(exportedAt.getTime())
    ? new Date().toISOString().slice(0, 10)
    : exportedAt.toISOString().slice(0, 10);
  const parts = [`org-security-${normalizedSlug}`];

  if (normalizedSeverities.length > 0) {
    parts.push(normalizedSeverities.join('_'));
  }

  if (normalizedEcosystem) {
    parts.push(normalizedEcosystem);
  }

  if (normalizedPackageQuery) {
    parts.push(`package-${normalizedPackageQuery}`);
  }

  parts.push(exportDate);

  return `${parts.join('--')}.csv`;
}

function normalizeOrgSecurityFilenamePart(
  value: string | null | undefined
): string {
  if (typeof value !== 'string') {
    return '';
  }

  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, '-')
    .replace(/-+/g, '-')
    .replace(/^-|-$/g, '');
}
