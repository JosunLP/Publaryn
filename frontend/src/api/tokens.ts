import { api } from './client';

type NullableString = string | null;

export interface CreateTokenInput {
  name: string;
  scopes: string[];
  expires_in_days?: number | null;
}

export interface TokenRecord {
  id?: NullableString;
  name?: NullableString;
  kind?: NullableString;
  created_at?: NullableString;
  last_used_at?: NullableString;
  expires_at?: NullableString;
  scopes?: string[];
  prefix?: NullableString;
}

export interface TokenListResponse {
  tokens: TokenRecord[];
}

export interface CreateTokenResponse {
  token: string;
}

export async function createToken(
  input: CreateTokenInput
): Promise<CreateTokenResponse> {
  const { data } = await api.post<CreateTokenResponse>('/v1/tokens', {
    body: {
      name: input.name,
      scopes: input.scopes,
      expires_in_days: input.expires_in_days,
    },
  });

  return data;
}

export async function listTokens(): Promise<TokenListResponse> {
  const { data } = await api.get<TokenListResponse>('/v1/tokens');
  return data;
}

export async function revokeToken(id: string): Promise<void> {
  await api.delete<null>(`/v1/tokens/${encodeURIComponent(id)}`);
}
