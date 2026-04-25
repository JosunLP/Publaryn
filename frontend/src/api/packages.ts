import { api } from './client';

type NullableString = string | null;
export type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

export type PackageEcosystemMetadata =
  | {
      kind: 'npm' | 'bun';
      details: {
        scope?: NullableString;
        unscoped_name: string;
      };
    }
  | {
      kind: 'pypi';
      details: {
        project_name: string;
        normalized_name: string;
      };
    }
  | {
      kind: 'cargo';
      details: {
        crate_name: string;
        normalized_name: string;
      };
    }
  | {
      kind: 'nuget';
      details: {
        package_id: string;
        normalized_id: string;
      };
    }
  | {
      kind: 'rubygems';
      details: {
        gem_name: string;
        normalized_name: string;
      };
    }
  | {
      kind: 'composer';
      details: {
        vendor: string;
        package: string;
      };
    }
  | {
      kind: 'maven';
      details: {
        group_id: string;
        artifact_id: string;
      };
    }
  | {
      kind: 'oci';
      details: {
        repository: string;
        segments: string[];
      };
    };

export type ReleaseEcosystemMetadata =
  | {
      kind: 'cargo';
      details: {
        dependencies: JsonValue;
        features: JsonValue;
        features2?: JsonValue | null;
        links?: NullableString;
        rust_version?: NullableString;
      };
    }
  | {
      kind: 'nuget';
      details: {
        authors?: NullableString;
        title?: NullableString;
        icon_url?: NullableString;
        license_url?: NullableString;
        license_expression?: NullableString;
        project_url?: NullableString;
        require_license_acceptance?: boolean;
        min_client_version?: NullableString;
        summary?: NullableString;
        tags: string[];
        dependency_groups: JsonValue;
        package_types: JsonValue;
        is_listed: boolean;
      };
    }
  | {
      kind: 'rubygems';
      details: {
        platform: string;
        summary?: NullableString;
        authors: string[];
        licenses: string[];
        required_ruby_version?: NullableString;
        required_rubygems_version?: NullableString;
        runtime_dependencies: JsonValue;
        development_dependencies: JsonValue;
      };
    }
  | {
      kind: 'maven';
      details: { [key: string]: JsonValue };
    }
  | {
      kind: 'composer';
      details: { [key: string]: JsonValue };
    }
  | {
      kind: 'oci';
      details: {
        manifest?: JsonValue | null;
        references: Array<{
          digest?: NullableString;
          kind?: NullableString;
          size?: number | null;
        }>;
      };
    };

export interface BundleAnalysisSummary {
  source_version?: NullableString;
  artifact_count?: number | null;
  total_artifact_size_bytes?: number | null;
  compressed_size_bytes?: number | null;
  install_size_bytes?: number | null;
  file_count?: number | null;
  direct_dependency_count?: number | null;
  runtime_dependency_count?: number | null;
  development_dependency_count?: number | null;
  peer_dependency_count?: number | null;
  optional_dependency_count?: number | null;
  bundled_dependency_count?: number | null;
  dependency_group_count?: number | null;
  extra_count?: number | null;
  package_type_count?: number | null;
  layer_count?: number | null;
  install_script_count?: number | null;
  has_cli_entrypoints?: boolean | null;
  has_tree_shaking_hints?: boolean | null;
  has_native_code?: boolean | null;
  risk?: BundleRiskSummary | null;
  notes?: string[] | null;
}

export interface BundleRiskSummary {
  score?: number | null;
  level?: NullableString;
  unresolved_finding_count?: number | null;
  worst_unresolved_severity?: NullableString;
  factors?: string[] | null;
}

export interface SearchPackagesOptions {
  q?: string;
  ecosystem?: string;
  org?: string;
  repository?: string;
  page?: number;
  perPage?: number;
}

export interface SearchPackage {
  ecosystem?: NullableString;
  name: string;
  display_name?: NullableString;
  latest_version?: NullableString;
  is_deprecated?: boolean;
  visibility?: NullableString;
  owner_name?: NullableString;
  repository_name?: NullableString;
  repository_slug?: NullableString;
  download_count?: number | null;
  updated_at?: NullableString;
  description?: NullableString;
  discovery?: SearchPackageDiscoverySummary | null;
}

