/// <reference path="./bun-test.d.ts" />

import { afterEach, describe, expect, mock, test } from 'bun:test';
import { fileURLToPath } from 'node:url';
import { writable } from 'svelte/store';

import { renderPackageSelectionValue } from '../src/pages/org-workspace-actions';
import {
  changeValue,
  click,
  renderSvelte,
  setChecked,
  submitForm,
} from './svelte-dom';

type JsonRecord = Record<string, unknown>;

class TestApiError<TBody = unknown> extends Error {
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

interface AuditRequest {
  action: string;
  actorUserId: string;
  occurredFrom: string;
  occurredUntil: string;
  page: number;
  perPage: number;
}

interface AuditExportRequest {
  action: string;
  actorUserId: string;
  occurredFrom: string;
  occurredUntil: string;
  page: string | null;
  perPage: string | null;
}

interface AuditLogFixture {
  id: string;
  action: string;
  actor_user_id: string;
  actor_username: string;
  actor_display_name: string;
  target_org_id: string;
  metadata: JsonRecord;
  occurred_at: string;
}

interface FetchScenario {
  canManageInvitations: boolean;
  canManageMembers: boolean;
  canManageTeams: boolean;
  canManageRepositories: boolean;
  canManageNamespaces: boolean;
  teamDeleteError: string | null;
  namespaceDeleteError: string | null;
  invitationRevokeError: string | null;
  memberRemoveError: string | null;
  ownershipTransferError: string | null;
  namespaceTransferError: string | null;
  repositoryTransferError: string | null;
  packageTransferError: string | null;
  members: Array<{
    user_id: string;
    username: string;
    display_name: string;
    role: string;
    joined_at: string;
  }>;
  invitations: Array<{
    id?: string | null;
    status?: string | null;
    role?: string | null;
    invited_user?: {
      username?: string | null;
      email?: string | null;
    } | null;
    invited_by?: {
      username?: string | null;
    } | null;
    created_at?: string | null;
    expires_at?: string | null;
  }>;
  teams: Array<{
    name: string;
    slug: string;
    description: string;
    created_at: string;
  }>;
  namespaces: Array<{
    id?: string | null;
    ecosystem?: string | null;
    namespace?: string | null;
    owner_org_id?: string | null;
    is_verified?: boolean | null;
    created_at?: string | null;
    can_manage?: boolean | null;
    can_transfer?: boolean | null;
  }>;
  workspaceBootstrapRequests: string[];
  teamMemberRequests: string[];
  teamPackageAccessRequests: string[];
  teamRepositoryAccessRequests: string[];
  teamNamespaceAccessRequests: string[];
  repositoryPageRequests: number[];
  repositoryPackageCoverageRequests: string[];
  packagePageRequests: number[];
  invitationRequests: string[];
  invitationRevokeCalls: string[];
  memberRemoveCalls: string[];
  orgUpdateCalls: MutationCall[];
  ownershipTransfers: MutationCall[];
  orgName: string;
  orgMfaRequired: boolean;
  orgMemberDirectoryIsPrivate: boolean;
  teamRepositoryAccessUpdates: MutationCall[];
  teamPackageAccessUpdates: MutationCall[];
  teamDeleteCalls: string[];
  namespaceDeleteCalls: string[];
  namespaceTransfers: MutationCall[];
  repositoryTransfers: MutationCall[];
  packageTransfers: MutationCall[];
  searchCalls: SearchCall[];
  auditLogs: AuditLogFixture[];
  auditRequests: AuditRequest[];
  auditExportRequests: AuditExportRequest[];
}

const ORG_ID = '11111111-1111-4111-8111-111111111111';
const AUDIT_ACTOR_USER_ID = 'bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb';
const TEAM_SLUG = 'release-engineering';
const ORG_SLUG = 'source-org';
const TARGET_ORG_SLUG = 'target-org';
const NAMESPACE_CLAIM_ID = 'claim-001';
const NAMESPACE_CLAIM_VALUE = '@source-org';
const ACTIVE_INVITATION_ID = 'invite-001';
const ACTIVE_INVITEE_EMAIL = 'new-maintainer@example.test';
const ORG_PAGE_PATH = fileURLToPath(
  new URL('../src/routes/orgs/[slug]/+page.svelte', import.meta.url)
);
const SEARCH_PAGE_PATH = fileURLToPath(
  new URL('../src/routes/search/+page.svelte', import.meta.url)
);
const apiClientModuleUrl = new URL('../src/api/client.ts', import.meta.url)
  .href;
const gotoCalls: string[] = [];
const pageStore = writable<TestPageState>(
  buildPageState('https://example.test/')
);
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
    user_id: AUDIT_ACTOR_USER_ID,
    username: 'admin-user',
    display_name: 'Admin User',
    role: 'admin',
    joined_at: '2026-04-02T00:00:00Z',
  },
];

const auditLogs = Array.from({ length: 41 }, (_, index) => {
  const entryNumber = index + 1;
  const suffix = String(entryNumber).padStart(3, '0');
  const day = String((index % 28) + 1).padStart(2, '0');
  const hour = String(index % 24).padStart(2, '0');

  return {
    id: `audit-${suffix}`,
    action: 'team_update',
    actor_user_id: AUDIT_ACTOR_USER_ID,
    actor_username: 'admin-user',
    actor_display_name: 'Admin User',
    target_org_id: ORG_ID,
    metadata: {
      name: `Audit Team ${suffix}`,
      previous_description: `Previous audit description ${suffix}`,
      description: `Updated audit description ${suffix}`,
    },
    occurred_at: `2026-04-${day}T${hour}:00:00Z`,
  } satisfies AuditLogFixture;
});

mock.module('$app/stores', () => ({
  page: {
    subscribe: pageStore.subscribe,
  },
}));

mock.module('$app/navigation', () => ({
  async goto(path: string | URL): Promise<void> {
    const nextPath = path.toString();
    gotoCalls.push(nextPath);

    const nextUrl = new URL(nextPath, 'https://example.test');
    if (nextUrl.pathname === `/orgs/${ORG_SLUG}`) {
      pageStore.set(
        buildPageState(nextUrl.toString(), {
          slug: ORG_SLUG,
        })
      );
    }
  },
}));

