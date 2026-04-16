import { api } from './client.js';

export async function searchPackagesRaw(queryParams) {
  const { data } = await api.get('/v1/search', { query: queryParams });
  return data;
}
