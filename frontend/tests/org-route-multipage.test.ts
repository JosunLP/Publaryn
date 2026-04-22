/// <reference path="./bun-test.d.ts" />

import { afterEach, describe, expect, mock, test } from 'bun:test';
import { writable } from 'svelte/store';

import { renderPackageSelectionValue } from '../src/pages/org-workspace-actions';
import {
  changeValue,
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
  body: JsonRecord;
}

interface SearchCall {
  org: string;
  repository: string;
}

interface FetchScenario {
  canManageInvitations: boolean;
  canManageMembers: boolean;
  canManageTeams: boolean;
  canManageRepositories: boolean;
  canManageNamespaces: boolean;
  repositoryPageRequests: number[];
  packagePageRequests: number[];
  invitationRequests: string[];
  orgUpdateCalls: MutationCall[];
  orgMfaRequired: boolean;
  teamRepositoryAccessUpdates: MutationCall[];
  teamPackageAccessUpdates: MutationCall[];
  repositoryTransfers: MutationCall[];
  packageTransfers: MutationCall[];
  searchCalls: SearchCall[];
}

const ORG_ID = '11111111-1111-4111-8111-111111111111';
const TEAM_SLUG = 'release-engineering';
const ORG_SLUG = 'source-org';
const TARGET_ORG_SLUG = 'target-org';
const apiClientModuleUrl = new URL('../src/api/client.ts', import.meta.url).href;
const gotoCalls: string[] = [];
const pageStore = writable<TestPageState>(buildPageState('https://example.test/'));
let currentAuthToken: string | null = null;
let currentScenario: FetchScenario | null = null;

const repositories = Array.from({ length: 101 }, (_, index) => {
  const suffix = String(index + 1).padStart(3, '0');
  return {
    id: `repo-${suffix}`,
    name: `Repository ${suffix}`,
    slug: `repo-${suffix}`,
    description: `Repository ${suffix} description`,
    kind: 'release',
    visibility: 'private',
    upstream_url: null,
    package_count: 0,
    created_at: '2026-04-01T00:00:00Z',
    can_transfer: true,
  };
});

const packages = Array.from({ length: 101 }, (_, index) => {
  const suffix = String(index + 1).padStart(3, '0');
  return {
    id: `pkg-${suffix}`,
    ecosystem: 'npm',
    name: `package-${suffix}`,
    description: `Package ${suffix} description`,
    download_count: 100 + index,
    created_at: '2026-04-01T00:00:00Z',
    can_transfer: true,
  };
});

const currentOrgMembership = {
  id: ORG_ID,
  slug: ORG_SLUG,
  name: 'Source Org',
  role: 'owner',
  package_count: packages.length,
  team_count: 1,
  capabilities: {
    can_manage: true,
    can_manage_invitations: true,
    can_manage_members: true,
    can_manage_teams: true,
    can_manage_repositories: true,
    can_manage_namespaces: true,
    can_view_member_directory: true,
    can_view_audit_log: true,
    can_transfer_ownership: true,
  },
};

const targetOrgMembership = {
  id: '22222222-2222-4222-8222-222222222222',
  slug: TARGET_ORG_SLUG,
  name: 'Target Org',
  role: 'admin',
  capabilities: {
    can_manage: true,
    can_manage_invitations: true,
    can_manage_members: true,
    can_manage_teams: true,
    can_manage_repositories: true,
    can_manage_namespaces: true,
    can_view_member_directory: true,
    can_view_audit_log: true,
    can_transfer_ownership: false,
  },
};

const members = [
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
];

mock.module('$app/stores', () => ({
  page: {
    subscribe: pageStore.subscribe,
  },
}));

mock.module('$app/navigation', () => ({
  async goto(path: string | URL): Promise<void> {
    gotoCalls.push(path.toString());
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
      return currentAuthToken;
    },
    setAuthToken(token: string | null): void {
      currentAuthToken = token;
    },
    clearAuthToken(): void {
      currentAuthToken = null;
    },
    onUnauthorized(): void {},
    api: {
      get: (path: string, options?: { query?: Record<string, unknown> }) =>
        handleApiRequest('GET', path, options),
      post: (
        path: string,
        options?: { query?: Record<string, unknown>; body?: JsonRecord }
      ) => handleApiRequest('POST', path, options),
      put: (
        path: string,
        options?: { query?: Record<string, unknown>; body?: JsonRecord }
      ) => handleApiRequest('PUT', path, options),
      patch: (
        path: string,
        options?: { query?: Record<string, unknown>; body?: JsonRecord }
      ) => handleApiRequest('PATCH', path, options),
      delete: (path: string, options?: { query?: Record<string, unknown> }) =>
        handleApiRequest('DELETE', path, options),
    },
  };
});

