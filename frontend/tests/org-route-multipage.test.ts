/// <reference path="./bun-test.d.ts" />

import { afterEach, describe, expect, mock, test } from 'bun:test';
import { writable } from 'svelte/store';

import { setAuthToken } from '../src/api/client';
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
  repositoryPageRequests: number[];
  packagePageRequests: number[];
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
const originalFetch = globalThis.fetch;
const gotoCalls: string[] = [];
const pageStore = writable<TestPageState>(buildPageState('https://example.test/'));

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
};

const targetOrgMembership = {
  id: '22222222-2222-4222-8222-222222222222',
  slug: TARGET_ORG_SLUG,
  name: 'Target Org',
  role: 'admin',
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

const SearchPage = await import('../src/routes/search/+page.svelte');
const OrgPage = await import('../src/routes/orgs/[slug]/+page.svelte');

afterEach(() => {
  gotoCalls.length = 0;
  setAuthToken(null);
  pageStore.set(buildPageState('https://example.test/'));
  globalThis.fetch = originalFetch;
});

describe('route-level multi-page org dataset coverage', () => {
  test('org workspace delegated access forms and transfer selectors include second-page packages and repositories', async () => {
    const scenario = createFetchScenario();
    installFetchScenario(scenario);
    setAuthToken('pub_test_token');
    pageStore.set(buildPageState(`https://example.test/orgs/${ORG_SLUG}`, {
      slug: ORG_SLUG,
    }));

    const { target, unmount } = await renderSvelte(OrgPage);

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
      });

      const repositoryGrantSelect = queryRequiredSelect(
        target,
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
        expect(scenario.teamRepositoryAccessUpdates).toHaveLength(1);
      });
      expect(scenario.teamRepositoryAccessUpdates[0]).toEqual({
        path: `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access/${finalRepository?.slug}`,
        body: { permissions: ['publish'] },
      });

      await waitFor(() => {
        expect(
          optionValues(queryRequiredSelect(target, `#team-package-${TEAM_SLUG}`))
        ).toContain(finalPackageKey);
      });

      const packageGrantSelect = queryRequiredSelect(
        target,
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
        expect(scenario.teamPackageAccessUpdates).toHaveLength(1);
      });
      expect(scenario.teamPackageAccessUpdates[0]).toEqual({
        path: `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access/npm/${finalPackage?.name}`,
        body: { permissions: ['publish'] },
      });

      await waitFor(() => {
        expect(
          optionValues(
            queryRequiredSelect(target, '#org-repository-transfer-repository')
          )
        ).toContain(finalRepository?.slug);
      });

      const repositoryTransferForm = queryRequiredForm(
        queryRequiredSelect(
          target,
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
        expect(scenario.repositoryTransfers).toHaveLength(1);
      });
      expect(scenario.repositoryTransfers[0]).toEqual({
        path: `/v1/repositories/${finalRepository?.slug}/ownership-transfer`,
        body: { target_org_slug: TARGET_ORG_SLUG },
      });

      await waitFor(() => {
        expect(
          optionValues(queryRequiredSelect(target, '#org-package-transfer-package'))
        ).toContain(finalPackageKey);
      });

      const packageTransferForm = queryRequiredForm(
        queryRequiredSelect(target, '#org-package-transfer-package').closest('form')
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
        expect(scenario.packageTransfers).toHaveLength(1);
      });
      expect(scenario.packageTransfers[0]).toEqual({
        path: `/v1/packages/npm/${finalPackage?.name}/ownership-transfer`,
        body: { target_org_slug: TARGET_ORG_SLUG },
      });
    } finally {
      unmount();
    }
  });

  test('org-scoped search loads repository filter options across multiple pages and submits the selected repository', async () => {
    const scenario = createFetchScenario();
    installFetchScenario(scenario);
    setAuthToken('pub_test_token');
    pageStore.set(buildPageState('https://example.test/search'));

    const { target, unmount } = await renderSvelte(SearchPage);

    try {
      await waitFor(() => {
        const organizationSelect = queryRequiredSelect(
          target,
          'select[aria-label="Organization scope"]'
        );
        expect(optionValues(organizationSelect)).toContain(ORG_SLUG);
      });

      const organizationSelect = queryRequiredSelect(
        target,
        'select[aria-label="Organization scope"]'
      );
      changeValue(organizationSelect, ORG_SLUG);

      await waitFor(() => {
        expect(scenario.repositoryPageRequests).toEqual([1, 2]);
        const repositorySelect = queryRequiredSelect(
          target,
          'select[aria-label="Repository scope"]'
        );
        expect(optionValues(repositorySelect)).toContain('repo-101');
      });

      const repositorySelect = queryRequiredSelect(
        target,
        'select[aria-label="Repository scope"]'
      );
      changeValue(repositorySelect, 'repo-101');

      const form = queryRequiredForm(target.querySelector('#search-form'));
      submitForm(form);

      await waitFor(() => {
        expect(gotoCalls).toEqual(['/search?org=source-org&repository=repo-101']);
      });
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
    repositoryPageRequests: [],
    packagePageRequests: [],
    teamRepositoryAccessUpdates: [],
    teamPackageAccessUpdates: [],
    repositoryTransfers: [],
    packageTransfers: [],
    searchCalls: [],
  };
}

function installFetchScenario(scenario: FetchScenario): void {
  globalThis.fetch = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(input.toString());
    const method = (init?.method || 'GET').toUpperCase();
    const path = url.pathname;
    const body = parseJsonBody(init?.body);

    if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}`) {
      return jsonResponse({
        id: ORG_ID,
        name: 'Source Org',
        slug: ORG_SLUG,
        description: 'Source organization',
        is_verified: true,
        website: null,
        email: null,
        created_at: '2026-04-01T00:00:00Z',
      });
    }

    if (method === 'GET' && path === '/v1/users/me/organizations') {
      return jsonResponse({
        organizations: [currentOrgMembership, targetOrgMembership],
      });
    }

    if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/repositories`) {
      const page = parsePage(url.searchParams.get('page'));
      const perPage = parsePerPage(url.searchParams.get('per_page'));
      scenario.repositoryPageRequests.push(page);
      return jsonResponse({
        repositories: paginate(repositories, page, perPage),
      });
    }

    if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/packages`) {
      const page = parsePage(url.searchParams.get('page'));
      const perPage = parsePerPage(url.searchParams.get('per_page'));
      scenario.packagePageRequests.push(page);
      return jsonResponse({
        packages: paginate(packages, page, perPage),
      });
    }

    if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/security-findings`) {
      return jsonResponse({
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

    if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/invitations`) {
      return jsonResponse({ invitations: [] });
    }

    if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/members`) {
      return jsonResponse({ members });
    }

    if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/teams`) {
      return jsonResponse({
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

    if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/audit`) {
      return jsonResponse({
        page: 1,
        per_page: 20,
        has_next: false,
        logs: [],
      });
    }

    if (
      method === 'GET' &&
      path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/members`
    ) {
      return jsonResponse({ members: [] });
    }

    if (
      method === 'GET' &&
      path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access`
    ) {
      return jsonResponse({ package_access: [] });
    }

    if (
      method === 'GET' &&
      path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access`
    ) {
      return jsonResponse({ repository_access: [] });
    }

    if (
      method === 'GET' &&
      path === `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/namespace-access`
    ) {
      return jsonResponse({ namespace_access: [] });
    }

    if (method === 'GET' && path === '/v1/namespaces') {
      return jsonResponse({ namespaces: [] });
    }

    if (method === 'GET' && path.startsWith('/v1/repositories/repo-')) {
      return jsonResponse({ packages: [] });
    }

    if (
      method === 'PUT' &&
      path.startsWith(
        `/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/repository-access/`
      )
    ) {
      scenario.teamRepositoryAccessUpdates.push({ path, body });
      return jsonResponse({ message: 'Saved repository access' });
    }

    if (
      method === 'PUT' &&
      path.startsWith(`/v1/orgs/${ORG_SLUG}/teams/${TEAM_SLUG}/package-access/`)
    ) {
      scenario.teamPackageAccessUpdates.push({ path, body });
      return jsonResponse({ message: 'Saved package access' });
    }

    if (
      method === 'POST' &&
      path.startsWith('/v1/repositories/') &&
      path.endsWith('/ownership-transfer')
    ) {
      scenario.repositoryTransfers.push({ path, body });
      return jsonResponse({
        repository: {
          slug: path.split('/')[3],
        },
        owner: {
          slug: body.target_org_slug,
        },
      });
    }

    if (
      method === 'POST' &&
      path.startsWith('/v1/packages/') &&
      path.endsWith('/ownership-transfer')
    ) {
      scenario.packageTransfers.push({ path, body });
      return jsonResponse({
        owner: {
          slug: body.target_org_slug,
        },
      });
    }

    if (method === 'GET' && path === '/v1/search') {
      scenario.searchCalls.push({
        org: url.searchParams.get('org') || '',
        repository: url.searchParams.get('repository') || '',
      });
      return jsonResponse({
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

    throw new Error(`Unhandled fetch request: ${method} ${url.toString()}`);
  }) as typeof fetch;
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

function parseJsonBody(body: BodyInit | null | undefined): JsonRecord {
  if (typeof body !== 'string' || body.length === 0) {
    return {};
  }

  return JSON.parse(body) as JsonRecord;
}

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: {
      'content-type': 'application/json',
    },
  });
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