export interface SearchPackageDiscoverySummary {
  risk_level?: NullableString;
  unresolved_security_finding_count?: number | null;
  worst_unresolved_security_severity?: NullableString;
  has_trusted_publisher?: boolean | null;
  trusted_publisher_count?: number | null;
  latest_release_status?: NullableString;
  latest_release_published_at?: NullableString;
  signals?: string[] | null;
}

export interface SearchPackagesResponse {
  total: number;
  packages: SearchPackage[];
  page?: number;
  per_page?: number;
}

export interface PackageDetail {
  ecosystem?: NullableString;
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
  can_manage_metadata?: boolean;
  can_manage_releases?: boolean;
  can_manage_trusted_publishers?: boolean;
  can_manage_visibility?: boolean;
  can_manage_security?: boolean;
  can_transfer?: boolean;
  team_access?: Array<{
    team_id?: NullableString;
    team_slug?: NullableString;
    team_name?: NullableString;
    permissions?: string[] | null;
    granted_at?: NullableString;
  }> | null;
  homepage?: NullableString;
  repository_url?: NullableString;
  keywords?: string[] | null;
  ecosystem_metadata?: PackageEcosystemMetadata | null;
  bundle_analysis?: BundleAnalysisSummary | null;
}

export interface CreatePackageInput {
  ecosystem: string;
  name: string;
  repositorySlug: string;
  visibility?: NullableString;
  displayName?: NullableString;
  description?: NullableString;
}

export interface CreatePackageResult {
  id?: NullableString;
  ecosystem?: NullableString;
  name?: NullableString;
  normalized_name?: NullableString;
  repository_slug?: NullableString;
  visibility?: NullableString;
  owner_user_id?: NullableString;
  owner_org_id?: NullableString;
}

