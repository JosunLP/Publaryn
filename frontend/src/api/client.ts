/**
 * Publaryn API client.
 *
 * Wraps fetch() with auth token injection, JSON parsing, error normalization,
 * and request-id forwarding.
 */

const AUTH_TOKEN_STORAGE_KEY = 'publaryn.authToken';

type QueryValue = string | number | boolean | null | undefined;
type BinaryRequestBody = Blob | ArrayBuffer | Uint8Array;
type RequestBody = FormData | BinaryRequestBody | object | null | undefined;
type UnauthorizedCallback = () => void;

export interface RequestOptions {
  body?: RequestBody;
  query?: Record<string, QueryValue>;
  headers?: HeadersInit;
}

export interface ApiResponse<T> {
  data: T;
  requestId: string | null;
}

let authToken: string | null =
  typeof window !== 'undefined'
    ? window.localStorage.getItem(AUTH_TOKEN_STORAGE_KEY)
    : null;
let unauthorizedCallback: UnauthorizedCallback | null = null;

export function setAuthToken(token: string | null): void {
  authToken = token;

  if (typeof window === 'undefined') {
    return;
  }

  if (token) {
    window.localStorage.setItem(AUTH_TOKEN_STORAGE_KEY, token);
  } else {
    window.localStorage.removeItem(AUTH_TOKEN_STORAGE_KEY);
  }
}

export function getAuthToken(): string | null {
  return authToken;
}

export function clearAuthToken(): void {
  setAuthToken(null);
}

export function onUnauthorized(callback: UnauthorizedCallback | null): void {
  unauthorizedCallback = callback;
}

export class ApiError<TBody = unknown> extends Error {
  readonly status: number;
  readonly body: TBody;

  constructor(status: number, body: TBody) {
    super(extractErrorMessage(body, status));
    this.name = 'ApiError';
    this.status = status;
    this.body = body;
  }
}

function extractErrorMessage(body: unknown, status: number): string {
  if (
    body &&
    typeof body === 'object' &&
    'error' in body &&
    typeof body.error === 'string'
  ) {
    return body.error;
  }

  return `HTTP ${status}`;
}

function isFormData(value: RequestBody): value is FormData {
  return typeof FormData !== 'undefined' && value instanceof FormData;
}

function isBinaryBody(value: RequestBody): value is BinaryRequestBody {
  return (
    (typeof Blob !== 'undefined' && value instanceof Blob) ||
    value instanceof ArrayBuffer ||
    value instanceof Uint8Array
  );
}

async function parseResponseBody(response: Response): Promise<unknown> {
  const contentType = response.headers.get('content-type') || '';

  if (contentType.includes('application/json')) {
    return response.json();
  }

  return response.text();
}

async function request<T>(
  method: string,
  path: string,
  { body, query, headers: extraHeaders }: RequestOptions = {}
): Promise<ApiResponse<T>> {
  const url = new URL(path, window.location.origin);

  if (query) {
    for (const [key, value] of Object.entries(query)) {
      if (value != null && value !== '') {
        url.searchParams.set(key, String(value));
      }
    }
  }

  const headers = new Headers(extraHeaders);

  if (authToken) {
    headers.set('Authorization', `Bearer ${authToken}`);
  }

  if (body && !isFormData(body) && !isBinaryBody(body)) {
    headers.set('Content-Type', 'application/json');
  }

  const requestBody: BodyInit | undefined =
    isFormData(body) || isBinaryBody(body)
      ? (body as unknown as BodyInit)
      : body
        ? JSON.stringify(body)
        : undefined;

  const response = await fetch(url.toString(), {
    method,
    headers,
    body: requestBody,
  });

  const requestId = response.headers.get('x-request-id');

  if (response.status === 204) {
    return { data: null as T, requestId };
  }

  const data = await parseResponseBody(response);

  if (!response.ok) {
    if (response.status === 401) {
      unauthorizedCallback?.();
    }

    throw new ApiError(response.status, data);
  }

  return {
    data: data as T,
    requestId,
  };
}

export const api = {
  get: <T>(path: string, options?: Omit<RequestOptions, 'body'>) =>
    request<T>('GET', path, options),
  post: <T>(path: string, options?: RequestOptions) =>
    request<T>('POST', path, options),
  put: <T>(path: string, options?: RequestOptions) =>
    request<T>('PUT', path, options),
  patch: <T>(path: string, options?: RequestOptions) =>
    request<T>('PATCH', path, options),
  delete: <T>(path: string, options?: Omit<RequestOptions, 'body'>) =>
    request<T>('DELETE', path, options),
};
