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
  can_manage_releases?: boolean;
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
  is_deprecated?: boolean;
  status?: NullableString;
  description?: NullableString;
  changelog?: NullableString;
  is_prerelease?: boolean;
  yank_reason?: NullableString;
  deprecation_message?: NullableString;
  source_ref?: NullableString;
  can_manage_releases?: boolean;
  sha256?: NullableString;
}

export interface Artifact {
  kind?: NullableString;
  filename: string;
  content_type?: NullableString;
  size_bytes?: number | null;
  sha256?: NullableString;
  sha512?: NullableString;
  uploaded_at?: NullableString;
  is_signed?: boolean;
  signature_key_id?: NullableString;
}

export interface CreateReleaseInput {
  version: string;
  description?: NullableString;
  changelog?: NullableString;
  sourceRef?: NullableString;
  isPrerelease?: boolean;
}

export interface UploadReleaseArtifactInput {
  filename: string;
  kind: string;
  file: File;
  sha256?: NullableString;
  isSigned?: boolean;
  signatureKeyId?: NullableString;
}

export interface ReleaseMutationResult {
  message?: NullableString;
  version?: NullableString;
  status?: NullableString;
  artifact_count?: number | null;
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

export async function createRelease(
  ecosystem: string,
  name: string,
  input: CreateReleaseInput
): Promise<Release> {
  const { data } = await api.post<Release>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases`,
    {
      body: {
        version: input.version,
        description: emptyToUndefined(input.description),
        changelog: emptyToUndefined(input.changelog),
        source_ref: emptyToUndefined(input.sourceRef),
        is_prerelease: input.isPrerelease || undefined,
      },
    }
  );

  return data;
}

export async function uploadReleaseArtifact(
  ecosystem: string,
  name: string,
  version: string,
  input: UploadReleaseArtifactInput
): Promise<Artifact> {
  const { data } = await api.put<Artifact>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases/${enc(version)}/artifacts/${enc(input.filename)}`,
    {
      query: {
        kind: input.kind,
        sha256: emptyToUndefined(input.sha256),
        is_signed: input.isSigned,
        signature_key_id: emptyToUndefined(input.signatureKeyId),
      },
      headers: input.file.type
        ? {
            'Content-Type': input.file.type,
          }
        : undefined,
      body: input.file,
    }
  );

  return data;
}

export async function publishRelease(
  ecosystem: string,
  name: string,
  version: string
): Promise<ReleaseMutationResult> {
  const { data } = await api.post<ReleaseMutationResult>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases/${enc(version)}/publish`
  );

  return data;
}

export async function yankRelease(
  ecosystem: string,
  name: string,
  version: string,
  { reason }: { reason?: NullableString } = {}
): Promise<ReleaseMutationResult> {
  const { data } = await api.put<ReleaseMutationResult>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases/${enc(version)}/yank`,
    {
      body: {
        reason: emptyToUndefined(reason),
      },
    }
  );

  return data;
}

export async function unyankRelease(
  ecosystem: string,
  name: string,
  version: string
): Promise<ReleaseMutationResult> {
  const { data } = await api.put<ReleaseMutationResult>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases/${enc(version)}/unyank`
  );

  return data;
}

export async function deprecateRelease(
  ecosystem: string,
  name: string,
  version: string,
  { message }: { message?: NullableString } = {}
): Promise<ReleaseMutationResult> {
  const { data } = await api.put<ReleaseMutationResult>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases/${enc(version)}/deprecate`,
    {
      body: {
        message: emptyToUndefined(message),
      },
    }
  );

  return data;
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

function emptyToUndefined(
  value: NullableString | undefined
): string | undefined {
  if (value == null) {
    return undefined;
  }

  const trimmed = value.trim();
  return trimmed ? trimmed : undefined;
}
