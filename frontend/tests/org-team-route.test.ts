/// <reference path="./bun-test.d.ts" />

import { afterEach, describe, expect, mock, test } from 'bun:test';
import { writable } from 'svelte/store';

import { renderSvelte } from './svelte-dom';

type JsonRecord = Record<string, unknown>;

interface TestPageState {
  url: URL;
  params: Record<string, string>;
  route: { id: string | null };
  status: number;
  error: null;
  data: Record<string, never>;
  form: null;
}

interface Scenario {
  canManageTeams: boolean;
  canManageRepositories: boolean;
  canManageNamespaces: boolean;
  requests: string[];
}

const ORG_SLUG = 'source-org';
const TEAM_SLUG = 'release-engineering';
const apiClientModuleUrl = new URL('../src/api/client.ts', import.meta.url).href;
const pageStore = writable<TestPageState>(
  buildPageState(`https://example.test/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}`)
);
let currentScenario: Scenario | null = null;

mock.module('$app/stores', () => ({
  page: {
    subscribe: pageStore.subscribe,
  },
}));

mock.module(apiClientModuleUrl, () => {
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
      return 'test-token';
    },
    setAuthToken(): void {},
    clearAuthToken(): void {},
    onUnauthorized(): void {},
    api: {
      get: (path: string, options?: { query?: Record<string, unknown> }) =>
        handleApiRequest('GET', path, options),
      post: () => Promise.reject(new Error('Unexpected POST request')),
      put: () => Promise.reject(new Error('Unexpected PUT request')),
      patch: () => Promise.reject(new Error('Unexpected PATCH request')),
      delete: () => Promise.reject(new Error('Unexpected DELETE request')),
    },
  };
});

const TeamPage = await import(
  '../src/routes/orgs/[slug]/teams/[team_slug]/+page.svelte'
);

afterEach(() => {
  currentScenario = null;
  pageStore.set(buildPageState(`https://example.test/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}`));
});

describe('organization team workspace route', () => {
  test('renders delegated members and access summaries for administrators', async () => {
    currentScenario = createScenario();

    const { target, unmount } = await renderSvelte(TeamPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Release Engineering');
        expect(target.textContent).toContain('Owner User');
        expect(target.textContent).toContain('source-package');
        expect(target.textContent).toContain('Repository Alpha');
        expect(target.textContent).toContain('@source');
        expect(
          target.querySelector(
            `a[href="/orgs/${ORG_SLUG}#team-${TEAM_SLUG}"]`
          )
        ).not.toBeNull();
      });

      expect(currentScenario?.requests).toEqual([
        `/v1/orgs/${ORG_SLUG}`,
        `/v1/orgs/${ORG_SLUG}/teams`,
        `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/members`,
        `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access`,
        `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access`,
        `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/namespace-access`,
      ]);
    } finally {
      unmount();
    }
  });

  test('shows an authorization message before loading admin-only access details', async () => {
    currentScenario = createScenario({ canManageTeams: false });

    const { target, unmount } = await renderSvelte(TeamPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain(
          'Team workspaces are available to organization administrators.'
        );
      });

      expect(currentScenario?.requests).toEqual([
        `/v1/orgs/${ORG_SLUG}`,
        `/v1/orgs/${ORG_SLUG}/teams`,
      ]);
    } finally {
      unmount();
    }
  });
});

function createScenario(overrides: Partial<Scenario> = {}): Scenario {
  return {
    canManageTeams: true,
    canManageRepositories: true,
    canManageNamespaces: true,
    requests: [],
    ...overrides,
  };
}

async function handleApiRequest(
  method: string,
  path: string,
  options?: { query?: Record<string, unknown> }
): Promise<{ data: JsonRecord; requestId: null }> {
  void options;
  if (!currentScenario) {
    throw new Error('Missing test scenario.');
  }

  currentScenario.requests.push(path);

  if (method !== 'GET') {
    throw new Error(`Unexpected ${method} request to ${path}`);
  }

  if (path === `/v1/orgs/${ORG_SLUG}`) {
    return apiResponse({
      id: '11111111-1111-4111-8111-111111111111',
      slug: ORG_SLUG,
      name: 'Source Org',
      description: 'Organization for testing team workspaces.',
      capabilities: {
        can_manage: currentScenario.canManageTeams,
        can_manage_invitations: currentScenario.canManageTeams,
        can_manage_members: currentScenario.canManageTeams,
        can_manage_teams: currentScenario.canManageTeams,
        can_manage_repositories: currentScenario.canManageRepositories,
        can_manage_namespaces: currentScenario.canManageNamespaces,
        can_view_member_directory: true,
        can_view_audit_log: currentScenario.canManageTeams,
        can_transfer_ownership: currentScenario.canManageTeams,
      },
    });
  }

  if (path === `/v1/orgs/${ORG_SLUG}/teams`) {
    return apiResponse({
      teams: [
        {
          id: 'team-1',
          slug: TEAM_SLUG,
          name: 'Release Engineering',
          description: 'Manages publish and release flows.',
          created_at: '2026-04-01T00:00:00Z',
        },
      ],
    });
  }

  if (path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/members`) {
    return apiResponse({
      members: [
        {
          username: 'owner-user',
          display_name: 'Owner User',
          added_at: '2026-04-02T00:00:00Z',
        },
      ],
    });
  }

  if (path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access`) {
    return apiResponse({
      package_access: [
        {
          package_id: 'pkg-1',
          ecosystem: 'npm',
          name: 'source-package',
          permissions: ['publish', 'write_metadata'],
          granted_at: '2026-04-03T00:00:00Z',
        },
      ],
    });
  }

  if (path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access`) {
    return apiResponse({
      repository_access: [
        {
          repository_id: 'repo-1',
          name: 'Repository Alpha',
          slug: 'repo-alpha',
          kind: 'release',
          visibility: 'private',
          permissions: ['admin'],
          granted_at: '2026-04-03T00:00:00Z',
        },
      ],
    });
  }

  if (path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/namespace-access`) {
    return apiResponse({
      namespace_access: [
        {
          namespace_claim_id: 'namespace-1',
          ecosystem: 'npm',
          namespace: '@source',
          is_verified: true,
          permissions: ['transfer_ownership'],
          granted_at: '2026-04-03T00:00:00Z',
        },
      ],
    });
  }

  throw new Error(`Unhandled request: ${method} ${path}`);
}

function apiResponse(data: JsonRecord): { data: JsonRecord; requestId: null } {
  return {
    data,
    requestId: null,
  };
}

function buildPageState(href: string): TestPageState {
  const url = new URL(href);
  return {
    url,
    params: {
      slug: ORG_SLUG,
      team_slug: TEAM_SLUG,
    },
    route: { id: '/orgs/[slug]/teams/[team_slug]' },
    status: 200,
    error: null,
    data: {},
    form: null,
  };
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
