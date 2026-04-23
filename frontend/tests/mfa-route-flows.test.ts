/// <reference path="./bun-test.d.ts" />

import { afterEach, describe, expect, mock, test } from 'bun:test';

import { changeValue, click, renderSvelte, submitForm } from './svelte-dom';

interface TestPageState {
  url: URL;
  params: Record<string, string>;
  route: { id: string | null };
  status: number;
  error: null;
  data: Record<string, never>;
  form: null;
}

interface JsonRecord {
  [key: string]: unknown;
}

interface ApiRequestOptions {
  query?: Record<string, unknown>;
  body?: JsonRecord;
}

interface SettingsScenario {
  kind: 'settings';
  authToken: string | null;
  gotoCalls: string[];
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

type Scenario = SettingsScenario | LoginScenario;

const clientModuleUrl = new URL('../src/api/client.ts', import.meta.url).href;
const pageStore = {
  subscribe: (_callback: (value: TestPageState) => void) => () => {},
};

let currentScenario: Scenario | null = null;
let storedAuthToken: string | null = null;

mock.module('$app/stores', () => ({
  page: pageStore,
}));

mock.module('$app/navigation', () => ({
  async goto(href: string): Promise<void> {
    currentScenario?.gotoCalls.push(href);
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
      return currentScenario?.kind === 'settings'
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

mock.module(new URL('../src/lib/session.ts', import.meta.url).href, () => ({
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

const SettingsPagePath =
  '/home/runner/work/Publaryn/Publaryn/frontend/src/routes/settings/+page.svelte';
const LoginPagePath =
  '/home/runner/work/Publaryn/Publaryn/frontend/src/routes/login/+page.svelte';

afterEach(() => {
  currentScenario = null;
  storedAuthToken = null;
});

describe('settings route MFA flows', () => {
  test('starts MFA setup, surfaces recovery state, and enables MFA after verification', async () => {
    currentScenario = createSettingsScenario(false);

    const { target, unmount, flush } = await renderSvelte(SettingsPagePath);

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Multi-factor authentication');
        expect(normalizeWhitespace(target.textContent)).toContain('Status: Disabled');
      });

      click(queryRequiredButton(target, '#mfa-setup-btn'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('MFA setup initialized. Verify one code to enable it.');
        expect(target.textContent).toContain('MANUALSECRET123');
        expect(target.textContent).toContain('otpauth://totp/Publaryn:alice');
        expect(target.textContent).toContain('recovery-a1');
        expect(target.textContent).toContain('recovery-b2');
      });

      changeValue(queryRequiredInput(target, '#mfa-verify-code'), '123456');
      submitForm(queryRequiredForm(target, '#mfa-verify-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('MFA enabled successfully.');
        expect(normalizeWhitespace(target.textContent)).toContain('Status: Enabled');
        expect(target.querySelector('#mfa-disable-form')).not.toBeNull();
        expect(target.textContent).not.toContain('MANUALSECRET123');
      });

      const scenario = requireSettingsScenario();
      expect(scenario.setupCalls).toBe(1);
      expect(scenario.verifyCalls).toEqual(['123456']);
      expect(scenario.requests).toEqual([
        '/v1/users/me',
        '/v1/tokens',
        '/v1/users/me/organizations',
        '/v1/org-invitations',
        '/v1/namespaces',
        '/v1/auth/mfa/setup',
        '/v1/users/me',
        '/v1/tokens',
        '/v1/users/me/organizations',
        '/v1/org-invitations',
        '/v1/namespaces',
        '/v1/auth/mfa/verify-setup',
        '/v1/users/me',
        '/v1/tokens',
        '/v1/users/me/organizations',
        '/v1/org-invitations',
        '/v1/namespaces',
      ]);
    } finally {
      unmount();
    }
  });

  test('disables MFA with a recovery code and returns to setup mode', async () => {
    currentScenario = createSettingsScenario(true);

    const { target, unmount, flush } = await renderSvelte(SettingsPagePath);

    try {
      await waitFor(() => {
        flush();
        expect(normalizeWhitespace(target.textContent)).toContain('Status: Enabled');
        expect(target.querySelector('#mfa-disable-form')).not.toBeNull();
      });

      changeValue(queryRequiredInput(target, '#mfa-disable-code'), 'recovery-a1');
      submitForm(queryRequiredForm(target, '#mfa-disable-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('MFA disabled.');
        expect(normalizeWhitespace(target.textContent)).toContain('Status: Disabled');
        expect(target.querySelector('#mfa-setup-btn')).not.toBeNull();
      });

      const scenario = requireSettingsScenario();
      expect(scenario.disableCalls).toEqual(['recovery-a1']);
      expect(scenario.requests).toEqual([
        '/v1/users/me',
        '/v1/tokens',
        '/v1/users/me/organizations',
        '/v1/org-invitations',
        '/v1/namespaces',
        '/v1/auth/mfa/disable',
        '/v1/users/me',
        '/v1/tokens',
        '/v1/users/me/organizations',
        '/v1/org-invitations',
        '/v1/namespaces',
      ]);
    } finally {
      unmount();
    }
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

  if (currentScenario.kind === 'settings') {
    switch (`${method} ${path}`) {
      case 'GET /v1/users/me':
        return { data: { ...currentScenario.user } };
      case 'GET /v1/tokens':
        return { data: { tokens: [] } };
      case 'GET /v1/users/me/organizations':
        return { data: { organizations: [], load_error: null } };
      case 'GET /v1/org-invitations':
        return { data: { invitations: [], load_error: null } };
      case 'GET /v1/namespaces':
        return { data: { namespaces: [], load_error: null } };
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
        throw new Error(`Unhandled settings API request: ${method} ${path}`);
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
      throw new ApiError<ApiErrorBody>(currentScenario.challengeErrorStatus, {
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

function createSettingsScenario(mfaEnabled: boolean): SettingsScenario {
  return {
    kind: 'settings',
    authToken: 'test-token',
    gotoCalls: [],
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

function requireSettingsScenario(): SettingsScenario {
  if (!currentScenario || currentScenario.kind !== 'settings') {
    throw new Error('Expected a settings scenario');
  }

  return currentScenario;
}

function requireLoginScenario(): LoginScenario {
  if (!currentScenario || currentScenario.kind !== 'login') {
    throw new Error('Expected a login scenario');
  }

  return currentScenario;
}

function queryRequiredButton(target: HTMLElement, selector: string): HTMLButtonElement {
  const button = target.querySelector(selector);
  expect(button).not.toBeNull();
  return button as HTMLButtonElement;
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

function normalizeWhitespace(value: string | null | undefined): string {
  return (value || '').replace(/\s+/g, ' ').trim();
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
