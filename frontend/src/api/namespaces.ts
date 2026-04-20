import { api } from './client';

type NullableString = string | null;

export interface NamespaceClaim {
  id?: NullableString;
  ecosystem?: NullableString;
  namespace?: NullableString;
  owner_user_id?: NullableString;
  owner_org_id?: NullableString;
  is_verified?: boolean | null;
  created_at?: NullableString;
}

export interface NamespaceListResponse {
  namespaces: NamespaceClaim[];
  load_error?: NullableString;
}

export interface ListNamespacesQuery {
  ecosystem?: string;
  ownerUserId?: string;
  ownerOrgId?: string;
  verified?: boolean;
}

export interface CreateNamespaceClaimInput {
  ecosystem: string;
  namespace: string;
  ownerUserId?: string;
  ownerOrgId?: string;
}

export async function listNamespaces(
  query: ListNamespacesQuery = {}
): Promise<NamespaceListResponse> {
  const { data } = await api.get<NamespaceListResponse>('/v1/namespaces', {
    query: {
      ecosystem: query.ecosystem,
      owner_user_id: query.ownerUserId,
      owner_org_id: query.ownerOrgId,
      verified:
        typeof query.verified === 'boolean'
          ? String(query.verified)
          : undefined,
    },
  });

  return data;
}

export async function listOrgNamespaces(
  ownerOrgId: string
): Promise<NamespaceListResponse> {
  return listNamespaces({ ownerOrgId });
}

export async function listUserNamespaces(
  ownerUserId: string
): Promise<NamespaceListResponse> {
  return listNamespaces({ ownerUserId });
}

export async function createNamespaceClaim(
  input: CreateNamespaceClaimInput
): Promise<NamespaceClaim> {
  const { data } = await api.post<NamespaceClaim>('/v1/namespaces', {
    body: {
      ecosystem: input.ecosystem,
      namespace: input.namespace,
      owner_user_id: input.ownerUserId,
      owner_org_id: input.ownerOrgId,
    },
  });

  return data;
}

export async function deleteNamespaceClaim(claimId: string): Promise<void> {
  await api.delete<null>(`/v1/namespaces/${encodeURIComponent(claimId)}`);
}
