import { api } from './client';

type NullableString = string | null;

export interface SearchPackagesOptions {
  q?: string;
  ecosystem?: string;
  page?: number;
  perPage?: number;
}

export interface SearchPackage {
  ecosystem?: NullableString;
  name: string;
  display_name?: NullableString;
  latest_version?: NullableString;
  is_deprecated?: boolean;
  owner_name?: NullableString;
  download_count?: number | null;
  updated_at?: NullableString;
  description?: NullableString;
}

export interface SearchPackagesResponse {
  total: number;
  packages: SearchPackage[];
  page?: number;
  per_page?: number;
}

export interface PackageDetail {
  name: string;
  display_name?: NullableString;
  latest_version?: NullableString;
  is_deprecated?: boolean;
  is_archived?: boolean;
  description?: NullableString;
  readme?: NullableString;
  license?: NullableString;
  visibility?: NullableString;
  download_count?: number | null;
  created_at?: NullableString;
  updated_at?: NullableString;
  owner_username?: NullableString;
  owner_org_slug?: NullableString;
  can_transfer?: boolean;
  homepage?: NullableString;
  repository_url?: NullableString;
  keywords?: string[] | null;
}

interface ReleaseListResponse {
  releases?: Release[] | null;
}

interface ArtifactListResponse {
  artifacts?: Artifact[] | null;
}

interface TagListResponse {
  tags?: Record<
    string,
    {
      version?: NullableString;
    } | null
  > | null;
}

export interface PackageTransferOwnershipResult {
  message?: NullableString;
  package?: {
    id?: NullableString;
    ecosystem?: NullableString;
    name?: NullableString;
    normalized_name?: NullableString;
  } | null;
  owner?: {
    type?: NullableString;
    id?: NullableString;
    slug?: NullableString;
    name?: NullableString;
  } | null;
}

export interface Release {
  version: string;
  published_at?: NullableString;
  created_at?: NullableString;
  is_yanked?: boolean;
  status?: NullableString;
  description?: NullableString;
  changelog?: NullableString;
  is_prerelease?: boolean;
  sha256?: NullableString;
}

export interface Artifact {
  filename: string;
  content_type?: NullableString;
  size_bytes?: number | null;
}

export interface Tag {
  tag?: NullableString;
  name?: NullableString;
  version: string;
}

export interface StatsResponse {
  packages?: number;
  releases?: number;
  organizations?: number;
}

export async function searchPackages({
  q,
  ecosystem,
  page,
  perPage,
}: SearchPackagesOptions = {}): Promise<SearchPackagesResponse> {
  const { data } = await api.get<SearchPackagesResponse>('/v1/search', {
    query: { q, ecosystem, page, per_page: perPage },
  });

  return data;
}

export async function getPackage(
  ecosystem: string,
  name: string
): Promise<PackageDetail> {
  const { data } = await api.get<PackageDetail>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}`
  );

  return data;
}

export async function listReleases(
  ecosystem: string,
  name: string,
  { page, perPage }: { page?: number; perPage?: number } = {}
): Promise<Release[]> {
  const { data } = await api.get<ReleaseListResponse>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases`,
    { query: { page, per_page: perPage } }
  );

  return data.releases || [];
}

export async function getRelease(
  ecosystem: string,
  name: string,
  version: string
): Promise<Release> {
  const { data } = await api.get<Release>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases/${enc(version)}`
  );

  return data;
}

export async function listArtifacts(
  ecosystem: string,
  name: string,
  version: string
): Promise<Artifact[]> {
  const { data } = await api.get<ArtifactListResponse>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases/${enc(version)}/artifacts`
  );

  return data.artifacts || [];
}

export async function listTags(
  ecosystem: string,
  name: string
): Promise<Tag[]> {
  const { data } = await api.get<TagListResponse>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/tags`
  );

  return Object.entries(data.tags || {}).map(([tag, details]) => ({
    tag,
    name: tag,
    version: details?.version || '',
  }));
}

export async function transferPackageOwnership(
  ecosystem: string,
  name: string,
  { targetOrgSlug }: { targetOrgSlug: string }
): Promise<PackageTransferOwnershipResult> {
  const { data } = await api.post<PackageTransferOwnershipResult>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/ownership-transfer`,
    {
      body: {
        target_org_slug: targetOrgSlug,
      },
    }
  );

  return data;
}

export interface SecurityFinding {
  id: string;
  kind: string;
  severity: string;
  title: string;
  description?: NullableString;
  advisory_id?: NullableString;
  is_resolved: boolean;
  resolved_at?: NullableString;
  resolved_by?: NullableString;
  detected_at: string;
  release_version?: NullableString;
  artifact_filename?: NullableString;
}

interface SecurityFindingsResponse {
  findings: SecurityFinding[];
}

export interface ListSecurityFindingsOptions {
  includeResolved?: boolean;
}

export async function listSecurityFindings(
  ecosystem: string,
  name: string,
  { includeResolved = false }: ListSecurityFindingsOptions = {}
): Promise<SecurityFinding[]> {
  const { data } = await api.get<SecurityFindingsResponse>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/security-findings`,
    { query: { include_resolved: includeResolved || undefined } }
  );

  return data.findings || [];
}

const SEVERITY_LEVELS: Record<string, number> = {
  critical: 4,
  high: 3,
  medium: 2,
  low: 1,
  info: 0,
};

export function severityLevel(severity: string): number {
  return SEVERITY_LEVELS[severity.toLowerCase()] ?? -1;
}

export async function getStats(): Promise<StatsResponse> {
  const { data } = await api.get<StatsResponse>('/v1/stats');
  return data;
}

function enc(value: string): string {
  return encodeURIComponent(value);
}
