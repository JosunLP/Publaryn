/// <reference path="./bun-test.d.ts" />

import { afterEach, describe, expect, mock, test } from 'bun:test';
import { writable } from 'svelte/store';

import {
  changeValue,
  click,
  renderSvelte,
  setChecked,
  submitForm,
} from './svelte-dom';

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

interface MutationCall {
  path: string;
  body?: JsonRecord;
}

interface Scenario {
  canManageTeams: boolean;
  canManageRepositories: boolean;
  canManageNamespaces: boolean;
  requests: string[];
  patchCalls: MutationCall[];
  postCalls: MutationCall[];
  putCalls: MutationCall[];
  deleteCalls: MutationCall[];
  team: {
    id: string;
    slug: string;
    name: string;
    description: string;
    created_at: string;
  };
  orgMembers: Array<{
    user_id: string;
    username: string;
    display_name: string;
    role: string;
    joined_at: string;
  }>;
  teamMembers: Array<{
    username: string;
    display_name: string;
    added_at: string;
  }>;
  orgPackages: Array<{
    package_id: string;
    ecosystem: string;
    name: string;
  }>;
  packageAccess: Array<{
    package_id: string;
    ecosystem: string;
    name: string;
    permissions: string[];
    granted_at: string;
  }>;
  orgRepositories: Array<{
    repository_id: string;
    name: string;
    slug: string;
    kind: string;
    visibility: string;
  }>;
  repositoryAccess: Array<{
    repository_id: string;
    name: string;
    slug: string;
    kind: string;
    visibility: string;
    permissions: string[];
    granted_at: string;
  }>;
  orgNamespaces: Array<{
    id: string;
    ecosystem: string;
    namespace: string;
    is_verified: boolean;
  }>;
  namespaceAccess: Array<{
    namespace_claim_id: string;
    ecosystem: string;
    namespace: string;
    is_verified: boolean;
    permissions: string[];
    granted_at: string;
  }>;
}

const ORG_ID = '11111111-1111-4111-8111-111111111111';
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

const TeamPage = await import(
  '../src/routes/orgs/[slug]/teams/[team_slug]/+page.svelte'
);

afterEach(() => {
  currentScenario = null;
  pageStore.set(buildPageState(`https://example.test/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}`));
});

