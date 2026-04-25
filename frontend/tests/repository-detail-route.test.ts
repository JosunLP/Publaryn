/// <reference path="./bun-test.d.ts" />

import { afterEach, describe, expect, mock, test } from 'bun:test';
import { writable } from 'svelte/store';

import { changeValue, renderSvelte, submitForm } from './svelte-dom';

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

interface ApiRequestOptions {
  query?: Record<string, unknown>;
  body?: JsonRecord;
}

interface RequestCall {
  method: string;
  path: string;
  query?: Record<string, unknown>;
}

interface MutationCall {
  path: string;
  body?: JsonRecord;
}

interface RepositoryRecord extends JsonRecord {
  id: string;
  name: string;
  slug: string;
  description: string | null;
  kind: string;
  visibility: string;
  owner_user_id: string | null;
  owner_org_id: string | null;
  owner_username: string | null;
  owner_org_slug: string | null;
  owner_org_name: string | null;
  can_manage: boolean;
  can_create_packages: boolean;
  can_transfer: boolean;
  created_at: string;
  updated_at: string;
  upstream_url: string | null;
}

interface PackageRecord extends JsonRecord {
  id: string;
  name: string;
  ecosystem: string;
  description: string | null;
  visibility: string;
  download_count: number | null;
  created_at: string;
}

interface Scenario {
  requests: RequestCall[];
  patchCalls: MutationCall[];
  postCalls: MutationCall[];
  repository: RepositoryRecord;
  packages: PackageRecord[];
  packageLoadError: string | null;
}

const ORG_ID = '11111111-1111-4111-8111-111111111111';
const ORG_SLUG = 'source-org';
const REPOSITORY_SLUG = 'source-packages';
const apiClientModuleUrl = new URL('../src/api/client.ts', import.meta.url)
  .href;
const pageStore = writable<TestPageState>(
  buildPageState(`https://example.test/repositories/${REPOSITORY_SLUG}`)
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
      patch: (path: string, options?: ApiRequestOptions) =>
        handleApiRequest('PATCH', path, options),
      post: (path: string, options?: ApiRequestOptions) =>
        handleApiRequest('POST', path, options),
    },
  };
});

const RepositoryPage =
  await import('../src/routes/repositories/[slug]/+page.svelte');

afterEach(() => {
  currentScenario = null;
  pageStore.set(
    buildPageState(`https://example.test/repositories/${REPOSITORY_SLUG}`)
  );
});

