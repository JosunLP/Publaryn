/// <reference path="./bun-test.d.ts" />

import { afterEach, describe, expect, mock, test } from 'bun:test';

import {
  changeValue,
  click,
  renderSvelte,
  setChecked,
  submitForm,
} from './svelte-dom';

import type {
  SettingsPageLoaders,
  SettingsPageTokenActions,
} from '../src/pages/settings-page';
import type { TokenRecord } from '../src/api/tokens';

const HarnessPath =
  '/home/runner/work/Publaryn/Publaryn/frontend/tests/fixtures/settings-page-harness.svelte';

const gotoCalls: Array<{ href: string; replaceState?: boolean }> = [];

mock.module('$app/navigation', () => ({
  async goto(
    href: string,
    options?: { replaceState?: boolean }
  ): Promise<void> {
    gotoCalls.push({ href, replaceState: options?.replaceState });
  },
}));

afterEach(() => {
  gotoCalls.length = 0;
});

describe('settings page controller harness', () => {
  test('redirects unauthenticated visitors to login before loading settings', async () => {
    let getCurrentUserCalls = 0;
    const loaders = createLoaders({
      async getCurrentUser() {
        getCurrentUserCalls += 1;
        return buildUser();
      },
    });

    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      authToken: null,
      loaders,
      tokenActions: createTokenActions(),
    });

    try {
      await waitFor(() => {
        flush();
        expect(gotoCalls).toEqual([{ href: '/login', replaceState: true }]);
      });
      expect(getCurrentUserCalls).toBe(0);
    } finally {
      unmount();
    }
  });

  test('creates a token, resets the form, and reloads the token list', async () => {
    const scenario = createTokenScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loaders: createLoaders({
        async listTokens() {
          return {
            tokens: scenario.tokens.map((token) => ({ ...token })),
          };
        },
      }),
      tokenActions: createTokenActions({
        async createToken(input) {
          scenario.createCalls.push({
            name: input.name,
            scopes: [...input.scopes],
            expires_in_days: input.expires_in_days ?? null,
          });
          scenario.tokens = [
            ...scenario.tokens,
            {
              id: 'token-2',
              name: input.name,
              kind: 'personal',
              created_at: '2026-04-24T00:00:00Z',
              last_used_at: null,
              expires_at: input.expires_in_days
                ? '2026-05-24T00:00:00Z'
                : null,
              scopes: [...input.scopes],
              prefix: 'pub_live',
            },
          ];
          return { token: 'pub_secret_created_token' };
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('bootstrap-token');
      });

      changeValue(queryRequiredInput(target, '#token-name'), 'CI deploy token');
      changeValue(queryRequiredInput(target, '#token-expiry'), '30');
      setChecked(queryScopeCheckbox(target, 'tokens:write'), false);
      submitForm(queryRequiredForm(target, '#token-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Token created successfully.');
        expect(target.textContent).toContain('pub_secret_created_token');
        expect(target.textContent).toContain('CI deploy token');
      });

      expect(queryRequiredInput(target, '#token-name').value).toBe('');
      expect(queryRequiredInput(target, '#token-expiry').value).toBe('');
      expect(queryScopeCheckbox(target, 'tokens:read').checked).toBe(true);
      expect(queryScopeCheckbox(target, 'tokens:write').checked).toBe(true);
      expect(scenario.createCalls).toEqual([
        {
          name: 'CI deploy token',
          scopes: ['tokens:read'],
          expires_in_days: 30,
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('revokes an existing token and reloads the active token list', async () => {
    const scenario = createTokenScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loaders: createLoaders({
        async listTokens() {
          return {
            tokens: scenario.tokens.map((token) => ({ ...token })),
          };
        },
      }),
      tokenActions: createTokenActions({
        async revokeToken(tokenId) {
          scenario.revokeCalls.push(tokenId);
          scenario.tokens = scenario.tokens.filter((token) => token.id !== tokenId);
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('bootstrap-token');
      });

      click(queryButtonByText(target, 'Revoke'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Token revoked.');
        expect(target.textContent).not.toContain('bootstrap-token');
        expect(normalizeWhitespace(target.textContent)).toContain('No tokens yet');
      });

      expect(scenario.revokeCalls).toEqual(['token-1']);
    } finally {
      unmount();
    }
  });
});

function createLoaders(
  overrides: Partial<SettingsPageLoaders> = {}
): SettingsPageLoaders {
  return {
    async getCurrentUser() {
      return buildUser();
    },
    async listTokens() {
      return {
        tokens: [
          {
            id: 'token-1',
            name: 'bootstrap-token',
            kind: 'personal',
            created_at: '2026-04-20T00:00:00Z',
            last_used_at: null,
            expires_at: null,
            scopes: ['tokens:read', 'tokens:write'],
            prefix: 'pub_boot',
          },
        ],
      };
    },
    async listMyOrganizations() {
      return {
        organizations: [],
        load_error: null,
      };
    },
    async listMyInvitations() {
      return {
        invitations: [],
        load_error: null,
      };
    },
    async listUserNamespaces() {
      return {
        namespaces: [],
        load_error: null,
      };
    },
    ...overrides,
  };
}

function createTokenActions(
  overrides: Partial<SettingsPageTokenActions> = {}
): SettingsPageTokenActions {
  return {
    async createToken() {
      return { token: 'pub_default' };
    },
    async revokeToken() {},
    ...overrides,
  };
}

function createTokenScenario() {
  return {
    tokens: [
      {
        id: 'token-1',
        name: 'bootstrap-token',
        kind: 'personal',
        created_at: '2026-04-20T00:00:00Z',
        last_used_at: null,
        expires_at: null,
        scopes: ['tokens:read', 'tokens:write'],
        prefix: 'pub_boot',
      },
    ] as TokenRecord[],
    createCalls: [] as Array<{
      name: string;
      scopes: string[];
      expires_in_days: number | null;
    }>,
    revokeCalls: [] as string[],
  };
}

function buildUser() {
  return {
    id: 'user-1',
    username: 'alice',
    email: 'alice@example.test',
    display_name: 'Alice',
    avatar_url: '',
    website: '',
    bio: '',
    mfa_enabled: false,
  };
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

function queryScopeCheckbox(target: HTMLElement, scope: string): HTMLInputElement {
  const label = Array.from(target.querySelectorAll('label')).find((candidate) =>
    candidate.textContent?.includes(scope)
  );
  expect(label).toBeDefined();
  const checkbox = label?.querySelector('input[type="checkbox"]');
  expect(checkbox).not.toBeNull();
  return checkbox as HTMLInputElement;
}

function queryButtonByText(target: HTMLElement, text: string): HTMLButtonElement {
  const button = Array.from(target.querySelectorAll('button')).find((candidate) =>
    candidate.textContent?.includes(text)
  );
  expect(button).toBeDefined();
  return button as HTMLButtonElement;
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
