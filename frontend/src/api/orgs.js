import { api } from './client.js';

export async function createOrg({ name, display_name, description }) {
  const { data } = await api.post('/v1/orgs', {
    body: { name, display_name, description },
  });
  return data;
}

export async function getOrg(slug) {
  const { data } = await api.get(`/v1/orgs/${enc(slug)}`);
  return data;
}

export async function updateOrg(slug, updates) {
  const { data } = await api.patch(`/v1/orgs/${enc(slug)}`, {
    body: updates,
  });
  return data;
}

export async function listMembers(slug) {
  const { data } = await api.get(`/v1/orgs/${enc(slug)}/members`);
  return data;
}

export async function addMember(slug, { username, role }) {
  const { data } = await api.post(`/v1/orgs/${enc(slug)}/members`, {
    body: { username, role },
  });
  return data;
}

export async function removeMember(slug, username) {
  await api.delete(`/v1/orgs/${enc(slug)}/members/${enc(username)}`);
}

export async function listTeams(slug) {
  const { data } = await api.get(`/v1/orgs/${enc(slug)}/teams`);
  return data;
}

export async function listOrgPackages(slug) {
  const { data } = await api.get(`/v1/orgs/${enc(slug)}/packages`);
  return data;
}

export async function sendInvitation(slug, { username, email, role }) {
  const { data } = await api.post(`/v1/orgs/${enc(slug)}/invitations`, {
    body: { username, email, role },
  });
  return data;
}

export async function listMyInvitations() {
  const { data } = await api.get('/v1/org-invitations');
  return data;
}

export async function acceptInvitation(id) {
  const { data } = await api.post(`/v1/org-invitations/${enc(id)}/accept`);
  return data;
}

export async function declineInvitation(id) {
  const { data } = await api.post(`/v1/org-invitations/${enc(id)}/decline`);
  return data;
}

function enc(s) {
  return encodeURIComponent(s);
}
