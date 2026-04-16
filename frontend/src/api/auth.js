import { api, clearAuthToken, setAuthToken } from './client.js';

export async function register({ username, email, password }) {
  const { data } = await api.post('/v1/auth/register', {
    body: { username, email, password },
  });
  return data;
}

export async function login({ usernameOrEmail, password }) {
  const { data } = await api.post('/v1/auth/login', {
    body: { username_or_email: usernameOrEmail, password },
  });
  if (data.token) {
    setAuthToken(data.token);
  }
  return data;
}

export async function completeMfaChallenge({ mfaToken, code }) {
  const { data } = await api.post('/v1/auth/mfa/challenge', {
    body: { mfa_token: mfaToken, code },
  });
  if (data.token) {
    setAuthToken(data.token);
  }
  return data;
}

export async function logout() {
  try {
    await api.post('/v1/auth/logout');
  } finally {
    clearAuthToken();
  }
}

export async function getProfile(username) {
  const { data } = await api.get(`/v1/users/${encodeURIComponent(username)}`);
  return data;
}

export async function getCurrentUser() {
  const { data } = await api.get('/v1/users/me');
  return data;
}

export async function updateCurrentUser(updates) {
  const { data } = await api.patch('/v1/users/me', {
    body: updates,
  });
  return data;
}

export async function updateProfile(username, updates) {
  const { data } = await api.patch(
    `/v1/users/${encodeURIComponent(username)}`,
    {
      body: updates,
    }
  );
  return data;
}

export async function setupMfa() {
  const { data } = await api.post('/v1/auth/mfa/setup');
  return data;
}

export async function verifyMfaSetup(code) {
  const { data } = await api.post('/v1/auth/mfa/verify-setup', {
    body: { code },
  });
  return data;
}

export async function disableMfa(code) {
  const { data } = await api.post('/v1/auth/mfa/disable', {
    body: { code },
  });
  return data;
}
