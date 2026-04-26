/// <reference path="./bun-test.d.ts" />

import { afterAll, afterEach, describe, expect, mock, test } from 'bun:test';
import { fileURLToPath } from 'node:url';

import { changeValue, renderSvelte, submitForm } from './svelte-dom';

interface ApiRequestBody {
  [key: string]: unknown;
}

interface ApiRequestOptions {
  query?: Record<string, unknown>;
  body?: ApiRequestBody;
}

interface AuthApiScenario {
  kind: 'auth-api';
  authToken: string | null;
  requests: string[];
  setupCalls: number;
  verifyCalls: string[];
  disableCalls: string[];
  user: {
    id: string;
    username: string;
    email: string;
    mfa_enabled: boolean;
  };
  setupState: {
    secret: string;
    provisioning_uri: string;
    recovery_codes: string[];
  };
}

interface LoginScenario {
  kind: 'login';
  gotoCalls: string[];
  requests: string[];
  loginCalls: Array<{ username_or_email: string; password: string }>;
  challengeCalls: Array<{ mfa_token: string; code: string }>;
  syncAuthTokenCalls: number;
  loginResponse: { token?: string | null; mfa_token?: string | null };
  challengeResponse: { token?: string | null; mfa_token?: string | null };
  challengeErrorStatus: number | null;
}

type Scenario = AuthApiScenario | LoginScenario;

interface ApiErrorBody {
  error?: string;
}

const clientModuleUrl = new URL('../src/api/client.ts', import.meta.url).href;
const sessionModuleUrl = new URL('../src/lib/session.ts', import.meta.url).href;
const authModuleUrl = new URL('../src/api/auth.ts', import.meta.url).href;
// Use a unique import URL so this lookup bypasses the mocked client-module cache
// and gives afterAll() a fresh copy of the real implementation to restore.
const realClientModule = await import(
  new URL('../src/api/client.ts?mfa-route-flow-real-client', import.meta.url).href
);
const LoginPagePath =
  fileURLToPath(new URL('../src/routes/login/+page.svelte', import.meta.url));

let currentScenario: Scenario | null = null;
let storedAuthToken: string | null = null;
let authApiPromise: Promise<typeof import('../src/api/auth')> | null = null;

mock.module('$app/navigation', () => ({
  async goto(href: string): Promise<void> {
    if (currentScenario?.kind === 'login') {
      currentScenario.gotoCalls.push(href);
    }
  },
}));

mock.module(clientModuleUrl, () => {
  class ApiError<TBody = unknown> extends Error {
    readonly status: number;
    readonly body: TBody;

    constructor(status: number, body: TBody) {
      super(
        body &&
          typeof body === 'object' &&
          'error' in (body as Record<string, unknown>) &&
          typeof (body as Record<string, unknown>).error === 'string'
          ? String((body as Record<string, unknown>).error)
          : `HTTP ${status}`
      );
      this.name = 'ApiError';
      this.status = status;
      this.body = body;
    }
  }

  return {
    ApiError,
    getAuthToken(): string | null {
      return currentScenario?.kind === 'auth-api'
        ? currentScenario.authToken
        : storedAuthToken;
    },
    setAuthToken(token: string | null): void {
      storedAuthToken = token;
    },
    clearAuthToken(): void {
      storedAuthToken = null;
    },
    onUnauthorized(): void {},
    api: {
      get: (path: string, options?: ApiRequestOptions) =>
        handleApiRequest('GET', path, options),
      post: (path: string, options?: ApiRequestOptions) =>
        handleApiRequest('POST', path, options),
      put: (path: string, options?: ApiRequestOptions) =>
        handleApiRequest('PUT', path, options),
      patch: (path: string, options?: ApiRequestOptions) =>
        handleApiRequest('PATCH', path, options),
      delete: (path: string, options?: ApiRequestOptions) =>
        handleApiRequest('DELETE', path, options),
    },
  };
});

mock.module(sessionModuleUrl, () => ({
  syncAuthToken(): void {
    if (currentScenario?.kind === 'login') {
      currentScenario.syncAuthTokenCalls += 1;
    }
  },
  clearSession(): void {
    storedAuthToken = null;
  },
  authToken: {
    subscribe: (_callback: (value: string | null) => void) => () => {},
  },
}));

function getAuthApi(): Promise<typeof import('../src/api/auth')> {
  authApiPromise ??=
    import(authModuleUrl) as Promise<typeof import('../src/api/auth')>;
  return authApiPromise;
}