const SearchPage = await import('../src/routes/search/+page.svelte');
const OrgPage = await import('../src/routes/orgs/[slug]/+page.svelte');

afterEach(() => {
  gotoCalls.length = 0;
  currentAuthToken = null;
  currentScenario = null;
  pageStore.set(buildPageState('https://example.test/'));
});

describe('route-level multi-page org dataset coverage', () => {
  test('org workspace renders second-page packages and repositories across delegated access and transfer controls', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(scenario.repositoryPageRequests).toEqual([1, 2]);
        expect(scenario.packagePageRequests).toEqual([1, 2]);
      });

      const finalRepository = repositories.at(-1);
      const finalPackage = packages.at(-1);
      const finalPackageKey = renderPackageSelectionValue(
        finalPackage?.ecosystem,
        finalPackage?.name
      );

      await waitFor(() => {
        const repositoryGrantSelect = queryRequiredSelect(
          target,
          `#team-repository-${TEAM_SLUG}`
        );
        const packageGrantSelect = queryRequiredSelect(
          target,
          `#team-package-${TEAM_SLUG}`
        );
        const repositoryTransferSelect = queryRequiredSelect(
          target,
          '#org-repository-transfer-repository'
        );
        const packageTransferSelect = queryRequiredSelect(
          target,
          '#org-package-transfer-package'
        );

        expect(optionValues(repositoryGrantSelect)).toContain(finalRepository?.slug);
        expect(optionValues(packageGrantSelect)).toContain(finalPackageKey);
        expect(optionValues(repositoryTransferSelect)).toContain(finalRepository?.slug);
        expect(optionValues(packageTransferSelect)).toContain(finalPackageKey);
        expect(
          target.querySelector(
            `a[href="/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}"]`
          )
        ).not.toBeNull();
        expect(
          target.querySelector(
            `a[href="/packages/${finalPackage?.ecosystem}/${finalPackage?.name}?tab=security"]`
          )
        ).not.toBeNull();
        expect(
          target.querySelector(
            `a[href="/packages/${finalPackage?.ecosystem}/${finalPackage?.name}"]`
          )
        ).not.toBeNull();
      });
    } finally {
      unmount();
    }
  });

  test('org workspace saves and reloads the organization MFA policy', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Organization profile');
      });

      const profileCheckbox = queryCheckbox(target, '#org-profile-mfa-required');
      expect(profileCheckbox.checked).toBe(false);

      const profileForm = queryRequiredForm(profileCheckbox.closest('form'));
      setChecked(profileCheckbox, true);
      submitForm(profileForm);

      await waitFor(() => {
        expect(scenario.orgUpdateCalls).toHaveLength(1);
        expect(target.textContent).toContain('Organization profile updated.');
        expect(target.textContent).toContain('MFA required');
      });

      expect(scenario.orgUpdateCalls[0]).toEqual({
        path: `/v1/orgs/${ORG_SLUG}`,
        body: {
          description: 'Source organization',
          website: null,
          email: null,
          mfa_required: true,
        },
      });
      expect(queryCheckbox(target, '#org-profile-mfa-required').checked).toBe(true);
    } finally {
      unmount();
    }
  });

  test('org workspace does not load invitation management UI when the explicit invitation capability is absent', async () => {
    const scenario = createFetchScenario();
    scenario.canManageInvitations = false;
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Organization profile');
        expect(target.textContent).toContain('Add member directly');
      });

      expect(scenario.invitationRequests).toEqual([]);
      expect(target.textContent).not.toContain('Invite a member');
      expect(target.textContent).not.toContain('Invitations');
    } finally {
      unmount();
    }
  });

  test('org workspace hides member mutation controls when the explicit member-management capability is absent', async () => {
    const scenario = createFetchScenario();
    scenario.canManageMembers = false;
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Invite a member');
        expect(target.textContent).toContain('Members');
      });

      expect(target.textContent).not.toContain('Add member directly');
      expect(target.querySelector('#org-member-username')).toBeNull();
      expect(target.querySelector('[id^="member-role-"]')).toBeNull();
    } finally {
      unmount();
    }
  });

  test('org workspace hides team management controls when the explicit team-management capability is absent', async () => {
    const scenario = createFetchScenario();
    scenario.canManageTeams = false;
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Members');
        expect(target.textContent).toContain('Teams');
      });

      expect(target.textContent).toContain('Add member directly');
      expect(target.querySelector('#team-create-name')).toBeNull();
      expect(target.querySelector(`#team-package-${TEAM_SLUG}`)).toBeNull();
      expect(target.querySelector(`#team-repository-${TEAM_SLUG}`)).toBeNull();
      expect(target.querySelector(`#team-namespace-${TEAM_SLUG}`)).toBeNull();
      expect(target.textContent).not.toContain('Create team');
    } finally {
      unmount();
    }
  });

  test('org workspace hides repository management controls when the explicit repository-management capability is absent', async () => {
    const scenario = createFetchScenario();
    scenario.canManageRepositories = false;
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Repositories');
        expect(target.textContent).toContain('Teams');
      });

      expect(target.querySelector(`#team-package-${TEAM_SLUG}`)).not.toBeNull();
      expect(target.querySelector(`#team-repository-${TEAM_SLUG}`)).toBeNull();
      expect(target.querySelector('#repository-create-name')).toBeNull();
      expect(target.querySelector('#org-repository-transfer-repository')).toBeNull();
      expect(target.querySelector('#repository-visibility-repo-001')).toBeNull();
      expect(target.textContent).not.toContain('Create repository');
      expect(target.textContent).not.toContain('Transfer repository ownership');
    } finally {
      unmount();
    }
  });

  test('org workspace hides namespace management controls when the explicit namespace-management capability is absent', async () => {
    const scenario = createFetchScenario();
    scenario.canManageNamespaces = false;
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Namespace claims');
        expect(target.textContent).toContain('Teams');
      });

      expect(target.querySelector(`#team-repository-${TEAM_SLUG}`)).not.toBeNull();
      expect(target.querySelector(`#team-namespace-${TEAM_SLUG}`)).toBeNull();
      expect(target.querySelector('#namespace-value')).toBeNull();
      expect(target.textContent).not.toContain('Create namespace claim');
    } finally {
      unmount();
    }
  });

  test('org workspace submits second-page delegated repository and package access selections', async () => {
    const finalRepository = repositories.at(-1);
    const finalPackage = packages.at(-1);
    const finalPackageKey = renderPackageSelectionValue(
      finalPackage?.ecosystem,
      finalPackage?.name
    );

    const repositoryScenario = createFetchScenario();
    const repositoryRender = await mountOrgPage(repositoryScenario);

    try {
      await waitFor(() => {
        expect(
          optionValues(
            queryRequiredSelect(
              repositoryRender.target,
              `#team-repository-${TEAM_SLUG}`
            )
          )
        ).toContain(finalRepository?.slug);
      });

      const repositoryGrantSelect = queryRequiredSelect(
        repositoryRender.target,
        `#team-repository-${TEAM_SLUG}`
      );
      const repositoryGrantForm = queryRequiredForm(
        repositoryGrantSelect.closest('form')
      );
      const repositoryGrantPublish = queryCheckbox(
        repositoryGrantForm,
        'input[value="publish"]'
      );
      changeValue(repositoryGrantSelect, finalRepository?.slug || '');
      setChecked(repositoryGrantPublish, true);
      submitForm(repositoryGrantForm);

      await waitFor(() => {
        expect(repositoryScenario.teamRepositoryAccessUpdates).toHaveLength(1);
      });
      expect(repositoryScenario.teamRepositoryAccessUpdates[0]).toEqual({
        path: `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access/${finalRepository?.slug}`,
        body: { permissions: ['publish'] },
      });
    } finally {
      repositoryRender.unmount();
    }

    const packageScenario = createFetchScenario();
    const packageRender = await mountOrgPage(packageScenario);

    try {
      await waitFor(() => {
        expect(
          optionValues(
            queryRequiredSelect(packageRender.target, `#team-package-${TEAM_SLUG}`)
          )
        ).toContain(finalPackageKey);
      });

      const packageGrantSelect = queryRequiredSelect(
        packageRender.target,
        `#team-package-${TEAM_SLUG}`
      );
      const packageGrantForm = queryRequiredForm(packageGrantSelect.closest('form'));
      const packageGrantPublish = queryCheckbox(
        packageGrantForm,
        'input[value="publish"]'
      );
      changeValue(packageGrantSelect, finalPackageKey);
      setChecked(packageGrantPublish, true);
      submitForm(packageGrantForm);

      await waitFor(() => {
        expect(packageScenario.teamPackageAccessUpdates).toHaveLength(1);
      });
      expect(packageScenario.teamPackageAccessUpdates[0]).toEqual({
        path: `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access/npm/${finalPackage?.name}`,
        body: { permissions: ['publish'] },
      });
    } finally {
      packageRender.unmount();
    }
  });

  test('org workspace submits second-page repository and package transfer selections', async () => {
    const finalRepository = repositories.at(-1);
    const finalPackage = packages.at(-1);
    const finalPackageKey = renderPackageSelectionValue(
      finalPackage?.ecosystem,
      finalPackage?.name
    );

    const repositoryScenario = createFetchScenario();
    const repositoryRender = await mountOrgPage(repositoryScenario);

    try {
      await waitFor(() => {
        expect(
          optionValues(
            queryRequiredSelect(
              repositoryRender.target,
              '#org-repository-transfer-repository'
            )
          )
        ).toContain(finalRepository?.slug);
      });

      const repositoryTransferForm = queryRequiredForm(
        queryRequiredSelect(
          repositoryRender.target,
          '#org-repository-transfer-repository'
        ).closest('form')
      );
      const repositoryTransferSelect = queryRequiredSelect(
        repositoryTransferForm,
        '#org-repository-transfer-repository'
      );
      const repositoryTransferTarget = queryRequiredSelect(
        repositoryTransferForm,
        '#org-repository-transfer-target'
      );
      const repositoryTransferConfirm = queryCheckbox(
        repositoryTransferForm,
        'input[name="confirm"]'
      );
      changeValue(repositoryTransferSelect, finalRepository?.slug || '');
      changeValue(repositoryTransferTarget, TARGET_ORG_SLUG);
      setChecked(repositoryTransferConfirm, true);
      submitForm(repositoryTransferForm);

      await waitFor(() => {
        expect(repositoryScenario.repositoryTransfers).toHaveLength(1);
      });
      expect(repositoryScenario.repositoryTransfers[0]).toEqual({
        path: `/v1/repositories/${finalRepository?.slug}/ownership-transfer`,
        body: { target_org_slug: TARGET_ORG_SLUG },
      });
    } finally {
      repositoryRender.unmount();
    }

    const packageScenario = createFetchScenario();
    const packageRender = await mountOrgPage(packageScenario);

    try {
      await waitFor(() => {
        expect(
          optionValues(
            queryRequiredSelect(
              packageRender.target,
              '#org-package-transfer-package'
            )
          )
        ).toContain(finalPackageKey);
      });

      const packageTransferForm = queryRequiredForm(
        queryRequiredSelect(
          packageRender.target,
          '#org-package-transfer-package'
        ).closest('form')
      );
      const packageTransferSelect = queryRequiredSelect(
        packageTransferForm,
        '#org-package-transfer-package'
      );
      const packageTransferTarget = queryRequiredSelect(
        packageTransferForm,
        '#org-package-transfer-target'
      );
      const packageTransferConfirm = queryCheckbox(
        packageTransferForm,
        'input[name="confirm"]'
      );
      changeValue(packageTransferSelect, finalPackageKey);
      changeValue(packageTransferTarget, TARGET_ORG_SLUG);
      setChecked(packageTransferConfirm, true);
      submitForm(packageTransferForm);

      await waitFor(() => {
        expect(packageScenario.packageTransfers).toHaveLength(1);
      });
      expect(packageScenario.packageTransfers[0]).toEqual({
        path: `/v1/packages/npm/${finalPackage?.name}/ownership-transfer`,
        body: { target_org_slug: TARGET_ORG_SLUG },
      });
    } finally {
      packageRender.unmount();
    }
  });

  test('org-scoped search loads repository filter options across multiple pages and submits the selected repository', async () => {
    const scenario = createFetchScenario();
    currentScenario = scenario;
    currentAuthToken = 'pub_test_token';
    pageStore.set(
      buildPageState(
        `https://example.test/search?org=${ORG_SLUG}&repository=repo-101`
      )
    );

    const { target, unmount, flush } = await renderSvelte(SearchPage);

    try {
      await waitFor(() => {
        expect(scenario.repositoryPageRequests).toEqual([1, 2]);
        const repositorySelect = queryRequiredSelect(
          target,
          'select[aria-label="Repository scope"]'
        );
        expect(optionValues(repositorySelect)).toContain('repo-101');
        expect(repositorySelect.value).toBe('repo-101');
      });

      const repositorySelect = queryRequiredSelect(
        target,
        'select[aria-label="Repository scope"]'
      );
      flush();
      expect(repositorySelect.value).toBe('repo-101');

      const form = queryRequiredForm(target.querySelector('#search-form'));
      submitForm(form);

      await waitFor(() => {
        expect(gotoCalls).toEqual(['/search?org=source-org&repository=repo-101']);
      });
    } finally {
      unmount();
    }
  });

  test('search results expose a secondary package-details link alongside security navigation', async () => {
    const scenario = createFetchScenario();
    currentScenario = scenario;
    currentAuthToken = 'pub_test_token';
    pageStore.set(buildPageState('https://example.test/search?q=example'));

    const { target, unmount } = await renderSvelte(SearchPage);

    try {
      await waitFor(() => {
        expect(
          target.querySelector('a[href="/packages/npm/example-package?tab=security"]')
        ).not.toBeNull();
        expect(
          target.querySelector('a[href="/packages/npm/example-package"]')
        ).not.toBeNull();
        expect(target.textContent).toContain('Open package details');
      });

      expect(scenario.searchCalls).toEqual([
        {
          org: '',
          repository: '',
        },
      ]);
    } finally {
      unmount();
    }
  });
});

