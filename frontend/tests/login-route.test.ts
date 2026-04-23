/// <reference path="./bun-test.d.ts" />

import { afterEach, describe, expect, mock, test } from 'bun:test';

import { changeValue, renderSvelte, submitForm } from './svelte-dom';

interface AuthResponse {
  token?: string | null;
  mfa_token?: string | null;
}

interface ApiErrorBody {
  error?: string;
}

interface Scenario {
  gotoCalls: string[];
  loginCalls: Array<{ usernameOrEmail: string; password: string }>;
  challengeCalls: Array<{ mfaToken: string; code: string }>;
  syncAuthTokenCalls: number;
  loginResponse: AuthResponse;
  challengeResponse: AuthResponse;
  challengeErrorStatus: number | null;
}

const authModuleUrl = new URL('../src/api/auth.ts', import.meta.url).href;
const clientModuleUrl = new URL('../src/api/client.ts', import.meta.url).href;
const sessionModuleUrl = new URL('../src/lib/session.ts', import.meta.url).href;

let currentScenario: Scenario | null = null;

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

  return { ApiError };
});

mock.module(authModuleUrl, () => ({
  async login(input: {
    usernameOrEmail: string;
    password: string;
  }): Promise<AuthResponse> {
    if (!currentScenario) {
      throw new Error('Missing login scenario');
    }

    currentScenario.loginCalls.push(input);
    return currentScenario.loginResponse;
  },
  async completeMfaChallenge(input: {
    mfaToken: string;
    code: string;
  }): Promise<AuthResponse> {
    if (!currentScenario) {
      throw new Error('Missing login scenario');
    }

    currentScenario.challengeCalls.push(input);
    if (currentScenario.challengeErrorStatus != null) {
      const { ApiError } = await import(clientModuleUrl);
      throw new ApiError<ApiErrorBody>(currentScenario.challengeErrorStatus, {
        error: 'Invalid challenge',
      });
    }
    return currentScenario.challengeResponse;
  },
}));

mock.module(sessionModuleUrl, () => ({
  syncAuthToken(): void {
    if (currentScenario) {
      currentScenario.syncAuthTokenCalls += 1;
    }
  },
}));

const LoginPage = await import('../src/routes/login/+page.svelte');

afterEach(() => {
  currentScenario = null;
});

describe('login route MFA challenge flow', () => {
  test('transitions from password login to MFA challenge and completes sign-in', async () => {
    currentScenario = {
      gotoCalls: [],
      loginCalls: [],
      challengeCalls: [],
      syncAuthTokenCalls: 0,
      loginResponse: { mfa_token: 'pending-mfa-token' },
      challengeResponse: { token: 'session-token' },
      challengeErrorStatus: null,
    };

    const { target, unmount } = await renderSvelte(LoginPage.default);

    try {
      changeValue(queryRequiredInput(target, '#login-username'), 'alice');
      changeValue(queryRequiredInput(target, '#login-password'), 'super-secret');
      submitForm(queryRequiredForm(target, '#login-form'));

      await waitFor(() => {
        expect(target.textContent).toContain('Complete verification');
        expect(target.querySelector('#mfa-form')).not.toBeNull();
      });

      changeValue(queryRequiredInput(target, '#mfa-code'), '654321');
      submitForm(queryRequiredForm(target, '#mfa-form'));

      await waitFor(() => {
        expect(currentScenario?.gotoCalls).toEqual(['/']);
      });

      expect(currentScenario.loginCalls).toEqual([
        { usernameOrEmail: 'alice', password: 'super-secret' },
      ]);
      expect(currentScenario.challengeCalls).toEqual([
        { mfaToken: 'pending-mfa-token', code: '654321' },
      ]);
      expect(currentScenario.syncAuthTokenCalls).toBe(1);
    } finally {
      unmount();
    }
  });

  test('keeps the MFA challenge open when a recovery code is rejected', async () => {
    currentScenario = {
      gotoCalls: [],
      loginCalls: [],
      challengeCalls: [],
      syncAuthTokenCalls: 0,
      loginResponse: { mfa_token: 'pending-mfa-token' },
      challengeResponse: {},
      challengeErrorStatus: 401,
    };

    const { target, unmount } = await renderSvelte(LoginPage.default);

    try {
      changeValue(queryRequiredInput(target, '#login-username'), 'alice');
      changeValue(queryRequiredInput(target, '#login-password'), 'super-secret');
      submitForm(queryRequiredForm(target, '#login-form'));

      await waitFor(() => {
        expect(target.textContent).toContain('Complete verification');
      });

      changeValue(queryRequiredInput(target, '#mfa-code'), 'recovery-a1');
      submitForm(queryRequiredForm(target, '#mfa-form'));

      await waitFor(() => {
        expect(target.textContent).toContain('The MFA code is invalid or expired.');
      });

      expect(currentScenario.challengeCalls).toEqual([
        { mfaToken: 'pending-mfa-token', code: 'recovery-a1' },
      ]);
      expect(currentScenario.gotoCalls).toEqual([]);
      expect(currentScenario.syncAuthTokenCalls).toBe(0);
    } finally {
      unmount();
    }
  });
});

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