afterEach(() => {
  currentScenario = null;
  storedAuthToken = null;
  authApiPromise = null;
});

afterAll(() => {
  mock.module('$app/navigation', () => ({
    async goto(): Promise<void> {},
  }));
  mock.module(clientModuleUrl, () => realClientModule);
  mock.module(sessionModuleUrl, () => ({
    syncAuthToken(): void {},
    clearSession(): void {},
    authToken: {
      subscribe: (_callback: (value: string | null) => void) => () => {},
    },
  }));
});

describe('auth MFA API helpers', () => {
  test('requests setup state and returns the shared secret and recovery codes', async () => {
    currentScenario = createAuthApiScenario(false);
    const authApi = await getAuthApi();

    const setupState = await authApi.setupMfa();

    expect(setupState).toEqual(requireAuthApiScenario().setupState);
    expect(requireAuthApiScenario().setupCalls).toBe(1);
    expect(requireAuthApiScenario().requests).toEqual(['/v1/auth/mfa/setup']);
  });

  test('submits MFA verification and disable codes to the expected endpoints', async () => {
    currentScenario = createAuthApiScenario(false);
    const authApi = await getAuthApi();

    await authApi.verifyMfaSetup('123456');
    expect(requireAuthApiScenario().verifyCalls).toEqual(['123456']);
    expect(requireAuthApiScenario().user.mfa_enabled).toBe(true);

    await authApi.disableMfa('recovery-a1');
    expect(requireAuthApiScenario().disableCalls).toEqual(['recovery-a1']);
    expect(requireAuthApiScenario().user.mfa_enabled).toBe(false);
    expect(requireAuthApiScenario().requests).toEqual([
      '/v1/auth/mfa/verify-setup',
      '/v1/auth/mfa/disable',
    ]);
  });
});

describe('login route MFA challenge flow', () => {
  test('transitions from password login to MFA challenge and completes sign-in', async () => {
    currentScenario = {
      kind: 'login',
      gotoCalls: [],
      requests: [],
      loginCalls: [],
      challengeCalls: [],
      syncAuthTokenCalls: 0,
      loginResponse: { mfa_token: 'pending-mfa-token' },
      challengeResponse: { token: 'session-token' },
      challengeErrorStatus: null,
    };

    const { target, unmount, flush } = await renderSvelte(LoginPagePath);

    try {
      changeValue(queryRequiredInput(target, '#login-username'), 'alice');
      changeValue(queryRequiredInput(target, '#login-password'), 'super-secret');
      submitForm(queryRequiredForm(target, '#login-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Complete verification');
        expect(target.querySelector('#mfa-form')).not.toBeNull();
      });

      changeValue(queryRequiredInput(target, '#mfa-code'), '654321');
      submitForm(queryRequiredForm(target, '#mfa-form'));

      await waitFor(() => {
        flush();
        expect(requireLoginScenario().gotoCalls).toEqual(['/']);
      });

      const scenario = requireLoginScenario();
      expect(scenario.loginCalls).toEqual([
        { username_or_email: 'alice', password: 'super-secret' },
      ]);
      expect(scenario.challengeCalls).toEqual([
        { mfa_token: 'pending-mfa-token', code: '654321' },
      ]);
      expect(scenario.requests).toEqual([
        '/v1/auth/login',
        '/v1/auth/mfa/challenge',
      ]);
      expect(scenario.syncAuthTokenCalls).toBe(1);
    } finally {
      unmount();
    }
  });

  test('keeps the MFA challenge open when a recovery code is rejected', async () => {
    currentScenario = {
      kind: 'login',
      gotoCalls: [],
      requests: [],
      loginCalls: [],
      challengeCalls: [],
      syncAuthTokenCalls: 0,
      loginResponse: { mfa_token: 'pending-mfa-token' },
      challengeResponse: {},
      challengeErrorStatus: 401,
    };

    const { target, unmount, flush } = await renderSvelte(LoginPagePath);

    try {
      changeValue(queryRequiredInput(target, '#login-username'), 'alice');
      changeValue(queryRequiredInput(target, '#login-password'), 'super-secret');
      submitForm(queryRequiredForm(target, '#login-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Complete verification');
      });

      changeValue(queryRequiredInput(target, '#mfa-code'), 'recovery-a1');
      submitForm(queryRequiredForm(target, '#mfa-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('The MFA code is invalid or expired.');
      });

      const scenario = requireLoginScenario();
      expect(scenario.challengeCalls).toEqual([
        { mfa_token: 'pending-mfa-token', code: 'recovery-a1' },
      ]);
      expect(scenario.requests).toEqual([
        '/v1/auth/login',
        '/v1/auth/mfa/challenge',
      ]);
      expect(scenario.gotoCalls).toEqual([]);
      expect(scenario.syncAuthTokenCalls).toBe(0);
    } finally {
      unmount();
    }
  });
});