function buildPageState(
  url: string,
  params: Record<string, string> = {}
): TestPageState {
  return {
    url: new URL(url),
    params,
    route: { id: null },
    status: 200,
    error: null,
    data: {},
    form: null,
  };
}

function createFetchScenario(): FetchScenario {
  return {
    canManageInvitations: true,
    canManageMembers: true,
    canManageTeams: true,
    canManageRepositories: true,
    canManageNamespaces: true,
    repositoryPageRequests: [],
    packagePageRequests: [],
    invitationRequests: [],
    orgUpdateCalls: [],
    orgMfaRequired: false,
    teamRepositoryAccessUpdates: [],
    teamPackageAccessUpdates: [],
    repositoryTransfers: [],
    packageTransfers: [],
    searchCalls: [],
  };
}

async function handleApiRequest(
  method: string,
  path: string,
  options?: { query?: Record<string, unknown>; body?: JsonRecord }
): Promise<{ data: unknown; requestId: null }> {
  const scenario = currentScenario;
  if (!scenario) {
    throw new Error(`No active API scenario for ${method} ${path}`);
  }

  const url = new URL(path, 'https://example.test');
  for (const [key, value] of Object.entries(options?.query || {})) {
    if (value != null && value !== '') {
      url.searchParams.set(key, String(value));
    }
  }
  const requestPath = url.pathname;
  const body = options?.body || {};

  if (method === 'GET' && requestPath === `/v1/orgs/${ORG_SLUG}`) {
    return apiResponse({
      id: ORG_ID,
      name: 'Source Org',
      slug: ORG_SLUG,
      description: 'Source organization',
      is_verified: true,
      mfa_required: scenario.orgMfaRequired,
      website: null,
      email: null,
      created_at: '2026-04-01T00:00:00Z',
      capabilities: {
        can_manage: true,
        can_manage_invitations: scenario.canManageInvitations,
        can_manage_members: scenario.canManageMembers,
        can_manage_teams: scenario.canManageTeams,
        can_manage_repositories: scenario.canManageRepositories,
        can_manage_namespaces: scenario.canManageNamespaces,
        can_view_member_directory: true,
        can_view_audit_log: true,
        can_transfer_ownership: true,
      },
    });
  }

  if (method === 'GET' && requestPath === '/v1/users/me/organizations') {
    return apiResponse({
      organizations: [
        {
          ...currentOrgMembership,
          capabilities: {
            ...currentOrgMembership.capabilities,
            can_manage_invitations: scenario.canManageInvitations,
            can_manage_members: scenario.canManageMembers,
            can_manage_teams: scenario.canManageTeams,
            can_manage_repositories: scenario.canManageRepositories,
            can_manage_namespaces: scenario.canManageNamespaces,
          },
        },
        targetOrgMembership,
      ],
    });
  }

  if (method === 'GET' && requestPath === `/v1/orgs/${ORG_SLUG}/repositories`) {
    const page = parsePage(url.searchParams.get('page'));
    const perPage = parsePerPage(url.searchParams.get('per_page'));
    scenario.repositoryPageRequests.push(page);
    return apiResponse({
      repositories: paginate(repositories, page, perPage),
    });
  }

  if (method === 'GET' && requestPath === `/v1/orgs/${ORG_SLUG}/packages`) {
    const page = parsePage(url.searchParams.get('page'));
    const perPage = parsePerPage(url.searchParams.get('per_page'));
    scenario.packagePageRequests.push(page);
    return apiResponse({
      packages: paginate(packages, page, perPage),
    });
  }

  if (
    method === 'GET' &&
    requestPath === `/v1/orgs/${ORG_SLUG}/security-findings`
  ) {
    return apiResponse({
        summary: {
          open_findings: 0,
          affected_packages: 0,
          severities: {
            critical: 0,
            high: 0,
            medium: 0,
            low: 0,
            info: 0,
          },
        },
        packages: [],
      });
  }

  if (method === 'GET' && requestPath === `/v1/orgs/${ORG_SLUG}/invitations`) {
    scenario.invitationRequests.push(url.toString());
    return apiResponse({ invitations: [] });
  }

  if (method === 'GET' && requestPath === `/v1/orgs/${ORG_SLUG}/members`) {
    return apiResponse({ members });
  }

  if (method === 'GET' && requestPath === `/v1/orgs/${ORG_SLUG}/teams`) {
    return apiResponse({
        teams: [
          {
            name: 'Release Engineering',
            slug: TEAM_SLUG,
            description: 'Owns release governance',
            created_at: '2026-04-01T00:00:00Z',
          },
        ],
      });
  }

  if (method === 'GET' && requestPath === `/v1/orgs/${ORG_SLUG}/audit`) {
    return apiResponse({
      page: 1,
      per_page: 20,
      has_next: false,
      logs: [],
    });
  }

  if (
    method === 'GET' &&
    requestPath === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/members`
  ) {
    return apiResponse({ members: [] });
  }

  if (
    method === 'GET' &&
    requestPath === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access`
  ) {
    return apiResponse({ package_access: [] });
  }

  if (
    method === 'GET' &&
    requestPath === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access`
  ) {
    return apiResponse({ repository_access: [] });
  }

  if (
    method === 'GET' &&
    requestPath === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/namespace-access`
  ) {
    return apiResponse({ namespace_access: [] });
  }

  if (method === 'GET' && requestPath === '/v1/namespaces') {
    return apiResponse({ namespaces: [] });
  }

  if (
    method === 'GET' &&
    requestPath.startsWith('/v1/repositories/repo-') &&
    requestPath.endsWith('/packages')
  ) {
    return apiResponse({ packages: [] });
  }

  if (
    method === 'PATCH' &&
    requestPath === `/v1/orgs/${ORG_SLUG}`
  ) {
    scenario.orgUpdateCalls.push({ path: requestPath, body });
    scenario.orgMfaRequired = body.mfa_required === true;
    return apiResponse({ message: 'Organization updated' });
  }

  if (
    method === 'PUT' &&
    requestPath.startsWith(
      `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access/`
    )
  ) {
    scenario.teamRepositoryAccessUpdates.push({ path: requestPath, body });
    return apiResponse({ message: 'Saved repository access' });
  }

  if (
    method === 'PUT' &&
    requestPath.startsWith(
      `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access/`
    )
  ) {
    scenario.teamPackageAccessUpdates.push({ path: requestPath, body });
    return apiResponse({ message: 'Saved package access' });
  }

  if (
    method === 'POST' &&
    requestPath.startsWith('/v1/repositories/') &&
    requestPath.endsWith('/ownership-transfer')
  ) {
    scenario.repositoryTransfers.push({ path: requestPath, body });
    return apiResponse({
      repository: {
        slug: requestPath.split('/')[3],
      },
      owner: {
        slug: body.target_org_slug,
      },
    });
  }

  if (
    method === 'POST' &&
    requestPath.startsWith('/v1/packages/') &&
    requestPath.endsWith('/ownership-transfer')
  ) {
    scenario.packageTransfers.push({ path: requestPath, body });
    return apiResponse({
      owner: {
        slug: body.target_org_slug,
      },
    });
  }

  if (method === 'GET' && requestPath === '/v1/search') {
    scenario.searchCalls.push({
      org: url.searchParams.get('org') || '',
      repository: url.searchParams.get('repository') || '',
    });
    return apiResponse({
      total: 1,
      packages: [
        {
          ecosystem: 'npm',
          name: 'example-package',
          display_name: 'Example Package',
          description: 'Example search result',
          owner_name: 'Source Org',
          repository_name: 'Repository 001',
          repository_slug: 'repo-001',
          latest_version: '1.0.0',
          download_count: 42,
          visibility: 'private',
          updated_at: '2026-04-01T00:00:00Z',
          is_deprecated: false,
        },
      ],
      page: parsePage(url.searchParams.get('page')),
      per_page: parsePerPage(url.searchParams.get('per_page'), 20),
    });
  }

  throw new Error(`Unhandled API request: ${method} ${url.toString()}`);
}

