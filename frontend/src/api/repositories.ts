import { api } from './client';

type NullableString = string | null;

export interface RepositoryDetail {
  id?: NullableString;
  name?: NullableString;
  slug?: NullableString;
  description?: NullableString;
  kind?: NullableString;
  visibility?: NullableString;
  upstream_url?: NullableString;
  owner_org_id?: NullableString;
  created_at?: NullableString;
  updated_at?: NullableString;
}

export interface RepositoryPackageSummary {
  id?: NullableString;
  name?: NullableString;
  ecosystem?: NullableString;
  description?: NullableString;
  visibility?: NullableString;
  download_count?: number | null;
  created_at?: NullableString;
}

export interface RepositoryPackageListResponse {
  packages: RepositoryPackageSummary[];
  load_error?: NullableString;
}

export interface ListRepositoryPackagesQuery {
  page?: number;
  perPage?: number;
}

export interface CreateRepositoryInput {
  name: string;
  slug: string;
  kind: string;
  visibility: string;
  description?: NullableString;
  upstreamUrl?: NullableString;
  ownerOrgId?: string;
}

export interface UpdateRepositoryInput {
  description?: string | null;
  visibility?: string;
  upstreamUrl?: string | null;
}

export async function createRepository(
  input: CreateRepositoryInput
): Promise<RepositoryDetail> {
  const { data } = await api.post<RepositoryDetail>('/v1/repositories', {
    body: {
      name: input.name,
      slug: input.slug,
      kind: input.kind,
      visibility: input.visibility,
      description: input.description,
      upstream_url: input.upstreamUrl,
      owner_org_id: input.ownerOrgId,
    },
  });

  return data;
}

export async function updateRepository(
  slug: string,
  updates: UpdateRepositoryInput
): Promise<Record<string, unknown>> {
  const { data } = await api.patch<Record<string, unknown>>(
    `/v1/repositories/${encodeURIComponent(slug)}`,
    {
      body: {
        description: updates.description,
        visibility: updates.visibility,
        upstream_url: updates.upstreamUrl,
      },
    }
  );

  return data;
}

export async function listRepositoryPackages(
  slug: string,
  query: ListRepositoryPackagesQuery = {}
): Promise<RepositoryPackageListResponse> {
  const { data } = await api.get<RepositoryPackageListResponse>(
    `/v1/repositories/${encodeURIComponent(slug)}/packages`,
    {
      query: {
        page: query.page,
        per_page: query.perPage,
      },
    }
  );

  return data;
}