async function handleApiRequest(
  method: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE',
  path: string,
  options?: ApiRequestOptions
): Promise<{ data: unknown }> {
  if (!currentScenario) {
    throw new Error(`No scenario configured for ${method} ${path}`);
  }

  currentScenario.requests.push(path);
  const { ApiError } = await import(clientModuleUrl);

  if (currentScenario.kind === 'auth-api') {
    switch (`${method} ${path}`) {
      case 'POST /v1/auth/mfa/setup':
        currentScenario.setupCalls += 1;
        return { data: currentScenario.setupState };
      case 'POST /v1/auth/mfa/verify-setup':
        currentScenario.verifyCalls.push(String(options?.body?.code || ''));
        currentScenario.user = { ...currentScenario.user, mfa_enabled: true };
        return { data: { ok: true } };
      case 'POST /v1/auth/mfa/disable':
        currentScenario.disableCalls.push(String(options?.body?.code || ''));
        currentScenario.user = { ...currentScenario.user, mfa_enabled: false };
        return { data: { ok: true } };
      default:
        throw new Error(`Unhandled auth API request: ${method} ${path}`);
    }
  }

  if (method === 'POST' && path === '/v1/auth/login') {
    currentScenario.loginCalls.push({
      username_or_email: String(options?.body?.username_or_email || ''),
      password: String(options?.body?.password || ''),
    });

    if (currentScenario.loginResponse.token) {
      storedAuthToken = currentScenario.loginResponse.token;
    }

    return { data: currentScenario.loginResponse };
  }

  if (method === 'POST' && path === '/v1/auth/mfa/challenge') {
    currentScenario.challengeCalls.push({
      mfa_token: String(options?.body?.mfa_token || ''),
      code: String(options?.body?.code || ''),
    });

    if (currentScenario.challengeErrorStatus != null) {
      throw new ApiError(currentScenario.challengeErrorStatus, {
        error: 'Invalid challenge',
      });
    }

    if (currentScenario.challengeResponse.token) {
      storedAuthToken = currentScenario.challengeResponse.token;
    }

    return { data: currentScenario.challengeResponse };
  }

  throw new Error(`Unhandled login API request: ${method} ${path}`);
}

function createAuthApiScenario(mfaEnabled: boolean): AuthApiScenario {
  return {
    kind: 'auth-api',
    authToken: 'test-token',
    requests: [],
    setupCalls: 0,
    verifyCalls: [],
    disableCalls: [],
    user: {
      id: 'user-1',
      username: 'alice',
      email: 'alice@example.test',
      mfa_enabled: mfaEnabled,
    },
    setupState: {
      secret: 'MANUALSECRET123',
      provisioning_uri: 'otpauth://totp/Publaryn:alice?secret=MANUALSECRET123&issuer=Publaryn',
      recovery_codes: ['recovery-a1', 'recovery-b2'],
    },
  };
}

function requireAuthApiScenario(): AuthApiScenario {
  if (!currentScenario || currentScenario.kind !== 'auth-api') {
    throw new Error('Expected an auth API scenario');
  }

  return currentScenario;
}

function requireLoginScenario(): LoginScenario {
  if (!currentScenario || currentScenario.kind !== 'login') {
    throw new Error('Expected a login scenario');
  }

  return currentScenario;
}

function queryRequiredInput(target: HTMLElement, selector: string): HTMLInputElement {
  const input = target.querySelector(selector);
  expect(input).not.toBeNull();
  return input as HTMLInputElement;
}

function queryRequiredForm(target: HTMLElement, selector: string): HTMLFormElement {
  const form = target.querySelector(selector);
  expect(form).not.toBeNull();
  return form as HTMLFormElement;
}

async function waitFor(
  assertion: () => void,
  { timeout = 1000, interval = 10 }: { timeout?: number; interval?: number } = {}
): Promise<void> {
  const startedAt = Date.now();
  let lastError: unknown;

  while (Date.now() - startedAt < timeout) {
    try {
      assertion();
      return;
    } catch (error) {
      lastError = error;
      await new Promise((resolve) => setTimeout(resolve, interval));
    }
  }

  throw lastError instanceof Error ? lastError : new Error('Timed out waiting for assertion.');
}