function paginate<T>(items: T[], page: number, perPage: number): T[] {
  const offset = (page - 1) * perPage;
  return items.slice(offset, offset + perPage);
}

function parsePage(value: string | null): number {
  const parsed = Number.parseInt(value || '1', 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 1;
}

function parsePerPage(value: string | null, fallback = 100): number {
  const parsed = Number.parseInt(value || String(fallback), 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

function apiResponse<T>(data: T): { data: T; requestId: null } {
  return {
    data,
    requestId: null,
  };
}

async function waitFor(
  assertion: () => void | Promise<void>,
  timeoutMs = 1_000
): Promise<void> {
  const startedAt = Date.now();
  let lastError: unknown;

  while (Date.now() - startedAt < timeoutMs) {
    try {
      await assertion();
      return;
    } catch (error) {
      lastError = error;
    }

    await Promise.resolve();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }

  throw lastError instanceof Error
    ? lastError
    : new Error('Timed out waiting for UI state.');
}

function queryRequiredSelect(
  root: ParentNode | Element | null,
  selector: string
): HTMLSelectElement {
  const element = root?.querySelector(selector);
  if (!(element instanceof HTMLSelectElement)) {
    throw new Error(`Expected select for selector: ${selector}`);
  }

  return element;
}

function queryRequiredForm(root: Element | ParentNode | null): HTMLFormElement {
  if (!(root instanceof HTMLFormElement)) {
    throw new Error('Expected form element.');
  }

  return root;
}

function queryCheckbox(
  root: ParentNode | Element,
  selector: string
): HTMLInputElement {
  const element = root.querySelector(selector);
  if (!(element instanceof HTMLInputElement)) {
    throw new Error(`Expected checkbox for selector: ${selector}`);
  }

  return element;
}

function optionValues(select: HTMLSelectElement): string[] {
  return Array.from(select.options).map((option) => option.value);
}

async function mountOrgPage(
  scenario: FetchScenario
): Promise<Awaited<ReturnType<typeof renderSvelte>>> {
  currentScenario = scenario;
  currentAuthToken = 'pub_test_token';
  pageStore.set(buildPageState(`https://example.test/orgs/${ORG_SLUG}`, {
    slug: ORG_SLUG,
  }));
  return renderSvelte(OrgPage);
}
