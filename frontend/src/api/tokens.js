import { api } from './client.js';

export async function createToken({ name, scopes, expires_in_days }) {
  const { data } = await api.post('/v1/tokens', {
    body: { name, scopes, expires_in_days },
  });
  return data;
}

export async function listTokens() {
  const { data } = await api.get('/v1/tokens');
  return data;
}

export async function revokeToken(id) {
  await api.delete(`/v1/tokens/${encodeURIComponent(id)}`);
}