describe('repository detail route', () => {
  test('renders org-owned repository details and package creation controls', async () => {
    currentScenario = createScenario();

    const { target, unmount } = await renderSvelte(RepositoryPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Source Packages');
        expect(target.textContent).toContain('Visible packages');
        expect(target.textContent).toContain('Repository settings');
        expect(target.textContent).toContain('Create a package');
        expect(target.textContent).toContain('release-widget');
        expect(
          target.querySelector(`a[href="/orgs/${ORG_SLUG}"]`)
        ).not.toBeNull();
        expect(
          target.querySelector(
            'a[href="/packages/npm/release-widget?tab=security"]'
          )
        ).not.toBeNull();
        expect(
          target.querySelector('a[href="/packages/npm/release-widget"]')
        ).not.toBeNull();
        expect(
          queryRequiredSelect(target, '#repository-visibility').value
        ).toBe('public');
        expect(
          optionValues(
            queryRequiredSelect(target, '#package-create-visibility')
          )
        ).toEqual(['', 'private', 'internal_org', 'unlisted', 'quarantined']);
      });

      expect(currentScenario.requests).toEqual([
        {
          method: 'GET',
          path: `/v1/repositories/${REPOSITORY_SLUG}`,
        },
        {
          method: 'GET',
          path: '/v1/users/me/organizations',
        },
        {
          method: 'GET',
          path: `/v1/repositories/${REPOSITORY_SLUG}/packages`,
          query: {
            page: undefined,
            per_page: 100,
          },
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('saves repository settings and reloads updated repository state', async () => {
    currentScenario = createScenario();

    const { target, unmount } = await renderSvelte(RepositoryPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Repository settings');
      });

      changeValue(
        queryRequiredTextArea(target, '#repository-description'),
        'Private release packages for internal teams.'
      );
      submitForm(
        queryClosestForm(queryRequiredSelect(target, '#repository-visibility'))
      );

      await waitFor(() => {
        expect(target.textContent).toContain('Repository updated.');
        expect(currentScenario?.repository.visibility).toBe('public');
        expect(currentScenario?.repository.description).toBe(
          'Private release packages for internal teams.'
        );
      });

      expect(currentScenario.patchCalls).toEqual([
        {
          path: `/v1/repositories/${REPOSITORY_SLUG}`,
          body: {
            description: 'Private release packages for internal teams.',
            visibility: 'public',
            upstream_url: undefined,
          },
        },
      ]);
      expect(currentScenario.requests).toEqual([
        {
          method: 'GET',
          path: `/v1/repositories/${REPOSITORY_SLUG}`,
        },
        {
          method: 'GET',
          path: '/v1/users/me/organizations',
        },
        {
          method: 'GET',
          path: `/v1/repositories/${REPOSITORY_SLUG}/packages`,
          query: {
            page: undefined,
            per_page: 100,
          },
        },
        {
          method: 'GET',
          path: `/v1/repositories/${REPOSITORY_SLUG}`,
        },
        {
          method: 'GET',
          path: '/v1/users/me/organizations',
        },
        {
          method: 'GET',
          path: `/v1/repositories/${REPOSITORY_SLUG}/packages`,
          query: {
            page: undefined,
            per_page: 100,
          },
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('creates a package from the repository workspace and reloads visible packages', async () => {
    currentScenario = createScenario();

    const { target, unmount } = await renderSvelte(RepositoryPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Create a package');
      });

      changeValue(
        queryRequiredSelect(target, '#package-create-ecosystem'),
        'npm'
      );
      changeValue(
        queryRequiredInput(target, '#package-create-name'),
        'internal-widget'
      );
      changeValue(
        queryRequiredInput(target, '#package-create-display-name'),
        'Internal Widget'
      );
      changeValue(
        queryRequiredTextArea(target, '#package-create-description'),
        'Internal widget package.'
      );
      submitForm(
        queryClosestForm(queryRequiredInput(target, '#package-create-name'))
      );

      await waitFor(() => {
        expect(normalizeWhitespace(target.textContent)).toContain(
          'Created npm package internal-widget in Source Packages.'
        );
        expect(target.textContent).toContain('internal-widget');
        expect(
          target.querySelector(
            'a[href="/packages/npm/internal-widget?tab=security"]'
          )
        ).not.toBeNull();
        expect(
          target.querySelector('a[href="/packages/npm/internal-widget"]')
        ).not.toBeNull();
        expect(queryRequiredInput(target, '#package-create-name').value).toBe(
          ''
        );
        expect(
          queryRequiredInput(target, '#package-create-display-name').value
        ).toBe('');
        expect(
          queryRequiredTextArea(target, '#package-create-description').value
        ).toBe('');
        expect(
          queryRequiredSelect(target, '#package-create-visibility').value
        ).toBe('');
      });

      expect(currentScenario.postCalls).toEqual([
        {
          path: '/v1/packages',
          body: {
            ecosystem: 'npm',
            name: 'internal-widget',
            repository_slug: REPOSITORY_SLUG,
            visibility: undefined,
            display_name: 'Internal Widget',
            description: 'Internal widget package.',
          },
        },
      ]);
      expect(currentScenario.requests).toEqual([
        {
          method: 'GET',
          path: `/v1/repositories/${REPOSITORY_SLUG}`,
        },
        {
          method: 'GET',
          path: '/v1/users/me/organizations',
        },
        {
          method: 'GET',
          path: `/v1/repositories/${REPOSITORY_SLUG}/packages`,
          query: {
            page: undefined,
            per_page: 100,
          },
        },
        {
          method: 'GET',
          path: `/v1/repositories/${REPOSITORY_SLUG}`,
        },
        {
          method: 'GET',
          path: '/v1/users/me/organizations',
        },
        {
          method: 'GET',
          path: `/v1/repositories/${REPOSITORY_SLUG}/packages`,
          query: {
            page: undefined,
            per_page: 100,
          },
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('shows the packages:write warning when repository package creation is unavailable', async () => {
    currentScenario = createScenario({
      repository: {
        can_create_packages: false,
      },
    });

    const { target, unmount } = await renderSvelte(RepositoryPage.default);

    try {
      await waitFor(() => {
        expect(normalizeWhitespace(target.textContent)).toContain(
          'Your current credential can manage this repository but cannot create packages because it does not include the packages:write scope.'
        );
      });

      expect(target.querySelector('#package-create-name')).toBeNull();
      expect(target.querySelector('#package-create-visibility')).toBeNull();
    } finally {
      unmount();
    }
  });
});

async function handleApiRequest(
  method: string,
  path: string,
  options?: ApiRequestOptions
): Promise<{ data: JsonRecord; requestId: null }> {
  if (!currentScenario) {
    throw new Error('Missing test scenario.');
  }

  if (method === 'GET') {
    const request: RequestCall = {
      method,
      path,
    };

    if (options?.query !== undefined) {
      request.query = options.query;
    }

    currentScenario.requests.push(request);
  }

  if (method === 'GET' && path === `/v1/repositories/${REPOSITORY_SLUG}`) {
    return apiResponse(currentScenario.repository);
  }

  if (
    method === 'GET' &&
    path === `/v1/repositories/${REPOSITORY_SLUG}/packages`
  ) {
    return apiResponse({
      packages: currentScenario.packages,
      load_error: currentScenario.packageLoadError,
    });
  }

  if (method === 'GET' && path === '/v1/users/me/organizations') {
    return apiResponse({
      organizations: [],
    });
  }

  if (method === 'PATCH' && path === `/v1/repositories/${REPOSITORY_SLUG}`) {
    currentScenario.patchCalls.push({
      path,
      body: options?.body,
    });

    currentScenario.repository = {
      ...currentScenario.repository,
      description:
        typeof options?.body?.description === 'string' ||
        options?.body?.description === null
          ? (options?.body?.description as string | null)
          : currentScenario.repository.description,
      visibility:
        typeof options?.body?.visibility === 'string'
          ? String(options.body.visibility)
          : currentScenario.repository.visibility,
      updated_at: '2026-04-05T00:00:00Z',
    };

    return apiResponse({
      message: 'Repository updated.',
    });
  }

  if (method === 'POST' && path === '/v1/packages') {
    currentScenario.postCalls.push({
      path,
      body: options?.body,
    });

    const ecosystem = String(options?.body?.ecosystem || '').trim();
    const name = String(options?.body?.name || '').trim();
    const description =
      typeof options?.body?.description === 'string'
        ? String(options.body.description)
        : null;
    const visibility =
      typeof options?.body?.visibility === 'string'
        ? String(options.body.visibility)
        : currentScenario.repository.visibility;

    currentScenario.packages = [
      ...currentScenario.packages,
      {
        id: `pkg-${name}`,
        ecosystem,
        name,
        description,
        visibility,
        download_count: null,
        created_at: '2026-04-05T00:00:00Z',
      },
    ];

    return apiResponse({
      id: `pkg-${name}`,
      ecosystem,
      name,
      repository_slug: REPOSITORY_SLUG,
      visibility,
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

function createScenario(
  overrides: {
    repository?: Partial<RepositoryRecord>;
    packages?: PackageRecord[];
    packageLoadError?: string | null;
  } = {}
): Scenario {
  return {
    requests: [],
    patchCalls: [],
    postCalls: [],
    repository: {
      id: 'repo-1',
      name: 'Source Packages',
      slug: REPOSITORY_SLUG,
      description: 'Public package repository for the source organization.',
      kind: 'public',
      visibility: 'public',
      owner_user_id: null,
      owner_org_id: ORG_ID,
      owner_username: null,
      owner_org_slug: ORG_SLUG,
      owner_org_name: 'Source Org',
      can_manage: true,
      can_create_packages: true,
      can_transfer: true,
      created_at: '2026-04-01T00:00:00Z',
      updated_at: '2026-04-02T00:00:00Z',
      upstream_url: null,
      ...(overrides.repository || {}),
    },
    packages: overrides.packages || [
      {
        id: 'pkg-1',
        ecosystem: 'npm',
        name: 'release-widget',
        description: 'Primary package published from this repository.',
        visibility: 'public',
        download_count: 42,
        created_at: '2026-04-03T00:00:00Z',
      },
    ],
    packageLoadError: overrides.packageLoadError ?? null,
  };
}

function buildPageState(href: string): TestPageState {
  const url = new URL(href);
  return {
    url,
    params: {
      slug: REPOSITORY_SLUG,
    },
    route: { id: '/repositories/[slug]' },
    status: 200,
    error: null,
    data: {},
    form: null,
  };
}

function queryRequiredInput(
  target: ParentNode,
  selector: string
): HTMLInputElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLInputElement)) {
    throw new Error(`Missing input for selector ${selector}.`);
  }
  return element;
}

function queryRequiredSelect(
  target: ParentNode,
  selector: string
): HTMLSelectElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLSelectElement)) {
    throw new Error(`Missing select for selector ${selector}.`);
  }
  return element;
}

function queryRequiredTextArea(
  target: ParentNode,
  selector: string
): HTMLTextAreaElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLTextAreaElement)) {
    throw new Error(`Missing textarea for selector ${selector}.`);
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

function optionValues(select: HTMLSelectElement): string[] {
  return Array.from(select.options).map((option) => option.value);
}

async function waitFor(
  assertion: () => void,
  {
    timeout = 1000,
    interval = 10,
  }: { timeout?: number; interval?: number } = {}
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

  throw lastError instanceof Error
    ? lastError
    : new Error('Timed out waiting for assertion.');
}

function normalizeWhitespace(value: string | null | undefined): string {
  return (value || '').replace(/\s+/g, ' ').trim();
}
