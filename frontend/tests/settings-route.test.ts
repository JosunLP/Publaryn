/// <reference path="./bun-test.d.ts" />

import { afterEach, describe, expect, mock, test } from 'bun:test';

import { changeValue, click, renderSvelte, submitForm } from './svelte-dom';

type UserProfile = import('../src/api/auth').UserProfile;
type MfaSetupState = import('../src/api/auth').MfaSetupState;

interface Scenario {
  authToken: string | null;
  gotoCalls: string[];
  setupCalls: number;
  verifyCalls: string[];
  disableCalls: string[];
  user: UserProfile;
  setupState: MfaSetupState;
}

const authModuleUrl = new URL('../src/api/auth.ts', import.meta.url).href;
const clientModuleUrl = new URL('../src/api/client.ts', import.meta.url).href;
const orgsModuleUrl = new URL('../src/api/orgs.ts', import.meta.url).href;
const namespacesModuleUrl = new URL('../src/api/namespaces.ts', import.meta.url).href;
const tokensModuleUrl = new URL('../src/api/tokens.ts', import.meta.url).href;

let currentScenario: Scenario | null = null;

mock.module('$app/navigation', () => ({
  async goto(href: string): Promise<void> {
    currentScenario?.gotoCalls.push(href);
  },
}));

mock.module(clientModuleUrl, () => ({
  getAuthToken(): string | null {
    return currentScenario?.authToken ?? null;
  },
}));

mock.module(authModuleUrl, () => ({
  async getCurrentUser(): Promise<UserProfile> {
    return { ...(currentScenario?.user || {}) };
  },
  async updateCurrentUser(): Promise<UserProfile> {
    throw new Error('updateCurrentUser should not be called in MFA settings tests');
  },
  async setupMfa(): Promise<MfaSetupState> {
    if (!currentScenario) {
      throw new Error('Missing MFA settings scenario');
    }

    currentScenario.setupCalls += 1;
    return currentScenario.setupState;
  },
  async verifyMfaSetup(code: string): Promise<Record<string, unknown>> {
    if (!currentScenario) {
      throw new Error('Missing MFA settings scenario');
    }

    currentScenario.verifyCalls.push(code);
    currentScenario.user = { ...currentScenario.user, mfa_enabled: true };
    return { ok: true };
  },
  async disableMfa(code: string): Promise<Record<string, unknown>> {
    if (!currentScenario) {
      throw new Error('Missing MFA settings scenario');
    }

    currentScenario.disableCalls.push(code);
    currentScenario.user = { ...currentScenario.user, mfa_enabled: false };
    return { ok: true };
  },
}));

mock.module(tokensModuleUrl, () => ({
  async listTokens() {
    return { tokens: [] };
  },
  async createToken() {
    throw new Error('createToken should not be called in MFA settings tests');
  },
  async revokeToken() {
    throw new Error('revokeToken should not be called in MFA settings tests');
  },
}));

mock.module(orgsModuleUrl, () => ({
  async listMyOrganizations() {
    return { organizations: [], load_error: null };
  },
  async listMyInvitations() {
    return { invitations: [], load_error: null };
  },
  async acceptInvitation() {
    throw new Error('acceptInvitation should not be called in MFA settings tests');
  },
  async declineInvitation() {
    throw new Error('declineInvitation should not be called in MFA settings tests');
  },
  async createOrg() {
    throw new Error('createOrg should not be called in MFA settings tests');
  },
}));

mock.module(namespacesModuleUrl, () => ({
  async listUserNamespaces() {
    return { namespaces: [], load_error: null };
  },
  async deleteNamespaceClaim() {
    throw new Error('deleteNamespaceClaim should not be called in MFA settings tests');
  },
  async transferNamespaceClaim() {
    throw new Error('transferNamespaceClaim should not be called in MFA settings tests');
  },
}));

const SettingsPage = await import('../src/routes/settings/+page.svelte');

afterEach(() => {
  currentScenario = null;
});

describe('settings route MFA flows', () => {
  test('starts MFA setup, surfaces recovery state, and enables MFA after verification', async () => {
    currentScenario = createScenario({ mfaEnabled: false });

    const { target, unmount } = await renderSvelte(SettingsPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Multi-factor authentication');
        expect(normalizeWhitespace(target.textContent)).toContain('Status: Disabled');
      });

      click(queryRequiredButton(target, '#mfa-setup-btn'));

      await waitFor(() => {
        expect(target.textContent).toContain('MFA setup initialized. Verify one code to enable it.');
        expect(target.textContent).toContain('MANUALSECRET123');
        expect(target.textContent).toContain('otpauth://totp/Publaryn:alice');
        expect(target.textContent).toContain('recovery-a1');
        expect(target.textContent).toContain('recovery-b2');
      });

      changeValue(queryRequiredInput(target, '#mfa-verify-code'), '123456');
      submitForm(queryRequiredForm(target, '#mfa-verify-form'));

      await waitFor(() => {
        expect(target.textContent).toContain('MFA enabled successfully.');
        expect(normalizeWhitespace(target.textContent)).toContain('Status: Enabled');
        expect(target.querySelector('#mfa-disable-form')).not.toBeNull();
        expect(target.textContent).not.toContain('MANUALSECRET123');
      });

      expect(currentScenario.setupCalls).toBe(1);
      expect(currentScenario.verifyCalls).toEqual(['123456']);
    } finally {
      unmount();
    }
  });

  test('disables MFA with a recovery code and returns to setup mode', async () => {
    currentScenario = createScenario({ mfaEnabled: true });

    const { target, unmount } = await renderSvelte(SettingsPage.default);

    try {
      await waitFor(() => {
        expect(normalizeWhitespace(target.textContent)).toContain('Status: Enabled');
        expect(target.querySelector('#mfa-disable-form')).not.toBeNull();
      });

      changeValue(queryRequiredInput(target, '#mfa-disable-code'), 'recovery-a1');
      submitForm(queryRequiredForm(target, '#mfa-disable-form'));

      await waitFor(() => {
        expect(target.textContent).toContain('MFA disabled.');
        expect(normalizeWhitespace(target.textContent)).toContain('Status: Disabled');
        expect(target.querySelector('#mfa-setup-btn')).not.toBeNull();
      });

      expect(currentScenario.disableCalls).toEqual(['recovery-a1']);
    } finally {
      unmount();
    }
  });
});

function createScenario(options: { mfaEnabled: boolean }): Scenario {
  return {
    authToken: 'test-token',
    gotoCalls: [],
    setupCalls: 0,
    verifyCalls: [],
    disableCalls: [],
    user: {
      id: 'user-1',
      username: 'alice',
      email: 'alice@example.test',
      mfa_enabled: options.mfaEnabled,
    },
    setupState: {
      secret: 'MANUALSECRET123',
      provisioning_uri: 'otpauth://totp/Publaryn:alice?secret=MANUALSECRET123&issuer=Publaryn',
      recovery_codes: ['recovery-a1', 'recovery-b2'],
    },
  };
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
