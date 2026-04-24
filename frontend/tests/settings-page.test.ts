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
  SettingsPageOrganizationActions,
  SettingsPageProfileActions,
  SettingsPageTokenActions,
} from '../src/pages/settings-page';
import type { UserProfile } from '../src/api/auth';
import type { MyInvitation, OrganizationMembership } from '../src/api/orgs';
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
      profileActions: createProfileActions(),
      organizationActions: createOrganizationActions(),
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
      profileActions: createProfileActions(),
      organizationActions: createOrganizationActions(),
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
      profileActions: createProfileActions(),
      organizationActions: createOrganizationActions(),
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

  test('saves the profile and reloads refreshed profile details', async () => {
    const scenario = createProfileScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loaders: createLoaders({
        async getCurrentUser() {
          return { ...scenario.user };
        },
      }),
      tokenActions: createTokenActions(),
      profileActions: createProfileActions({
        async updateCurrentUser(updates) {
          scenario.updateCalls.push(updates);
          scenario.user = {
            ...scenario.user,
            ...updates,
          };
          return { ...scenario.user };
        },
      }),
      organizationActions: createOrganizationActions(),
    });

    try {
      await waitFor(() => {
        flush();
        expect(queryRequiredInput(target, '#settings-display-name').value).toBe(
          'Alice'
        );
      });

      changeValue(queryRequiredInput(target, '#settings-display-name'), 'Alice Admin');
      changeValue(
        queryRequiredInput(target, '#settings-avatar-url'),
        'https://example.test/avatar.png'
      );
      changeValue(
        queryRequiredInput(target, '#settings-website'),
        'https://alice.example.test'
      );
      changeValue(
        queryRequiredTextArea(target, '#settings-bio'),
        'Maintains Publaryn settings.'
      );
      submitForm(queryRequiredForm(target, '#profile-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Profile updated successfully.');
        expect(queryRequiredInput(target, '#settings-display-name').value).toBe(
          'Alice Admin'
        );
      });

      expect(scenario.updateCalls).toEqual([
        {
          display_name: 'Alice Admin',
          avatar_url: 'https://example.test/avatar.png',
          website: 'https://alice.example.test',
          bio: 'Maintains Publaryn settings.',
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('creates an organization, resets the form, and reloads organization data', async () => {
    const scenario = createOrganizationScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loaders: createLoaders({
        async listMyOrganizations() {
          return {
            organizations: scenario.organizations.map((organization) => ({
              ...organization,
            })),
            load_error: null,
          };
        },
      }),
      tokenActions: createTokenActions(),
      profileActions: createProfileActions(),
      organizationActions: createOrganizationActions({
        async createOrg(input) {
          scenario.createCalls.push({
            ...input,
          });
          scenario.organizations = [
            ...scenario.organizations,
            {
              slug: input.slug,
              name: input.name,
              description: input.description,
              website: input.website,
              email: input.email,
              role: 'owner',
              package_count: 0,
              team_count: 0,
              joined_at: '2026-04-24T00:00:00Z',
            },
          ];
          return {
            slug: input.slug,
            name: input.name,
          };
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(normalizeWhitespace(target.textContent)).toContain(
          'No pending invitations'
        );
      });

      changeValue(queryRequiredInput(target, '#org-name'), 'Acme Platform');
      await waitFor(() => {
        flush();
        expect(queryRequiredInput(target, '#org-slug').value).toBe('acme-platform');
      });
      changeValue(
        queryRequiredTextArea(target, '#org-description'),
        'Publishes adapters'
      );
      changeValue(
        queryRequiredInput(target, '#org-website'),
        'https://acme.example.test'
      );
      changeValue(
        queryRequiredInput(target, '#org-email'),
        'packages@acme.example.test'
      );
      submitForm(queryRequiredForm(target, '#org-create-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Organization created successfully. Slug: acme-platform.'
        );
      });

      expect(queryRequiredInput(target, '#org-name').value).toBe('');
      expect(queryRequiredInput(target, '#org-slug').value).toBe('');
      expect(queryRequiredInput(target, '#org-website').value).toBe('');
      expect(queryRequiredInput(target, '#org-email').value).toBe('');
      expect(queryRequiredTextArea(target, '#org-description').value).toBe('');
      expect(scenario.createCalls).toEqual([
        {
          name: 'Acme Platform',
          slug: 'acme-platform',
          description: 'Publishes adapters',
          website: 'https://acme.example.test',
          email: 'packages@acme.example.test',
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('accepts and declines invitations and reloads the invitation list', async () => {
    const scenario = createInvitationScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loaders: createLoaders({
        async listMyInvitations() {
          return {
            invitations: scenario.invitations.map((invitation) => ({ ...invitation })),
            load_error: null,
          };
        },
      }),
      tokenActions: createTokenActions(),
      profileActions: createProfileActions(),
      organizationActions: createOrganizationActions({
        async acceptInvitation(invitationId) {
          scenario.acceptCalls.push(invitationId);
          const invitation = scenario.invitations.find(
            (candidate) => candidate.id === invitationId
          );
          scenario.invitations = scenario.invitations.filter(
            (candidate) => candidate.id !== invitationId
          );
          return {
            role: invitation?.role || 'member',
            org: invitation?.org || null,
          };
        },
        async declineInvitation(invitationId) {
          scenario.declineCalls.push(invitationId);
          scenario.invitations = scenario.invitations.filter(
            (candidate) => candidate.id !== invitationId
          );
          return {};
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Docs Team');
        expect(target.textContent).toContain('Build Team');
      });

      click(queryButtonByText(target, 'Accept'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Invitation accepted. You are now maintainer in Docs Team.'
        );
        expect(
          target.querySelector('[data-test="invitation-invite-1"]')
        ).toBeNull();
        expect(
          target.querySelector('[data-test="invitation-invite-2"]')
        ).not.toBeNull();
        expect(target.textContent).toContain('Build Team');
      });

      click(queryButtonByText(target, 'Decline'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Invitation declined.');
        expect(normalizeWhitespace(target.textContent)).toContain(
          'No pending invitations'
        );
      });

      expect(scenario.acceptCalls).toEqual(['invite-1']);
      expect(scenario.declineCalls).toEqual(['invite-2']);
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

function createProfileActions(
  overrides: Partial<SettingsPageProfileActions> = {}
): SettingsPageProfileActions {
  return {
    async updateCurrentUser(updates) {
      return {
        ...buildUser(),
        ...updates,
      };
    },
    ...overrides,
  };
}

function createOrganizationActions(
  overrides: Partial<SettingsPageOrganizationActions> = {}
): SettingsPageOrganizationActions {
  return {
    async createOrg(input) {
      return {
        slug: input.slug,
        name: input.name,
      };
    },
    async acceptInvitation() {
      return {
        role: 'member',
        org: { name: 'Example Org', slug: 'example-org' },
      };
    },
    async declineInvitation() {
      return {};
    },
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

function createProfileScenario() {
  return {
    user: buildUser(),
    updateCalls: [] as Array<Record<string, unknown>>,
  };
}

function createOrganizationScenario() {
  return {
    organizations: [] as OrganizationMembership[],
    createCalls: [] as Array<Record<string, unknown>>,
  };
}

function createInvitationScenario() {
  return {
    invitations: [
      {
        id: 'invite-1',
        org: { name: 'Docs Team', slug: 'docs-team' },
        role: 'maintainer',
        invited_by: { username: 'owner-user' },
        created_at: '2026-04-20T00:00:00Z',
        expires_at: '2026-05-20T00:00:00Z',
        status: 'pending',
        actionable: true,
      },
      {
        id: 'invite-2',
        org: { name: 'Build Team', slug: 'build-team' },
        role: 'viewer',
        invited_by: { username: 'owner-user' },
        created_at: '2026-04-21T00:00:00Z',
        expires_at: '2026-05-21T00:00:00Z',
        status: 'pending',
        actionable: true,
      },
    ] as MyInvitation[],
    acceptCalls: [] as string[],
    declineCalls: [] as string[],
  };
}

function buildUser(): UserProfile {
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

function queryRequiredTextArea(
  target: HTMLElement,
  selector: string
): HTMLTextAreaElement {
  const textarea = target.querySelector(selector);
  expect(textarea).not.toBeNull();
  return textarea as HTMLTextAreaElement;
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