export interface UpdatePackageInput {
  description?: NullableString;
  homepage?: NullableString;
  repositoryUrl?: NullableString;
  license?: NullableString;
  keywords?: string[] | null;
  readme?: NullableString;
  visibility?: NullableString;
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

interface TrustedPublisherListResponse {
  trusted_publishers?: TrustedPublisher[] | null;
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
  ecosystem?: NullableString;
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
  ecosystem_metadata?: ReleaseEcosystemMetadata | null;
  bundle_analysis?: BundleAnalysisSummary | null;
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

export interface TagMutationResult {
  message?: NullableString;
  tag?: NullableString;
  version?: NullableString;
}

export interface TrustedPublisher {
  id?: NullableString;
  issuer?: NullableString;
  subject?: NullableString;
  repository?: NullableString;
  workflow_ref?: NullableString;
  environment?: NullableString;
  created_by?: NullableString;
  created_at?: NullableString;
}

export interface CreateTrustedPublisherInput {
  issuer: string;
  subject: string;
  repository?: NullableString;
  workflowRef?: NullableString;
  environment?: NullableString;
}

export interface TrustedPublisherMutationResult {
  message?: NullableString;
}

export interface PackageMutationResult {
  message?: NullableString;
}

export interface StatsResponse {
  packages?: number;
  releases?: number;
  organizations?: number;
}

export async function searchPackages({
  q,
  ecosystem,
  org,
  repository,
  page,
  perPage,
}: SearchPackagesOptions = {}): Promise<SearchPackagesResponse> {
  const { data } = await api.get<SearchPackagesResponse>('/v1/search', {
    query: { q, ecosystem, org, repository, page, per_page: perPage },
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

export async function createPackage(
  input: CreatePackageInput
): Promise<CreatePackageResult> {
  const { data } = await api.post<CreatePackageResult>('/v1/packages', {
    body: {
      ecosystem: input.ecosystem,
      name: input.name,
      repository_slug: input.repositorySlug,
      visibility: emptyToUndefined(input.visibility),
      display_name: emptyToUndefined(input.displayName),
      description: emptyToUndefined(input.description),
    },
  });

  return data;
}

export async function updatePackage(
  ecosystem: string,
  name: string,
  input: UpdatePackageInput
): Promise<PackageMutationResult> {
  const body: Record<string, NullableString | string[] | null> = {};

  if (hasOwn(input, 'description')) {
    body.description = input.description ?? null;
  }
  if (hasOwn(input, 'homepage')) {
    body.homepage = input.homepage ?? null;
  }
  if (hasOwn(input, 'repositoryUrl')) {
    body.repository_url = input.repositoryUrl ?? null;
  }
  if (hasOwn(input, 'license')) {
    body.license = input.license ?? null;
  }
  if (hasOwn(input, 'keywords')) {
    body.keywords = input.keywords ?? null;
  }
  if (hasOwn(input, 'readme')) {
    body.readme = input.readme ?? null;
  }
  if (hasOwn(input, 'visibility')) {
    const visibility = emptyToUndefined(input.visibility);
    if (visibility !== undefined) {
      body.visibility = visibility;
    }
  }

  const { data } = await api.patch<PackageMutationResult>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}`,
    {
      body,
    }
  );

  return data;
}

export async function deletePackage(
  ecosystem: string,
  name: string
): Promise<PackageMutationResult> {
  const { data } = await api.delete<PackageMutationResult>(
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

export async function undeprecateRelease(
  ecosystem: string,
  name: string,
  version: string
): Promise<ReleaseMutationResult> {
  const { data } = await api.put<ReleaseMutationResult>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases/${enc(version)}/undeprecate`
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

export async function upsertTag(
  ecosystem: string,
  name: string,
  tag: string,
  { version }: { version: string }
): Promise<TagMutationResult> {
  const { data } = await api.put<TagMutationResult>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/tags/${enc(tag)}`,
    {
      body: {
        version,
      },
    }
  );

  return data;
}

export async function deleteTag(
  ecosystem: string,
  name: string,
  tag: string
): Promise<TagMutationResult> {
  const { data } = await api.delete<TagMutationResult>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/tags/${enc(tag)}`
  );

  return data;
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

export async function listTrustedPublishers(
  ecosystem: string,
  name: string
): Promise<TrustedPublisher[]> {
  const { data } = await api.get<TrustedPublisherListResponse>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/trusted-publishers`
  );

  return data.trusted_publishers || [];
}

export async function createTrustedPublisher(
  ecosystem: string,
  name: string,
  input: CreateTrustedPublisherInput
): Promise<TrustedPublisher> {
  const { data } = await api.post<TrustedPublisher>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/trusted-publishers`,
    {
      body: {
        issuer: input.issuer,
        subject: input.subject,
        repository: emptyToUndefined(input.repository),
        workflow_ref: emptyToUndefined(input.workflowRef),
        environment: emptyToUndefined(input.environment),
      },
    }
  );

  return data;
}

export async function deleteTrustedPublisher(
  ecosystem: string,
  name: string,
  publisherId: string
): Promise<TrustedPublisherMutationResult> {
  const { data } = await api.delete<TrustedPublisherMutationResult>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/trusted-publishers/${enc(publisherId)}`
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

export interface UpdateSecurityFindingInput {
  isResolved: boolean;
  note?: string;
}

export async function updateSecurityFinding(
  ecosystem: string,
  name: string,
  findingId: string,
  { isResolved, note }: UpdateSecurityFindingInput
): Promise<SecurityFinding> {
  const body: Record<string, unknown> = { is_resolved: isResolved };
  if (note !== undefined && note.trim().length > 0) {
    body.note = note.trim();
  }
  const { data } = await api.patch<SecurityFinding>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/security-findings/${enc(findingId)}`,
    { body }
  );

  return data;
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

function hasOwn<ObjectType extends object, Key extends PropertyKey>(
  value: ObjectType,
  key: Key
): value is ObjectType & Record<Key, unknown> {
  return Object.prototype.hasOwnProperty.call(value, key);
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
