/**
 * Publaryn API client.
 *
 * Wraps fetch() with auth token injection, JSON parsing, error normalization,
 * and request-id forwarding.
 */

const AUTH_TOKEN_STORAGE_KEY = 'publaryn.authToken';

let _authToken =
  typeof window !== 'undefined'
    ? window.localStorage.getItem(AUTH_TOKEN_STORAGE_KEY)
    : null;
let _onUnauthorized = null;

export function setAuthToken(token) {
  _authToken = token;
  if (typeof window !== 'undefined') {
    if (token) {
      window.localStorage.setItem(AUTH_TOKEN_STORAGE_KEY, token);
    } else {
      window.localStorage.removeItem(AUTH_TOKEN_STORAGE_KEY);
    }
  }
}

export function getAuthToken() {
  return _authToken;
}

export function clearAuthToken() {
  setAuthToken(null);
}

export function onUnauthorized(callback) {
  _onUnauthorized = callback;
}

export class ApiError extends Error {
  constructor(status, body) {
    super(body?.error || `HTTP ${status}`);
    this.status = status;
    this.body = body;
  }
}

async function request(method, path, { body, query, headers: extra } = {}) {
  const url = new URL(path, window.location.origin);
  if (query) {
    for (const [k, v] of Object.entries(query)) {
      if (v != null && v !== '') url.searchParams.set(k, v);
    }
  }

  const headers = { ...extra };
  if (_authToken) {
    headers['Authorization'] = `Bearer ${_authToken}`;
  }
  if (body && !(body instanceof FormData)) {
    headers['Content-Type'] = 'application/json';
  }

  const resp = await fetch(url.toString(), {
    method,
    headers,
    body:
      body instanceof FormData ? body : body ? JSON.stringify(body) : undefined,
  });

  const requestId = resp.headers.get('x-request-id');

  if (resp.status === 204) return { data: null, requestId };

  let data;
  const ct = resp.headers.get('content-type') || '';
  if (ct.includes('application/json')) {
    data = await resp.json();
  } else {
    data = await resp.text();
  }

  if (!resp.ok) {
    if (resp.status === 401 && _onUnauthorized) {
      _onUnauthorized();
    }
    throw new ApiError(resp.status, data);
  }

  return { data, requestId };
}

export const api = {
  get: (path, opts) => request('GET', path, opts),
  post: (path, opts) => request('POST', path, opts),
  put: (path, opts) => request('PUT', path, opts),
  patch: (path, opts) => request('PATCH', path, opts),
  delete: (path, opts) => request('DELETE', path, opts),
};
