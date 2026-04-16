import { api } from './client.js';

export async function searchPackages({ q, ecosystem, page, perPage } = {}) {
  const { data } = await api.get('/v1/search', {
    query: { q, ecosystem, page, per_page: perPage },
  });
  return data;
}

export async function getPackage(ecosystem, name) {
  const { data } = await api.get(`/v1/packages/${enc(ecosystem)}/${enc(name)}`);
  return data;
}

export async function listReleases(ecosystem, name, { page, perPage } = {}) {
  const { data } = await api.get(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases`,
    { query: { page, per_page: perPage } }
  );
  return data;
}

export async function getRelease(ecosystem, name, version) {
  const { data } = await api.get(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases/${enc(version)}`
  );
  return data;
}

export async function listArtifacts(ecosystem, name, version) {
  const { data } = await api.get(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/releases/${enc(version)}/artifacts`
  );
  return data;
}

export async function listTags(ecosystem, name) {
  const { data } = await api.get(
    `/v1/packages/${enc(ecosystem)}/${enc(name)}/tags`
  );
  return data;
}

export async function getStats() {
  const { data } = await api.get('/v1/stats');
  return data;
}

function enc(s) {
  return encodeURIComponent(s);
}
