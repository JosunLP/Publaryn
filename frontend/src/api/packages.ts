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
  homepage?: NullableString;
  repository_url?: NullableString;
  keywords?: string[] | null;
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
  const { data } = await api.get<Release[]>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases`,
    { query: { page, per_page: perPage } }
  );

  return data;
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
  const { data } = await api.get<Artifact[]>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases/${enc(version)}/artifacts`
  );

  return data;
}

export async function listTags(
  ecosystem: string,
  name: string
): Promise<Tag[]> {
  const { data } = await api.get<Tag[]>(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/tags`
  );

  return data;
}

export async function getStats(): Promise<StatsResponse> {
  const { data } = await api.get<StatsResponse>('/v1/stats');
  return data;
}

function enc(value: string): string {
  return encodeURIComponent(value);
}