mock.module(apiClientModuleUrl, () => {
  return {
    ApiError: TestApiError,
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
        expect(scenario.workspaceBootstrapRequests).toEqual([
          `/v1/orgs/${ORG_SLUG}/workspace`,
        ]);
        expect(scenario.repositoryPageRequests).toEqual([]);
        expect(scenario.packagePageRequests).toEqual([]);
        expect(scenario.repositoryPackageCoverageRequests).toEqual([]);
        expect(scenario.teamMemberRequests).toEqual([]);
        expect(scenario.teamPackageAccessRequests).toEqual([]);
        expect(scenario.teamRepositoryAccessRequests).toEqual([]);
        expect(scenario.teamNamespaceAccessRequests).toEqual([]);
      }, 5_000);

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

        expect(optionValues(repositoryGrantSelect)).toContain(
          finalRepository?.slug
        );
        expect(optionValues(packageGrantSelect)).toContain(finalPackageKey);
        expect(optionValues(repositoryTransferSelect)).toContain(
          finalRepository?.slug
        );
        expect(optionValues(packageTransferSelect)).toContain(finalPackageKey);
        expect(
          target.querySelector(`a[href="/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}"]`)
        ).not.toBeNull();
        expect(target.textContent).toContain('@admin-user');
        expect(
          target.querySelector(
            `#team-member-remove-${encodeURIComponent('admin-user')}`
          )
        ).not.toBeNull();
        expect(
          target.querySelector(
            `#team-package-revoke-${encodeURIComponent(
              `${finalPackage?.ecosystem}-${finalPackage?.name}`
            )}`
          )
        ).not.toBeNull();
        expect(
          target.querySelector(
            `#team-repository-revoke-${encodeURIComponent(
              finalRepository?.slug || ''
            )}`
          )
        ).not.toBeNull();
        expect(
          target.querySelector(
            `#team-namespace-revoke-${encodeURIComponent('claim-001')}`
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
      }, 5_000);
    } finally {
      unmount();
    }
  });

  test('org workspace renders repository package coverage from freshly loaded repositories', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(scenario.workspaceBootstrapRequests).toEqual([
          `/v1/orgs/${ORG_SLUG}/workspace`,
        ]);
        expect(scenario.repositoryPackageCoverageRequests).toEqual([]);
        expect(target.textContent).toContain('repo-package-001');
        expect(
          target.querySelector(
            'a[href="/packages/npm/repo-package-001?tab=security"]'
          )
        ).not.toBeNull();
        expect(
          target.querySelector('a[href="/packages/npm/repo-package-001"]')
        ).not.toBeNull();
      }, 5_000);
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

      const profileCheckbox = queryCheckbox(
        target,
        '#org-profile-mfa-required'
      );
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
          name: 'Source Org',
          description: 'Source organization',
          website: null,
          email: null,
          mfa_required: true,
          member_directory_is_private: false,
        },
      });
      expect(queryCheckbox(target, '#org-profile-mfa-required').checked).toBe(
        true
      );
    } finally {
      unmount();
    }
  });

  test('org workspace saves and reloads private member-directory policy', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Organization profile');
      });

      const directoryCheckbox = queryCheckbox(
        target,
        '#org-profile-member-directory-private'
      );
      expect(directoryCheckbox.checked).toBe(false);

      const profileForm = queryRequiredForm(directoryCheckbox.closest('form'));
      setChecked(directoryCheckbox, true);
      submitForm(profileForm);

      await waitFor(() => {
        expect(scenario.orgUpdateCalls).toHaveLength(1);
        expect(target.textContent).toContain('Organization profile updated.');
        expect(target.textContent).toContain('Private directory');
      });

      expect(scenario.orgUpdateCalls[0]).toEqual({
        path: `/v1/orgs/${ORG_SLUG}`,
        body: {
          name: 'Source Org',
          description: 'Source organization',
          website: null,
          email: null,
          mfa_required: false,
          member_directory_is_private: true,
        },
      });
      expect(
        queryCheckbox(target, '#org-profile-member-directory-private').checked
      ).toBe(true);
    } finally {
      unmount();
    }
  });

  test('org workspace saves and reloads the organization display name', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(queryRequiredInput(target, '#org-profile-name').value).toBe(
          'Source Org'
        );
      });

      const nameInput = queryRequiredInput(target, '#org-profile-name');
      const profileForm = queryRequiredForm(nameInput.closest('form'));
      changeValue(nameInput, 'Source Registry');
      submitForm(profileForm);

      await waitFor(() => {
        expect(scenario.orgUpdateCalls).toHaveLength(1);
        expect(target.textContent).toContain('Organization profile updated.');
        expect(queryRequiredInput(target, '#org-profile-name').value).toBe(
          'Source Registry'
        );
      });

      expect(scenario.orgUpdateCalls[0]).toEqual({
        path: `/v1/orgs/${ORG_SLUG}`,
        body: {
          name: 'Source Registry',
          description: 'Source organization',
          website: null,
          email: null,
          mfa_required: false,
          member_directory_is_private: false,
        },
      });
    } finally {
      unmount();
    }
  });

  test('org workspace blocks blank organization names client-side and keeps the slug guidance visible', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(queryRequiredInput(target, '#org-profile-name').value).toBe(
          'Source Org'
        );
        expect(target.textContent).toContain(
          'Organization slugs are part of workspace URLs'
        );
        expect(target.textContent).toContain('stay immutable after');
      });

      const nameInput = queryRequiredInput(target, '#org-profile-name');
      const profileForm = queryRequiredForm(nameInput.closest('form'));
      changeValue(nameInput, '   ');
      submitForm(profileForm);

      await waitFor(() => {
        expect(target.textContent).toContain('Organization name is required.');
      });

      expect(scenario.orgUpdateCalls).toEqual([]);
    } finally {
      unmount();
    }
  });

  test('org workspace blocks invalid profile contact fields and keeps profile guidance visible', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(queryRequiredInput(target, '#org-profile-name').value).toBe(
          'Source Org'
        );
        expect(target.textContent).toContain(
          'Use a full http:// or https:// URL.'
        );
        expect(target.textContent).toContain('Optional public contact email');
      });

      const websiteInput = queryRequiredInput(target, '#org-profile-website');
      const emailInput = queryRequiredInput(target, '#org-profile-email');
      const profileForm = queryRequiredForm(websiteInput.closest('form'));

      changeValue(websiteInput, 'example.test');
      submitForm(profileForm);

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Website must be a valid http:// or https:// URL.'
        );
      });
      expect(scenario.orgUpdateCalls).toEqual([]);

      changeValue(websiteInput, 'https://source.example.test');
      changeValue(emailInput, 'not-an-email');
      submitForm(profileForm);

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Email must be a valid email address.'
        );
      });
      expect(scenario.orgUpdateCalls).toEqual([]);
    } finally {
      unmount();
    }
  });

  test('org workspace surfaces redirect notices from focused team actions', async () => {
    const scenario = createFetchScenario();
    currentScenario = scenario;
    currentAuthToken = 'pub_test_token';
    pageStore.set(
      buildPageState(
        `https://example.test/orgs/${ORG_SLUG}?notice=${encodeURIComponent(
          `Deleted team ${TEAM_SLUG}.`
        )}`,
        {
          slug: ORG_SLUG,
        }
      )
    );

    const { target, unmount } = await renderSvelte(ORG_PAGE_PATH);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain(`Deleted team ${TEAM_SLUG}.`);
      });
    } finally {
      unmount();
    }
  });

  test('org workspace paginates activity log across route navigation', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(scenario.auditRequests).toEqual([
          {
            action: '',
            actorUserId: '',
            occurredFrom: '',
            occurredUntil: '',
            page: 1,
            perPage: 20,
          },
        ]);
        expect(target.textContent).toContain(
          'Showing page 1 with up to 20 events.'
        );
        expect(target.textContent).toContain('Audit Team 001');
        expect(target.textContent).toContain('Audit Team 020');
        expect(target.textContent).not.toContain('Audit Team 021');
        expect(queryButtonByText(target, '← Prev')).toBeNull();
        expect(queryButtonByText(target, 'Next →')).not.toBeNull();
      }, 5_000);

      click(queryRequiredButtonByText(target, 'Next →'));

      await waitFor(() => {
        expect(gotoCalls.at(-1)).toBe(`/orgs/${ORG_SLUG}?page=2`);
        expect(scenario.auditRequests.map((request) => request.page)).toEqual([
          1, 2,
        ]);
        expect(target.textContent).toContain(
          'Showing page 2 with up to 20 events.'
        );
        expect(target.textContent).toContain('Audit Team 021');
        expect(target.textContent).toContain('Audit Team 040');
        expect(target.textContent).not.toContain('Audit Team 001');
        expect(queryButtonByText(target, '← Prev')).not.toBeNull();
        expect(queryButtonByText(target, 'Next →')).not.toBeNull();
      }, 5_000);

      click(queryRequiredButtonByText(target, 'Next →'));

      await waitFor(() => {
        expect(gotoCalls.at(-1)).toBe(`/orgs/${ORG_SLUG}?page=3`);
        expect(scenario.auditRequests.map((request) => request.page)).toEqual([
          1, 2, 3,
        ]);
        expect(target.textContent).toContain(
          'Showing page 3 with up to 20 events.'
        );
        expect(target.textContent).toContain('Audit Team 041');
        expect(target.textContent).not.toContain('Audit Team 040');
        expect(queryButtonByText(target, '← Prev')).not.toBeNull();
        expect(queryButtonByText(target, 'Next →')).toBeNull();
      }, 5_000);
    } finally {
      unmount();
    }
  });

  test('org workspace resets activity log pagination when filters change', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario, '?page=2');

    try {
      await waitFor(() => {
        expect(target.textContent).toContain(
          'Showing page 2 with up to 20 events.'
        );
        expect(scenario.auditRequests.map((request) => request.page)).toEqual([
          2,
        ]);
      }, 5_000);

      changeValue(
        queryRequiredSelect(target, '#org-audit-action'),
        'team_update'
      );
      changeValue(queryRequiredInput(target, '#org-audit-actor'), 'admin-user');
      changeValue(queryRequiredInput(target, '#org-audit-from'), '2026-04-01');
      changeValue(queryRequiredInput(target, '#org-audit-until'), '2026-04-30');
      submitForm(
        queryRequiredForm(
          queryRequiredSelect(target, '#org-audit-action').closest('form')
        )
      );

      await waitFor(() => {
        const lastNavigation = new URL(
          gotoCalls.at(-1) || '',
          'https://example.test'
        );
        expect(lastNavigation.pathname).toBe(`/orgs/${ORG_SLUG}`);
        expect(lastNavigation.searchParams.get('page')).toBeNull();
        expect(lastNavigation.searchParams.get('action')).toBe('team_update');
        expect(lastNavigation.searchParams.get('actor_user_id')).toBe(
          AUDIT_ACTOR_USER_ID
        );
        expect(lastNavigation.searchParams.get('actor_username')).toBe(
          'admin-user'
        );
        expect(lastNavigation.searchParams.get('occurred_from')).toBe(
          '2026-04-01'
        );
        expect(lastNavigation.searchParams.get('occurred_until')).toBe(
          '2026-04-30'
        );
        expect(scenario.auditRequests.at(-1)).toEqual({
          action: 'team_update',
          actorUserId: AUDIT_ACTOR_USER_ID,
          occurredFrom: '2026-04-01',
          occurredUntil: '2026-04-30',
          page: 1,
          perPage: 20,
        });
        expect(target.textContent).toContain(
          'Showing page 1 with up to 20 events, filtered by team updated, actor @admin-user, UTC dates 2026-04-01 through 2026-04-30.'
        );
      }, 5_000);
    } finally {
      unmount();
    }
  });

  test('org workspace exports filtered activity log without pagination parameters', async () => {
    const scenario = createFetchScenario();
    const originalCreateObjectURL = window.URL.createObjectURL;
    const originalRevokeObjectURL = window.URL.revokeObjectURL;
    const createObjectURLCalls: Blob[] = [];
    const revokeObjectURLCalls: string[] = [];
    const createObjectURL = (blob: Blob): string => {
      createObjectURLCalls.push(blob);
      return 'blob:audit-export';
    };
    const revokeObjectURL = (objectUrl: string): void => {
      revokeObjectURLCalls.push(objectUrl);
    };
    Object.defineProperty(window.URL, 'createObjectURL', {
      configurable: true,
      writable: true,
      value: createObjectURL,
    });
    Object.defineProperty(window.URL, 'revokeObjectURL', {
      configurable: true,
      writable: true,
      value: revokeObjectURL,
    });

    const { target, unmount } = await mountOrgPage(
      scenario,
      `?action=team_update&actor_user_id=${AUDIT_ACTOR_USER_ID}&actor_username=admin-user&occurred_from=2026-04-01&occurred_until=2026-04-30&page=2`
    );

    try {
      await waitFor(() => {
        expect(target.textContent).toContain(
          'Showing page 2 with up to 20 events, filtered by team updated, actor @admin-user, UTC dates 2026-04-01 through 2026-04-30.'
        );
      }, 5_000);

      click(queryRequiredButtonByText(target, 'Export CSV'));

      await waitFor(() => {
        expect(scenario.auditExportRequests).toEqual([
          {
            action: 'team_update',
            actorUserId: AUDIT_ACTOR_USER_ID,
            occurredFrom: '2026-04-01',
            occurredUntil: '2026-04-30',
            page: null,
            perPage: null,
          },
        ]);
        expect(createObjectURLCalls).toHaveLength(1);
        expect(revokeObjectURLCalls).toEqual(['blob:audit-export']);
      }, 5_000);
    } finally {
      Object.defineProperty(window.URL, 'createObjectURL', {
        configurable: true,
        writable: true,
        value: originalCreateObjectURL,
      });
      Object.defineProperty(window.URL, 'revokeObjectURL', {
        configurable: true,
        writable: true,
        value: originalRevokeObjectURL,
      });
      unmount();
    }
  });

  test('org workspace requires explicit confirmation before deleting a team', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Release Engineering');
      });

      click(queryRequiredButton(target, `#team-delete-toggle-${TEAM_SLUG}`));

      await waitFor(() => {
        expect(
          queryRequiredFormBySelector(target, `#team-delete-form-${TEAM_SLUG}`)
        ).toBeDefined();
      });

      submitForm(
        queryRequiredFormBySelector(target, `#team-delete-form-${TEAM_SLUG}`)
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Please confirm that you understand deleting this team revokes its delegated access.'
        );
      });

      expect(scenario.teamDeleteCalls).toEqual([]);
      expect(scenario.teams.map((team) => team.slug)).toContain(TEAM_SLUG);
      expect(
        queryRequiredFormBySelector(target, `#team-delete-form-${TEAM_SLUG}`)
      ).toBeDefined();
    } finally {
      unmount();
    }
  });

  test('org workspace deletes a team after explicit confirmation', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Release Engineering');
      });

      click(queryRequiredButton(target, `#team-delete-toggle-${TEAM_SLUG}`));

      await waitFor(() => {
        expect(
          queryRequiredFormBySelector(target, `#team-delete-form-${TEAM_SLUG}`)
        ).toBeDefined();
      });

      setChecked(
        queryCheckbox(target, `#team-delete-confirm-${TEAM_SLUG}`),
        true
      );
      submitForm(
        queryRequiredFormBySelector(target, `#team-delete-form-${TEAM_SLUG}`)
      );

      await waitFor(() => {
        expect(scenario.teamDeleteCalls).toEqual([
          `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}`,
        ]);
        expect(target.textContent).toContain(`Deleted team ${TEAM_SLUG}.`);
      });

      expect(target.textContent).not.toContain('Release Engineering');
    } finally {
      unmount();
    }
  });

  test('org workspace keeps the confirmation surface open when team deletion fails', async () => {
    const scenario = createFetchScenario();
    scenario.teamDeleteError = 'Failed to delete team.';
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Release Engineering');
      });

      click(queryRequiredButton(target, `#team-delete-toggle-${TEAM_SLUG}`));

      await waitFor(() => {
        expect(
          queryRequiredFormBySelector(target, `#team-delete-form-${TEAM_SLUG}`)
        ).toBeDefined();
      });

      setChecked(
        queryCheckbox(target, `#team-delete-confirm-${TEAM_SLUG}`),
        true
      );
      submitForm(
        queryRequiredFormBySelector(target, `#team-delete-form-${TEAM_SLUG}`)
      );

      await waitFor(() => {
        expect(target.textContent).toContain('Failed to delete team.');
        expect(scenario.teamDeleteCalls).toEqual([]);
        expect(
          queryRequiredFormBySelector(target, `#team-delete-form-${TEAM_SLUG}`)
        ).toBeDefined();
      });

      expect(scenario.teams.map((team) => team.slug)).toContain(TEAM_SLUG);
      expect(target.textContent).toContain('Release Engineering');
    } finally {
      unmount();
    }
  });

  test('org workspace requires explicit confirmation before deleting a namespace claim', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain(NAMESPACE_CLAIM_VALUE);
      });

      click(
        queryRequiredButton(
          target,
          `#namespace-delete-toggle-${NAMESPACE_CLAIM_ID}`
        )
      );

      await waitFor(() => {
        expect(
          queryRequiredFormBySelector(
            target,
            `#namespace-delete-form-${NAMESPACE_CLAIM_ID}`
          )
        ).toBeDefined();
      });

      submitForm(
        queryRequiredFormBySelector(
          target,
          `#namespace-delete-form-${NAMESPACE_CLAIM_ID}`
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Please confirm that you understand deleting this namespace claim is immediate and cannot be undone.'
        );
      });

      expect(scenario.namespaceDeleteCalls).toEqual([]);
      expect(scenario.namespaces.map((claim) => claim.id)).toContain(
        NAMESPACE_CLAIM_ID
      );
      expect(
        queryRequiredFormBySelector(
          target,
          `#namespace-delete-form-${NAMESPACE_CLAIM_ID}`
        )
      ).toBeDefined();
    } finally {
      unmount();
    }
  });

  test('org workspace deletes a namespace claim after explicit confirmation', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain(NAMESPACE_CLAIM_VALUE);
      });

      click(
        queryRequiredButton(
          target,
          `#namespace-delete-toggle-${NAMESPACE_CLAIM_ID}`
        )
      );

      await waitFor(() => {
        expect(
          queryRequiredFormBySelector(
            target,
            `#namespace-delete-form-${NAMESPACE_CLAIM_ID}`
          )
        ).toBeDefined();
      });

      setChecked(
        queryCheckbox(
          target,
          `#namespace-delete-confirm-${NAMESPACE_CLAIM_ID}`
        ),
        true
      );
      submitForm(
        queryRequiredFormBySelector(
          target,
          `#namespace-delete-form-${NAMESPACE_CLAIM_ID}`
        )
      );

      await waitFor(() => {
        expect(scenario.namespaceDeleteCalls).toEqual([
          `/v1/namespaces/${NAMESPACE_CLAIM_ID}`,
        ]);
        expect(target.textContent).toContain(
          `Deleted namespace claim ${NAMESPACE_CLAIM_VALUE}.`
        );
      });

      expect(
        target.querySelector(`#namespace-delete-toggle-${NAMESPACE_CLAIM_ID}`)
      ).toBeNull();
    } finally {
      unmount();
    }
  });

  test('org workspace keeps namespace delete confirmation open when deletion fails', async () => {
    const scenario = createFetchScenario();
    scenario.namespaceDeleteError = 'Failed to delete namespace claim.';
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain(NAMESPACE_CLAIM_VALUE);
      });

      click(
        queryRequiredButton(
          target,
          `#namespace-delete-toggle-${NAMESPACE_CLAIM_ID}`
        )
      );

      await waitFor(() => {
        expect(
          queryRequiredFormBySelector(
            target,
            `#namespace-delete-form-${NAMESPACE_CLAIM_ID}`
          )
        ).toBeDefined();
      });

      setChecked(
        queryCheckbox(
          target,
          `#namespace-delete-confirm-${NAMESPACE_CLAIM_ID}`
        ),
        true
      );
      submitForm(
        queryRequiredFormBySelector(
          target,
          `#namespace-delete-form-${NAMESPACE_CLAIM_ID}`
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Failed to delete namespace claim.'
        );
        expect(scenario.namespaceDeleteCalls).toEqual([]);
        expect(
          queryRequiredFormBySelector(
            target,
            `#namespace-delete-form-${NAMESPACE_CLAIM_ID}`
          )
        ).toBeDefined();
      });

      expect(scenario.namespaces.map((claim) => claim.id)).toContain(
        NAMESPACE_CLAIM_ID
      );
      expect(target.textContent).toContain(NAMESPACE_CLAIM_VALUE);
    } finally {
      unmount();
    }
  });

  test('org workspace requires explicit confirmation before revoking an invitation', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain(ACTIVE_INVITEE_EMAIL);
      });

      click(
        queryRequiredButton(
          target,
          `#invitation-revoke-toggle-${ACTIVE_INVITATION_ID}`
        )
      );

      await waitFor(() => {
        expect(
          queryRequiredFormBySelector(
            target,
            `#invitation-revoke-form-${ACTIVE_INVITATION_ID}`
          )
        ).toBeDefined();
      });

      submitForm(
        queryRequiredFormBySelector(
          target,
          `#invitation-revoke-form-${ACTIVE_INVITATION_ID}`
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Please confirm that you want to revoke this invitation immediately.'
        );
      });

      expect(scenario.invitationRevokeCalls).toEqual([]);
      expect(target.textContent).toContain(ACTIVE_INVITEE_EMAIL);
    } finally {
      unmount();
    }
  });

  test('org workspace revokes an invitation after explicit confirmation', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain(ACTIVE_INVITEE_EMAIL);
      });

      click(
        queryRequiredButton(
          target,
          `#invitation-revoke-toggle-${ACTIVE_INVITATION_ID}`
        )
      );

      await waitFor(() => {
        expect(
          queryRequiredFormBySelector(
            target,
            `#invitation-revoke-form-${ACTIVE_INVITATION_ID}`
          )
        ).toBeDefined();
      });

      setChecked(
        queryCheckbox(
          target,
          `#invitation-revoke-confirm-${ACTIVE_INVITATION_ID}`
        ),
        true
      );
      submitForm(
        queryRequiredFormBySelector(
          target,
          `#invitation-revoke-form-${ACTIVE_INVITATION_ID}`
        )
      );

      await waitFor(() => {
        expect(scenario.invitationRevokeCalls).toEqual([
          `/v1/orgs/${ORG_SLUG}/invitations/${ACTIVE_INVITATION_ID}`,
        ]);
        expect(target.textContent).toContain('Invitation revoked.');
      });

      expect(target.textContent).not.toContain(ACTIVE_INVITEE_EMAIL);
    } finally {
      unmount();
    }
  });

  test('org workspace keeps invitation revoke confirmation open when revocation fails', async () => {
    const scenario = createFetchScenario();
    scenario.invitationRevokeError = 'Failed to revoke invitation.';
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain(ACTIVE_INVITEE_EMAIL);
      });

      click(
        queryRequiredButton(
          target,
          `#invitation-revoke-toggle-${ACTIVE_INVITATION_ID}`
        )
      );

      await waitFor(() => {
        expect(
          queryRequiredFormBySelector(
            target,
            `#invitation-revoke-form-${ACTIVE_INVITATION_ID}`
          )
        ).toBeDefined();
      });

      setChecked(
        queryCheckbox(
          target,
          `#invitation-revoke-confirm-${ACTIVE_INVITATION_ID}`
        ),
        true
      );
      submitForm(
        queryRequiredFormBySelector(
          target,
          `#invitation-revoke-form-${ACTIVE_INVITATION_ID}`
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain('Failed to revoke invitation.');
        expect(scenario.invitationRevokeCalls).toEqual([]);
        expect(
          queryRequiredFormBySelector(
            target,
            `#invitation-revoke-form-${ACTIVE_INVITATION_ID}`
          )
        ).toBeDefined();
      });

      expect(target.textContent).toContain(ACTIVE_INVITEE_EMAIL);
    } finally {
      unmount();
    }
  });

  test('org workspace requires explicit confirmation before removing a member', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Admin User');
      });

      click(queryRequiredButton(target, '#member-remove-toggle-admin-user'));

      await waitFor(() => {
        expect(
          queryRequiredFormBySelector(target, '#member-remove-form-admin-user')
        ).toBeDefined();
      });

      submitForm(
        queryRequiredFormBySelector(target, '#member-remove-form-admin-user')
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Please confirm that you want to remove this member from the organization.'
        );
      });

      expect(scenario.memberRemoveCalls).toEqual([]);
      expect(target.textContent).toContain('Admin User');
    } finally {
      unmount();
    }
  });

  test('org workspace removes a member after explicit confirmation', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Admin User');
      });

      click(queryRequiredButton(target, '#member-remove-toggle-admin-user'));

      await waitFor(() => {
        expect(
          queryRequiredFormBySelector(target, '#member-remove-form-admin-user')
        ).toBeDefined();
      });

      setChecked(
        queryCheckbox(target, '#member-remove-confirm-admin-user'),
        true
      );
      submitForm(
        queryRequiredFormBySelector(target, '#member-remove-form-admin-user')
      );

      await waitFor(() => {
        expect(scenario.memberRemoveCalls).toEqual([
          `/v1/orgs/${ORG_SLUG}/members/admin-user`,
        ]);
        expect(target.textContent).toContain(
          'Removed @admin-user from the organization.'
        );
      });

      expect(
        target.querySelector('#member-remove-toggle-admin-user')
      ).toBeNull();
    } finally {
      unmount();
    }
  });

  test('org workspace keeps member removal confirmation open when removal fails', async () => {
    const scenario = createFetchScenario();
    scenario.memberRemoveError = 'Failed to remove member.';
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Admin User');
      });

      click(queryRequiredButton(target, '#member-remove-toggle-admin-user'));

      await waitFor(() => {
        expect(
          queryRequiredFormBySelector(target, '#member-remove-form-admin-user')
        ).toBeDefined();
      });

      setChecked(
        queryCheckbox(target, '#member-remove-confirm-admin-user'),
        true
      );
      submitForm(
        queryRequiredFormBySelector(target, '#member-remove-form-admin-user')
      );

      await waitFor(() => {
        expect(target.textContent).toContain('Failed to remove member.');
        expect(scenario.memberRemoveCalls).toEqual([]);
        expect(
          queryRequiredFormBySelector(target, '#member-remove-form-admin-user')
        ).toBeDefined();
      });

      expect(target.textContent).toContain('Admin User');
    } finally {
      unmount();
    }
  });

  test('org workspace requires explicit confirmation before transferring ownership', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Transfer ownership');
      });

      changeValue(
        queryRequiredInput(target, '#org-transfer-owner'),
        members[1]?.username || 'admin-user'
      );
      click(queryRequiredButton(target, '#org-ownership-transfer-toggle'));

      await waitFor(() => {
        expect(
          queryCheckbox(target, '#org-ownership-transfer-confirm')
        ).toBeDefined();
      });

      submitForm(
        queryRequiredForm(
          queryRequiredInput(target, '#org-transfer-owner').closest('form')
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Please confirm the ownership transfer.'
        );
      });

      expect(scenario.ownershipTransfers).toEqual([]);
      expect(
        queryRequiredButton(target, '#org-ownership-transfer-submit')
      ).toBeDefined();
    } finally {
      unmount();
    }
  });

  test('org workspace transfers ownership after explicit confirmation', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Transfer ownership');
      });

      const targetUsername = members[1]?.username || 'admin-user';
      changeValue(
        queryRequiredInput(target, '#org-transfer-owner'),
        targetUsername
      );
      click(queryRequiredButton(target, '#org-ownership-transfer-toggle'));

      await waitFor(() => {
        expect(
          queryCheckbox(target, '#org-ownership-transfer-confirm')
        ).toBeDefined();
      });

      setChecked(
        queryCheckbox(target, '#org-ownership-transfer-confirm'),
        true
      );
      submitForm(
        queryRequiredForm(
          queryRequiredInput(target, '#org-transfer-owner').closest('form')
        )
      );

      await waitFor(() => {
        expect(scenario.ownershipTransfers).toEqual([
          {
            path: `/v1/orgs/${ORG_SLUG}/ownership-transfer`,
            body: { username: targetUsername },
          },
        ]);
        expect(target.textContent).toContain(
          `Ownership transferred to @${targetUsername}.`
        );
      });
    } finally {
      unmount();
    }
  });

  test('org workspace keeps ownership transfer confirmation open when transfer fails', async () => {
    const scenario = createFetchScenario();
    scenario.ownershipTransferError =
      'Failed to transfer organization ownership.';
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Transfer ownership');
      });

      changeValue(
        queryRequiredInput(target, '#org-transfer-owner'),
        members[1]?.username || 'admin-user'
      );
      click(queryRequiredButton(target, '#org-ownership-transfer-toggle'));

      await waitFor(() => {
        expect(
          queryCheckbox(target, '#org-ownership-transfer-confirm')
        ).toBeDefined();
      });

      setChecked(
        queryCheckbox(target, '#org-ownership-transfer-confirm'),
        true
      );
      submitForm(
        queryRequiredForm(
          queryRequiredInput(target, '#org-transfer-owner').closest('form')
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Failed to transfer organization ownership.'
        );
        expect(scenario.ownershipTransfers).toEqual([]);
        expect(
          queryRequiredButton(target, '#org-ownership-transfer-submit')
        ).toBeDefined();
      });
    } finally {
      unmount();
    }
  });

  test('org workspace requires explicit confirmation before transferring a namespace claim', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Transfer a namespace');
      });

      changeValue(
        queryRequiredSelect(target, '#org-namespace-transfer-claim'),
        NAMESPACE_CLAIM_ID
      );
      changeValue(
        queryRequiredSelect(target, '#org-namespace-transfer-target'),
        TARGET_ORG_SLUG
      );
      click(queryRequiredButton(target, '#org-namespace-transfer-toggle'));

      await waitFor(() => {
        expect(
          queryCheckbox(target, '#org-namespace-transfer-confirm')
        ).toBeDefined();
      });

      submitForm(
        queryRequiredForm(
          queryRequiredSelect(target, '#org-namespace-transfer-claim').closest(
            'form'
          )
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Please confirm the namespace transfer.'
        );
      });

      expect(scenario.namespaceTransfers).toEqual([]);
      expect(
        queryRequiredButton(target, '#org-namespace-transfer-submit')
      ).toBeDefined();
    } finally {
      unmount();
    }
  });

  test('org workspace transfers a namespace claim after explicit confirmation', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Transfer a namespace');
      });

      changeValue(
        queryRequiredSelect(target, '#org-namespace-transfer-claim'),
        NAMESPACE_CLAIM_ID
      );
      changeValue(
        queryRequiredSelect(target, '#org-namespace-transfer-target'),
        TARGET_ORG_SLUG
      );
      click(queryRequiredButton(target, '#org-namespace-transfer-toggle'));

      await waitFor(() => {
        expect(
          queryCheckbox(target, '#org-namespace-transfer-confirm')
        ).toBeDefined();
      });

      setChecked(
        queryCheckbox(target, '#org-namespace-transfer-confirm'),
        true
      );
      submitForm(
        queryRequiredForm(
          queryRequiredSelect(target, '#org-namespace-transfer-claim').closest(
            'form'
          )
        )
      );

      await waitFor(() => {
        expect(scenario.namespaceTransfers).toEqual([
          {
            path: `/v1/namespaces/${NAMESPACE_CLAIM_ID}/ownership-transfer`,
            body: { target_org_slug: TARGET_ORG_SLUG },
          },
        ]);
        expect(target.textContent).toContain(
          `Transferred ${NAMESPACE_CLAIM_VALUE} to ${TARGET_ORG_SLUG}.`
        );
      });
    } finally {
      unmount();
    }
  });

  test('org workspace keeps namespace transfer confirmation open when transfer fails', async () => {
    const scenario = createFetchScenario();
    scenario.namespaceTransferError =
      'Failed to transfer namespace claim ownership.';
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Transfer a namespace');
      });

      changeValue(
        queryRequiredSelect(target, '#org-namespace-transfer-claim'),
        NAMESPACE_CLAIM_ID
      );
      changeValue(
        queryRequiredSelect(target, '#org-namespace-transfer-target'),
        TARGET_ORG_SLUG
      );
      click(queryRequiredButton(target, '#org-namespace-transfer-toggle'));

      await waitFor(() => {
        expect(
          queryCheckbox(target, '#org-namespace-transfer-confirm')
        ).toBeDefined();
      });

      setChecked(
        queryCheckbox(target, '#org-namespace-transfer-confirm'),
        true
      );
      submitForm(
        queryRequiredForm(
          queryRequiredSelect(target, '#org-namespace-transfer-claim').closest(
            'form'
          )
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Failed to transfer namespace claim ownership.'
        );
        expect(scenario.namespaceTransfers).toEqual([]);
        expect(
          queryRequiredButton(target, '#org-namespace-transfer-submit')
        ).toBeDefined();
      });
    } finally {
      unmount();
    }
  });

  test('org workspace requires explicit confirmation before transferring a repository', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Transfer repository ownership');
      });

      changeValue(
        queryRequiredSelect(target, '#org-repository-transfer-repository'),
        repositories[0]?.slug || ''
      );
      changeValue(
        queryRequiredSelect(target, '#org-repository-transfer-target'),
        TARGET_ORG_SLUG
      );
      click(queryRequiredButton(target, '#org-repository-transfer-toggle'));

      await waitFor(() => {
        expect(
          queryCheckbox(target, '#org-repository-transfer-confirm')
        ).toBeDefined();
      });

      submitForm(
        queryRequiredForm(
          queryRequiredSelect(
            target,
            '#org-repository-transfer-repository'
          ).closest('form')
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Please confirm the repository transfer.'
        );
      });

      expect(scenario.repositoryTransfers).toEqual([]);
      expect(
        queryRequiredButton(target, '#org-repository-transfer-submit')
      ).toBeDefined();
    } finally {
      unmount();
    }
  });

  test('org workspace keeps repository transfer confirmation open when transfer fails', async () => {
    const scenario = createFetchScenario();
    scenario.repositoryTransferError =
      'Failed to transfer repository ownership.';
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Transfer repository ownership');
      });

      changeValue(
        queryRequiredSelect(target, '#org-repository-transfer-repository'),
        repositories[0]?.slug || ''
      );
      changeValue(
        queryRequiredSelect(target, '#org-repository-transfer-target'),
        TARGET_ORG_SLUG
      );
      click(queryRequiredButton(target, '#org-repository-transfer-toggle'));

      await waitFor(() => {
        expect(
          queryCheckbox(target, '#org-repository-transfer-confirm')
        ).toBeDefined();
      });

      setChecked(
        queryCheckbox(target, '#org-repository-transfer-confirm'),
        true
      );
      submitForm(
        queryRequiredForm(
          queryRequiredSelect(
            target,
            '#org-repository-transfer-repository'
          ).closest('form')
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Failed to transfer repository ownership.'
        );
        expect(scenario.repositoryTransfers).toEqual([]);
        expect(
          queryRequiredButton(target, '#org-repository-transfer-submit')
        ).toBeDefined();
      });
    } finally {
      unmount();
    }
  });

  test('org workspace requires explicit confirmation before transferring a package', async () => {
    const scenario = createFetchScenario();
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Transfer package ownership');
      });

      changeValue(
        queryRequiredSelect(target, '#org-package-transfer-package'),
        renderPackageSelectionValue(packages[0]?.ecosystem, packages[0]?.name)
      );
      changeValue(
        queryRequiredSelect(target, '#org-package-transfer-target'),
        TARGET_ORG_SLUG
      );
      click(queryRequiredButton(target, '#org-package-transfer-toggle'));

      await waitFor(() => {
        expect(
          queryCheckbox(target, '#org-package-transfer-confirm')
        ).toBeDefined();
      });

      submitForm(
        queryRequiredForm(
          queryRequiredSelect(target, '#org-package-transfer-package').closest(
            'form'
          )
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Please confirm the package transfer.'
        );
      });

      expect(scenario.packageTransfers).toEqual([]);
      expect(
        queryRequiredButton(target, '#org-package-transfer-submit')
      ).toBeDefined();
    } finally {
      unmount();
    }
  });

  test('org workspace keeps package transfer confirmation open when transfer fails', async () => {
    const scenario = createFetchScenario();
    scenario.packageTransferError = 'Failed to transfer package ownership.';
    const { target, unmount } = await mountOrgPage(scenario);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Transfer package ownership');
      });

      changeValue(
        queryRequiredSelect(target, '#org-package-transfer-package'),
        renderPackageSelectionValue(packages[0]?.ecosystem, packages[0]?.name)
      );
      changeValue(
        queryRequiredSelect(target, '#org-package-transfer-target'),
        TARGET_ORG_SLUG
      );
      click(queryRequiredButton(target, '#org-package-transfer-toggle'));

      await waitFor(() => {
        expect(
          queryCheckbox(target, '#org-package-transfer-confirm')
        ).toBeDefined();
      });

      setChecked(queryCheckbox(target, '#org-package-transfer-confirm'), true);
      submitForm(
        queryRequiredForm(
          queryRequiredSelect(target, '#org-package-transfer-package').closest(
            'form'
          )
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Failed to transfer package ownership.'
        );
        expect(scenario.packageTransfers).toEqual([]);
        expect(
          queryRequiredButton(target, '#org-package-transfer-submit')
        ).toBeDefined();
      });
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
      expect(
        target.querySelector('#org-repository-transfer-repository')
      ).toBeNull();
      expect(
        target.querySelector('#repository-visibility-repo-001')
      ).toBeNull();
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

      expect(
        target.querySelector(`#team-repository-${TEAM_SLUG}`)
      ).not.toBeNull();
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
            queryRequiredSelect(
              packageRender.target,
              `#team-package-${TEAM_SLUG}`
            )
          )
        ).toContain(finalPackageKey);
      });

      const packageGrantSelect = queryRequiredSelect(
        packageRender.target,
        `#team-package-${TEAM_SLUG}`
      );
      const packageGrantForm = queryRequiredForm(
        packageGrantSelect.closest('form')
      );
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
      changeValue(repositoryTransferSelect, finalRepository?.slug || '');
      changeValue(repositoryTransferTarget, TARGET_ORG_SLUG);
      click(
        queryRequiredButton(
          repositoryTransferForm,
          '#org-repository-transfer-toggle'
        )
      );
      await waitFor(() => {
        expect(
          queryCheckbox(
            repositoryTransferForm,
            '#org-repository-transfer-confirm'
          )
        ).toBeDefined();
      });
      const repositoryTransferConfirm = queryCheckbox(
        repositoryTransferForm,
        '#org-repository-transfer-confirm'
      );
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
      changeValue(packageTransferSelect, finalPackageKey);
      changeValue(packageTransferTarget, TARGET_ORG_SLUG);
      click(
        queryRequiredButton(packageTransferForm, '#org-package-transfer-toggle')
      );
      await waitFor(() => {
        expect(
          queryCheckbox(packageTransferForm, '#org-package-transfer-confirm')
        ).toBeDefined();
      });
      const packageTransferConfirm = queryCheckbox(
        packageTransferForm,
        '#org-package-transfer-confirm'
      );
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

    const { target, unmount, flush } = await renderSvelte(SEARCH_PAGE_PATH);

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
        expect(gotoCalls).toEqual([
          '/search?org=source-org&repository=repo-101',
        ]);
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

    const { target, unmount } = await renderSvelte(SEARCH_PAGE_PATH);

    try {
      await waitFor(() => {
        expect(
          target.querySelector(
            'a[href="/packages/npm/example-package?tab=security"]'
          )
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
    teamDeleteError: null,
    namespaceDeleteError: null,
    invitationRevokeError: null,
    memberRemoveError: null,
    ownershipTransferError: null,
    namespaceTransferError: null,
    repositoryTransferError: null,
    packageTransferError: null,
    members: members.map((member) => ({ ...member })),
    invitations: [
      {
        id: ACTIVE_INVITATION_ID,
        status: 'pending',
        role: 'maintainer',
        invited_user: {
          username: 'new-maintainer',
          email: ACTIVE_INVITEE_EMAIL,
        },
        invited_by: {
          username: 'owner-user',
        },
        created_at: '2026-04-03T00:00:00Z',
        expires_at: '2026-04-10T00:00:00Z',
      },
    ],
    teams: [
      {
        name: 'Release Engineering',
        slug: TEAM_SLUG,
        description: 'Owns release governance',
        created_at: '2026-04-01T00:00:00Z',
      },
    ],
    namespaces: [
      {
        id: NAMESPACE_CLAIM_ID,
        ecosystem: 'npm',
        namespace: NAMESPACE_CLAIM_VALUE,
        owner_org_id: ORG_ID,
        is_verified: true,
        created_at: '2026-04-01T00:00:00Z',
        can_manage: true,
        can_transfer: true,
      },
    ],
    workspaceBootstrapRequests: [],
    teamMemberRequests: [],
    teamPackageAccessRequests: [],
    teamRepositoryAccessRequests: [],
    teamNamespaceAccessRequests: [],
    repositoryPageRequests: [],
    repositoryPackageCoverageRequests: [],
    packagePageRequests: [],
    invitationRequests: [],
    invitationRevokeCalls: [],
    memberRemoveCalls: [],
    orgUpdateCalls: [],
    ownershipTransfers: [],
    orgName: 'Source Org',
    orgMfaRequired: false,
    orgMemberDirectoryIsPrivate: false,
    teamRepositoryAccessUpdates: [],
    teamPackageAccessUpdates: [],
    teamDeleteCalls: [],
    namespaceDeleteCalls: [],
    namespaceTransfers: [],
    repositoryTransfers: [],
    packageTransfers: [],
    searchCalls: [],
    auditLogs: auditLogs.map((log) => ({
      ...log,
      metadata: { ...log.metadata },
    })),
    auditRequests: [],
    auditExportRequests: [],
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

  if (method === 'GET' && requestPath === `/v1/orgs/${ORG_SLUG}/workspace`) {
    scenario.workspaceBootstrapRequests.push(requestPath);
    return apiResponse({
      org: {
        id: ORG_ID,
        name: scenario.orgName,
        slug: ORG_SLUG,
        description: 'Source organization',
        is_verified: true,
        mfa_required: scenario.orgMfaRequired,
        member_directory_is_private: scenario.orgMemberDirectoryIsPrivate,
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
      },
      teams: scenario.teams,
      repositories,
      repository_package_coverage: [
        {
          repository_slug: 'repo-001',
          packages: [
            {
              id: 'repo-package-001',
              ecosystem: 'npm',
              name: 'repo-package-001',
              description: 'Repository scoped package',
              latest_version: '1.0.0',
              download_count: 7,
              created_at: '2026-04-01T00:00:00Z',
            },
          ],
        },
      ],
      packages,
      namespaces: scenario.namespaces,
      invitations: scenario.canManageInvitations ? scenario.invitations : [],
      team_management: {
        members_by_team_slug: scenario.canManageTeams
          ? {
              [TEAM_SLUG]: [
                {
                  display_name: 'Admin User',
                  username: 'admin-user',
                  added_at: '2026-04-03T00:00:00Z',
                },
              ],
            }
          : {},
        package_access_by_team_slug: scenario.canManageTeams
          ? {
              [TEAM_SLUG]: [
                {
                  package_id: packages.at(-1)?.id,
                  ecosystem: packages.at(-1)?.ecosystem,
                  name: packages.at(-1)?.name,
                  normalized_name: packages.at(-1)?.name,
                  permissions: ['write_metadata'],
                  granted_at: '2026-04-03T00:00:00Z',
                },
              ],
            }
          : {},
        repository_access_by_team_slug: scenario.canManageRepositories
          ? {
              [TEAM_SLUG]: [
                {
                  repository_id: repositories.at(-1)?.id,
                  name: repositories.at(-1)?.name,
                  slug: repositories.at(-1)?.slug,
                  kind: repositories.at(-1)?.kind,
                  visibility: repositories.at(-1)?.visibility,
                  permissions: ['publish'],
                  granted_at: '2026-04-03T00:00:00Z',
                },
              ],
            }
          : {},
        namespace_access_by_team_slug: scenario.canManageNamespaces
          ? {
              [TEAM_SLUG]: [
                {
                  namespace_claim_id: 'claim-001',
                  ecosystem: 'npm',
                  namespace: '@source-org',
                  is_verified: true,
                  permissions: ['admin'],
                  granted_at: '2026-04-03T00:00:00Z',
                },
              ],
            }
          : {},
      },
      security: {
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
      },
    });
  }

  if (method === 'GET' && requestPath === `/v1/orgs/${ORG_SLUG}`) {
    return apiResponse({
      id: ORG_ID,
      name: 'Source Org',
      slug: ORG_SLUG,
      description: 'Source organization',
      is_verified: true,
      mfa_required: scenario.orgMfaRequired,
      member_directory_is_private: scenario.orgMemberDirectoryIsPrivate,
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
    requestPath.endsWith('/repository-package-coverage')
  ) {
    scenario.repositoryPackageCoverageRequests.push(requestPath);
    return apiResponse({
      repositories: [
        {
          repository_slug: 'repo-001',
          packages: [
            {
              id: 'repo-package-001',
              ecosystem: 'npm',
              name: 'repo-package-001',
              description: 'Repository scoped package',
              latest_version: '1.0.0',
              download_count: 7,
              created_at: '2026-04-01T00:00:00Z',
            },
          ],
        },
      ],
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
    return apiResponse({ invitations: scenario.invitations });
  }

  if (method === 'GET' && requestPath === `/v1/orgs/${ORG_SLUG}/members`) {
    return apiResponse({ members: scenario.members });
  }

  if (
    method === 'DELETE' &&
    requestPath === `/v1/orgs/${ORG_SLUG}/invitations/${ACTIVE_INVITATION_ID}`
  ) {
    if (scenario.invitationRevokeError) {
      throw new TestApiError(500, { error: scenario.invitationRevokeError });
    }
    scenario.invitationRevokeCalls.push(requestPath);
    scenario.invitations = scenario.invitations.filter(
      (invitation) => invitation.id !== ACTIVE_INVITATION_ID
    );
    return apiResponse(null);
  }

  if (
    method === 'DELETE' &&
    requestPath === `/v1/orgs/${ORG_SLUG}/members/admin-user`
  ) {
    if (scenario.memberRemoveError) {
      throw new TestApiError(500, { error: scenario.memberRemoveError });
    }
    scenario.memberRemoveCalls.push(requestPath);
    scenario.members = scenario.members.filter(
      (member) => member.username !== 'admin-user'
    );
    return apiResponse(null);
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
    const request = readAuditRequest(url);
    const filteredLogs = filterAuditLogs(scenario.auditLogs, request);
    scenario.auditRequests.push(request);

    return apiResponse({
      page: request.page,
      per_page: request.perPage,
      has_next: request.page * request.perPage < filteredLogs.length,
      logs: paginate(filteredLogs, request.page, request.perPage),
    });
  }

  if (method === 'GET' && requestPath === `/v1/orgs/${ORG_SLUG}/audit/export`) {
    scenario.auditExportRequests.push(readAuditExportRequest(url));
    return apiResponse('id,action\naudit-001,team_update\n');
  }

  if (
    method === 'GET' &&
    requestPath === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/members`
  ) {
    scenario.teamMemberRequests.push(requestPath);
    return apiResponse({ members: [] });
  }

  if (
    method === 'GET' &&
    requestPath === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access`
  ) {
    scenario.teamPackageAccessRequests.push(requestPath);
    return apiResponse({ package_access: [] });
  }

  if (
    method === 'GET' &&
    requestPath === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access`
  ) {
    scenario.teamRepositoryAccessRequests.push(requestPath);
    return apiResponse({ repository_access: [] });
  }

  if (
    method === 'GET' &&
    requestPath === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/namespace-access`
  ) {
    scenario.teamNamespaceAccessRequests.push(requestPath);
    return apiResponse({ namespace_access: [] });
  }

  if (method === 'GET' && requestPath === '/v1/namespaces') {
    return apiResponse({ namespaces: [] });
  }

  if (method === 'PATCH' && requestPath === `/v1/orgs/${ORG_SLUG}`) {
    scenario.orgUpdateCalls.push({ path: requestPath, body });
    scenario.orgName =
      typeof body.name === 'string' && body.name.trim().length > 0
        ? body.name.trim()
        : scenario.orgName;
    scenario.orgMfaRequired = body.mfa_required === true;
    scenario.orgMemberDirectoryIsPrivate =
      body.member_directory_is_private === true;
    return apiResponse({ message: 'Organization updated' });
  }

  if (
    method === 'POST' &&
    requestPath === `/v1/orgs/${ORG_SLUG}/ownership-transfer`
  ) {
    if (scenario.ownershipTransferError) {
      throw new TestApiError(500, { error: scenario.ownershipTransferError });
    }
    scenario.ownershipTransfers.push({ path: requestPath, body });
    return apiResponse({
      new_owner: {
        username: body.username,
      },
    });
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
    method === 'DELETE' &&
    requestPath === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}`
  ) {
    if (scenario.teamDeleteError) {
      throw new TestApiError(500, { error: scenario.teamDeleteError });
    }
    scenario.teamDeleteCalls.push(requestPath);
    scenario.teams = scenario.teams.filter((team) => team.slug !== TEAM_SLUG);
    return apiResponse({ message: 'Deleted team' });
  }

  if (
    method === 'DELETE' &&
    requestPath === `/v1/namespaces/${NAMESPACE_CLAIM_ID}`
  ) {
    if (scenario.namespaceDeleteError) {
      throw new TestApiError(500, { error: scenario.namespaceDeleteError });
    }
    scenario.namespaceDeleteCalls.push(requestPath);
    scenario.namespaces = scenario.namespaces.filter(
      (claim) => claim.id !== NAMESPACE_CLAIM_ID
    );
    return apiResponse(null);
  }

  if (
    method === 'POST' &&
    requestPath.startsWith('/v1/namespaces/') &&
    requestPath.endsWith('/ownership-transfer')
  ) {
    if (scenario.namespaceTransferError) {
      throw new TestApiError(500, { error: scenario.namespaceTransferError });
    }
    scenario.namespaceTransfers.push({ path: requestPath, body });
    return apiResponse({
      namespace_claim: {
        id: requestPath.split('/')[3],
        namespace: NAMESPACE_CLAIM_VALUE,
      },
      owner: {
        slug: body.target_org_slug,
      },
    });
  }

  if (
    method === 'POST' &&
    requestPath.startsWith('/v1/repositories/') &&
    requestPath.endsWith('/ownership-transfer')
  ) {
    if (scenario.repositoryTransferError) {
      throw new TestApiError(500, { error: scenario.repositoryTransferError });
    }
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
    if (scenario.packageTransferError) {
      throw new TestApiError(500, { error: scenario.packageTransferError });
    }
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

function readAuditRequest(url: URL): AuditRequest {
  return {
    action: url.searchParams.get('action') || '',
    actorUserId: url.searchParams.get('actor_user_id') || '',
    occurredFrom: url.searchParams.get('occurred_from') || '',
    occurredUntil: url.searchParams.get('occurred_until') || '',
    page: parsePage(url.searchParams.get('page')),
    perPage: parsePerPage(url.searchParams.get('per_page'), 20),
  };
}

function readAuditExportRequest(url: URL): AuditExportRequest {
  return {
    action: url.searchParams.get('action') || '',
    actorUserId: url.searchParams.get('actor_user_id') || '',
    occurredFrom: url.searchParams.get('occurred_from') || '',
    occurredUntil: url.searchParams.get('occurred_until') || '',
    page: url.searchParams.get('page'),
    perPage: url.searchParams.get('per_page'),
  };
}

function filterAuditLogs(
  logs: AuditLogFixture[],
  request: AuditRequest
): AuditLogFixture[] {
  return logs.filter((log) => {
    if (request.action && log.action !== request.action) {
      return false;
    }
    if (request.actorUserId && log.actor_user_id !== request.actorUserId) {
      return false;
    }

    const occurredDate = log.occurred_at.slice(0, 10);
    if (request.occurredFrom && occurredDate < request.occurredFrom) {
      return false;
    }
    if (request.occurredUntil && occurredDate > request.occurredUntil) {
      return false;
    }

    return true;
  });
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

function queryRequiredInput(
  root: ParentNode | Element | null,
  selector: string
): HTMLInputElement {
  const element = root?.querySelector(selector);
  if (!(element instanceof HTMLInputElement)) {
    throw new Error(`Expected input for selector: ${selector}`);
  }

  return element;
}

function queryRequiredForm(root: Element | ParentNode | null): HTMLFormElement {
  if (!(root instanceof HTMLFormElement)) {
    throw new Error('Expected form element.');
  }

  return root;
}

function queryRequiredFormBySelector(
  root: ParentNode | Element,
  selector: string
): HTMLFormElement {
  return queryRequiredForm(root.querySelector(selector));
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

function queryRequiredButton(
  root: ParentNode | Element,
  selector: string
): HTMLButtonElement {
  const element = root.querySelector(selector);
  if (!(element instanceof HTMLButtonElement)) {
    throw new Error(`Expected button for selector: ${selector}`);
  }

  return element;
}

function queryButtonByText(
  root: ParentNode | Element,
  text: string
): HTMLButtonElement | null {
  return (
    Array.from(root.querySelectorAll('button')).find(
      (button): button is HTMLButtonElement =>
        button instanceof HTMLButtonElement &&
        button.textContent?.trim() === text
    ) || null
  );
}

function queryRequiredButtonByText(
  root: ParentNode | Element,
  text: string
): HTMLButtonElement {
  const button = queryButtonByText(root, text);
  if (!button) {
    throw new Error(`Expected button with text: ${text}`);
  }

  return button;
}

function optionValues(select: HTMLSelectElement): string[] {
  return Array.from(select.options).map((option) => option.value);
}

async function mountOrgPage(
  scenario: FetchScenario,
  search = ''
): Promise<Awaited<ReturnType<typeof renderSvelte>>> {
  currentScenario = scenario;
  currentAuthToken = 'pub_test_token';
  pageStore.set(
    buildPageState(`https://example.test/orgs/${ORG_SLUG}${search}`, {
      slug: ORG_SLUG,
    })
  );
  return renderSvelte(ORG_PAGE_PATH);
}