describe('organization team workspace route', () => {
  test('renders editable team details and delegated access controls for administrators', async () => {
    currentScenario = createScenario();

    const { target, unmount } = await renderSvelte(TeamPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Release Engineering');
        expect(target.textContent).toContain('Owner User');
        expect(target.textContent).toContain('source-package');
        expect(target.textContent).toContain('Repository Alpha');
        expect(target.textContent).toContain('@source');
        expect(queryRequiredForm(target, '#team-settings-form')).toBeDefined();
        expect(queryRequiredForm(target, '#team-member-form')).toBeDefined();
        expect(queryRequiredSelect(target, '#team-package-access')).toBeDefined();
        expect(queryRequiredSelect(target, '#team-repository-access')).toBeDefined();
        expect(queryRequiredSelect(target, '#team-namespace-access')).toBeDefined();
      });

      expect(currentScenario?.requests).toEqual([
        `/v1/orgs/${ORG_SLUG}`,
        `/v1/orgs/${ORG_SLUG}/teams`,
        `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/members`,
        `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access`,
        `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access`,
        `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/namespace-access`,
        `/v1/orgs/${ORG_SLUG}/members`,
        `/v1/orgs/${ORG_SLUG}/packages`,
        `/v1/orgs/${ORG_SLUG}/repositories`,
        '/v1/namespaces',
      ]);
    } finally {
      unmount();
    }
  });

  test('updates team details and manages team members directly from the dedicated page', async () => {
    currentScenario = createScenario();

    const { target, unmount } = await renderSvelte(TeamPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Release Engineering');
      });

      changeValue(queryRequiredInput(target, '#team-name'), 'Platform Releases');
      changeValue(
        queryRequiredTextArea(target, '#team-description'),
        'Owns release automation.'
      );
      submitForm(queryRequiredForm(target, '#team-settings-form'));

      await waitFor(() => {
        expect(target.textContent).toContain('Saved changes to release-engineering.');
        expect(target.textContent).toContain('Platform Releases');
        expect(target.textContent).toContain('Owns release automation.');
      });

      changeValue(queryRequiredInput(target, '#team-member-input'), 'admin-user');
      submitForm(queryRequiredForm(target, '#team-member-form'));

      await waitFor(() => {
        expect(target.textContent).toContain('Added a member to release-engineering.');
        expect(target.textContent).toContain('Admin User');
      });

      click(queryRequiredButton(target, '#team-member-remove-admin-user'));

      await waitFor(() => {
        expect(target.textContent).toContain('Removed @admin-user from release-engineering.');
        expect(target.textContent).toContain('Eligible members 1');
        expect(
          target.querySelector('#team-member-remove-admin-user')
        ).toBeNull();
      });

      expect(currentScenario?.patchCalls).toEqual([
        {
          path: `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}`,
          body: {
            name: 'Platform Releases',
            description: 'Owns release automation.',
          },
        },
      ]);
      expect(currentScenario?.postCalls).toContainEqual({
        path: `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/members`,
        body: {
          username: 'admin-user',
        },
      });
      expect(currentScenario?.deleteCalls).toContainEqual({
        path: `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/members/admin-user`,
      });
    } finally {
      unmount();
    }
  });

  test('updates and revokes delegated package, repository, and namespace access from the team page', async () => {
    currentScenario = createScenario();

    const { target, unmount } = await renderSvelte(TeamPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('source-package');
      });

      const packageAccessForm = queryClosestForm(
        queryRequiredSelect(target, '#team-package-access')
      );
      changeValue(queryRequiredSelect(target, '#team-package-access'), 'npm:new-package');
      setChecked(
        queryRequiredCheckbox(
          packageAccessForm,
          'input[name="permissions"][value="publish"]'
        ),
        true
      );
      submitForm(packageAccessForm);

      await waitFor(() => {
        expect(target.textContent).toContain('Saved package access for new-package.');
        expect(target.textContent).toContain('new-package');
      });

      const repositoryAccessForm = queryClosestForm(
        queryRequiredSelect(target, '#team-repository-access')
      );
      changeValue(queryRequiredSelect(target, '#team-repository-access'), 'repo-beta');
      setChecked(
        queryRequiredCheckbox(
          repositoryAccessForm,
          'input[name="permissions"][value="admin"]'
        ),
        true
      );
      submitForm(repositoryAccessForm);

      await waitFor(() => {
        expect(target.textContent).toContain('Saved repository access for repo-beta.');
        expect(target.textContent).toContain('Repository Beta');
      });

      const namespaceAccessForm = queryClosestForm(
        queryRequiredSelect(target, '#team-namespace-access')
      );
      changeValue(queryRequiredSelect(target, '#team-namespace-access'), 'namespace-2');
      setChecked(
        queryRequiredCheckbox(
          namespaceAccessForm,
          'input[name="permissions"][value="transfer_ownership"]'
        ),
        true
      );
      submitForm(namespaceAccessForm);

      await waitFor(() => {
        expect(target.textContent).toContain('Saved namespace access for @target.');
        expect(target.textContent).toContain('@target');
      });

      click(
        queryRequiredButton(target, '#team-package-revoke-npm-source-package')
      );
      await waitFor(() => {
        expect(target.textContent).toContain('Revoked package access for source-package.');
      });

      click(queryRequiredButton(target, '#team-repository-revoke-repo-alpha'));
      await waitFor(() => {
        expect(target.textContent).toContain('Revoked repository access for repo-alpha.');
      });

      click(queryRequiredButton(target, '#team-namespace-revoke-namespace-1'));
      await waitFor(() => {
        expect(target.textContent).toContain('Revoked namespace access for @source.');
      });

      expect(currentScenario?.putCalls).toContainEqual({
        path: `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access/npm/new-package`,
        body: {
          permissions: ['publish'],
        },
      });
      expect(currentScenario?.putCalls).toContainEqual({
        path: `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access/repo-beta`,
        body: {
          permissions: ['admin'],
        },
      });
      expect(currentScenario?.putCalls).toContainEqual({
        path: `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/namespace-access/namespace-2`,
        body: {
          permissions: ['transfer_ownership'],
        },
      });
      expect(currentScenario?.deleteCalls).toContainEqual({
        path: `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access/npm/source-package`,
      });
      expect(currentScenario?.deleteCalls).toContainEqual({
        path: `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access/repo-alpha`,
      });
      expect(currentScenario?.deleteCalls).toContainEqual({
        path: `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/namespace-access/namespace-1`,
      });
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

interface ApiRequestOptions {
  query?: Record<string, unknown>;
  body?: JsonRecord;
}

function createScenario(overrides: Partial<Scenario> = {}): Scenario {
  return {
    canManageTeams: true,
    canManageRepositories: true,
    canManageNamespaces: true,
    requests: [],
    patchCalls: [],
    postCalls: [],
    putCalls: [],
    deleteCalls: [],
    team: {
      id: 'team-1',
      slug: TEAM_SLUG,
      name: 'Release Engineering',
      description: 'Manages publish and release flows.',
      created_at: '2026-04-01T00:00:00Z',
    },
    orgMembers: [
      {
        user_id: 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa',
        username: 'owner-user',
        display_name: 'Owner User',
        role: 'owner',
        joined_at: '2026-04-01T00:00:00Z',
      },
      {
        user_id: 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb',
        username: 'admin-user',
        display_name: 'Admin User',
        role: 'admin',
        joined_at: '2026-04-02T00:00:00Z',
      },
    ],
    teamMembers: [
      {
        username: 'owner-user',
        display_name: 'Owner User',
        added_at: '2026-04-02T00:00:00Z',
      },
    ],
    orgPackages: [
      {
        package_id: 'pkg-1',
        ecosystem: 'npm',
        name: 'source-package',
      },
      {
        package_id: 'pkg-2',
        ecosystem: 'npm',
        name: 'new-package',
      },
    ],
    packageAccess: [
      {
        package_id: 'pkg-1',
        ecosystem: 'npm',
        name: 'source-package',
        permissions: ['publish', 'write_metadata'],
        granted_at: '2026-04-03T00:00:00Z',
      },
    ],
    orgRepositories: [
      {
        repository_id: 'repo-1',
        name: 'Repository Alpha',
        slug: 'repo-alpha',
        kind: 'release',
        visibility: 'private',
      },
      {
        repository_id: 'repo-2',
        name: 'Repository Beta',
        slug: 'repo-beta',
        kind: 'release',
        visibility: 'private',
      },
    ],
    repositoryAccess: [
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
    orgNamespaces: [
      {
        id: 'namespace-1',
        ecosystem: 'npm',
        namespace: '@source',
        is_verified: true,
      },
      {
        id: 'namespace-2',
        ecosystem: 'npm',
        namespace: '@target',
        is_verified: true,
      },
    ],
    namespaceAccess: [
      {
        namespace_claim_id: 'namespace-1',
        ecosystem: 'npm',
        namespace: '@source',
        is_verified: true,
        permissions: ['transfer_ownership'],
        granted_at: '2026-04-03T00:00:00Z',
      },
    ],
    ...overrides,
  };
}

async function handleApiRequest(
  method: string,
  path: string,
  options?: ApiRequestOptions
): Promise<{ data: JsonRecord; requestId: null }> {
  if (!currentScenario) {
    throw new Error('Missing test scenario.');
  }

  if (method === 'GET') {
    currentScenario.requests.push(path);
  }

  if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}`) {
    return apiResponse({
      id: ORG_ID,
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

  if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/teams`) {
    return apiResponse({
      teams: [currentScenario.team],
    });
  }

  if (method === 'PATCH' && path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}`) {
    currentScenario.patchCalls.push({
      path,
      body: options?.body,
    });
    currentScenario.team = {
      ...currentScenario.team,
      name: String(options?.body?.name || currentScenario.team.name),
      description: String(options?.body?.description || ''),
    };
    return apiResponse({ message: 'Team updated' });
  }

  if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/members`) {
    return apiResponse({
      members: currentScenario.teamMembers,
    });
  }

  if (method === 'POST' && path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/members`) {
    currentScenario.postCalls.push({
      path,
      body: options?.body,
    });
    const username = String(options?.body?.username || '');
    const orgMember = currentScenario.orgMembers.find(
      (member) => member.username === username
    );
    if (orgMember) {
      currentScenario.teamMembers = [
        ...currentScenario.teamMembers,
        {
          username: orgMember.username,
          display_name: orgMember.display_name,
          added_at: '2026-04-05T00:00:00Z',
        },
      ];
    }
    return apiResponse({ message: 'Team member added' });
  }

  if (
    method === 'DELETE' &&
    path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/members/admin-user`
  ) {
    currentScenario.deleteCalls.push({ path });
    currentScenario.teamMembers = currentScenario.teamMembers.filter(
      (member) => member.username !== 'admin-user'
    );
    return apiResponse({ message: 'Team member removed' });
  }

  if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/members`) {
    return apiResponse({
      members: currentScenario.orgMembers,
    });
  }

  if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/packages`) {
    return apiResponse({
      packages: currentScenario.orgPackages.map((pkg) => ({
        id: pkg.package_id,
        ecosystem: pkg.ecosystem,
        name: pkg.name,
      })),
    });
  }

  if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access`) {
    return apiResponse({
      package_access: currentScenario.packageAccess,
    });
  }

  if (
    method === 'PUT' &&
    path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access/npm/new-package`
  ) {
    currentScenario.putCalls.push({
      path,
      body: options?.body,
    });
    currentScenario.packageAccess = [
      ...currentScenario.packageAccess,
      {
        package_id: 'pkg-2',
        ecosystem: 'npm',
        name: 'new-package',
        permissions: ['publish'],
        granted_at: '2026-04-06T00:00:00Z',
      },
    ];
    return apiResponse({
      message: 'Team package access updated',
      package: {
        ecosystem: 'npm',
        name: 'new-package',
      },
      permissions: ['publish'],
    });
  }

  if (
    method === 'DELETE' &&
    path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access/npm/source-package`
  ) {
    currentScenario.deleteCalls.push({ path });
    currentScenario.packageAccess = currentScenario.packageAccess.filter(
      (grant) => grant.name !== 'source-package'
    );
    return apiResponse({ message: 'Team package access removed' });
  }

  if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/repositories`) {
    return apiResponse({
      repositories: currentScenario.orgRepositories.map((repository) => ({
        id: repository.repository_id,
        name: repository.name,
        slug: repository.slug,
        kind: repository.kind,
        visibility: repository.visibility,
      })),
    });
  }

  if (
    method === 'GET' &&
    path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access`
  ) {
    return apiResponse({
      repository_access: currentScenario.repositoryAccess,
    });
  }

  if (
    method === 'PUT' &&
    path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access/repo-beta`
  ) {
    currentScenario.putCalls.push({
      path,
      body: options?.body,
    });
    currentScenario.repositoryAccess = [
      ...currentScenario.repositoryAccess,
      {
        repository_id: 'repo-2',
        name: 'Repository Beta',
        slug: 'repo-beta',
        kind: 'release',
        visibility: 'private',
        permissions: ['admin'],
        granted_at: '2026-04-06T00:00:00Z',
      },
    ];
    return apiResponse({
      message: 'Team repository access updated',
      repository: {
        slug: 'repo-beta',
      },
      permissions: ['admin'],
    });
  }

  if (
    method === 'DELETE' &&
    path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access/repo-alpha`
  ) {
    currentScenario.deleteCalls.push({ path });
    currentScenario.repositoryAccess = currentScenario.repositoryAccess.filter(
      (grant) => grant.slug !== 'repo-alpha'
    );
    return apiResponse({ message: 'Team repository access removed' });
  }

  if (
    method === 'GET' &&
    path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/namespace-access`
  ) {
    return apiResponse({
      namespace_access: currentScenario.namespaceAccess,
    });
  }

  if (
    method === 'GET' &&
    path === '/v1/namespaces' &&
    options?.query?.owner_org_id === ORG_ID
  ) {
    return apiResponse({
      namespaces: currentScenario.orgNamespaces,
    });
  }

  if (
    method === 'PUT' &&
    path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/namespace-access/namespace-2`
  ) {
    currentScenario.putCalls.push({
      path,
      body: options?.body,
    });
    currentScenario.namespaceAccess = [
      ...currentScenario.namespaceAccess,
      {
        namespace_claim_id: 'namespace-2',
        ecosystem: 'npm',
        namespace: '@target',
        is_verified: true,
        permissions: ['transfer_ownership'],
        granted_at: '2026-04-06T00:00:00Z',
      },
    ];
    return apiResponse({
      message: 'Team namespace access updated',
      namespace_claim: {
        id: 'namespace-2',
        namespace: '@target',
      },
      permissions: ['transfer_ownership'],
    });
  }

  if (
    method === 'DELETE' &&
    path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/namespace-access/namespace-1`
  ) {
    currentScenario.deleteCalls.push({ path });
    currentScenario.namespaceAccess = currentScenario.namespaceAccess.filter(
      (grant) => grant.namespace_claim_id !== 'namespace-1'
    );
    return apiResponse({ message: 'Team namespace access removed' });
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

function queryRequiredForm(target: HTMLElement, selector: string): HTMLFormElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLFormElement)) {
    throw new Error(`Missing form for selector ${selector}.`);
  }
  return element;
}

function queryRequiredInput(target: HTMLElement, selector: string): HTMLInputElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLInputElement)) {
    throw new Error(`Missing input for selector ${selector}.`);
  }
  return element;
}

function queryRequiredTextArea(
  target: HTMLElement,
  selector: string
): HTMLTextAreaElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLTextAreaElement)) {
    throw new Error(`Missing textarea for selector ${selector}.`);
  }
  return element;
}

function queryRequiredSelect(target: HTMLElement, selector: string): HTMLSelectElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLSelectElement)) {
    throw new Error(`Missing select for selector ${selector}.`);
  }
  return element;
}

function queryRequiredCheckbox(
  target: ParentNode,
  selector: string
): HTMLInputElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLInputElement)) {
    throw new Error(`Missing checkbox for selector ${selector}.`);
  }
  return element;
}

function queryRequiredButton(target: HTMLElement, selector: string): HTMLButtonElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLButtonElement)) {
    throw new Error(`Missing button for selector ${selector}.`);
  }
  return element;
}

function queryClosestForm(element: Element): HTMLFormElement {
  const form = element.closest('form');
  if (!(form instanceof HTMLFormElement)) {
    throw new Error('Expected element to belong to a form.');
  }
  return form;
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
