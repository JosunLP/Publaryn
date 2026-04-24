/// <reference path="./bun-test.d.ts" />

import { afterEach, describe, expect, mock, test } from 'bun:test';
import { writable } from 'svelte/store';

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

interface ApiRequestOptions {
  query?: Record<string, unknown>;
  body?: JsonRecord;
}

interface Scenario {
  requests: string[];
  transferCalls: Array<{ slug: string; targetOrgSlug: string }>;
  repositoryDetail: JsonRecord;
  packages: JsonRecord[];
  organizations: JsonRecord[];
}

const REPOSITORY_SLUG = 'source-packages';
const apiClientModuleUrl = new URL('../src/api/client.ts', import.meta.url).href;
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
      post: (path: string, options?: ApiRequestOptions) =>
        handleApiRequest('POST', path, options),
      patch: (path: string, options?: ApiRequestOptions) =>
        handleApiRequest('PATCH', path, options),
    },
  };
});

const RepositoryPage = await import(
  '../src/routes/repositories/[slug]/+page.svelte'
);

afterEach(() => {
  currentScenario = null;
  pageStore.set(buildPageState(`https://example.test/repositories/${REPOSITORY_SLUG}`));
});

describe('repository detail route', () => {
  test('renders repository transfer controls and transfers ownership', async () => {
    currentScenario = createScenario();

    const { target, unmount, flush } = await renderSvelte(RepositoryPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Source Packages');
        expect(target.textContent).toContain('Transfer ownership');
      });

      changeValue(
        queryRequiredSelect(target, '#repository-transfer-target'),
        'target-org'
      );
      setChecked(queryRequiredCheckbox(target, '#repository-transfer-confirm'), true);
      flush();
      submitForm(queryRequiredForm(target, '#repository-transfer-form'));

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Repository ownership transferred to Target Org.'
        );
        expect(target.textContent).toContain('Target Org (@target-org)');
      });

      expect(currentScenario.requests).toEqual([
        `/v1/repositories/${REPOSITORY_SLUG}`,
        '/v1/users/me/organizations',
        `/v1/repositories/${REPOSITORY_SLUG}/packages`,
        `/v1/repositories/${REPOSITORY_SLUG}`,
        '/v1/users/me/organizations',
        `/v1/repositories/${REPOSITORY_SLUG}/packages`,
      ]);
      expect(currentScenario.transferCalls).toEqual([
        {
          slug: REPOSITORY_SLUG,
          targetOrgSlug: 'target-org',
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('skips transfer-target loading when repository transfer is unavailable', async () => {
    currentScenario = createScenario({
      repositoryDetail: {
        can_transfer: false,
      },
    });

    const { target, unmount } = await renderSvelte(RepositoryPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Repository settings');
      });

      expect(target.textContent).not.toContain('Transfer ownership');
      expect(currentScenario.requests).toEqual([
        `/v1/repositories/${REPOSITORY_SLUG}`,
        `/v1/repositories/${REPOSITORY_SLUG}/packages`,
      ]);
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
    currentScenario.requests.push(path);
  }

  if (method === 'GET' && path === `/v1/repositories/${REPOSITORY_SLUG}`) {
    return apiResponse(currentScenario.repositoryDetail);
  }

  if (method === 'GET' && path === `/v1/repositories/${REPOSITORY_SLUG}/packages`) {
    expect(options?.query).toEqual({
      page: undefined,
      per_page: 100,
    });
    return apiResponse({ packages: currentScenario.packages });
  }

  if (method === 'GET' && path === '/v1/users/me/organizations') {
    return apiResponse({ organizations: currentScenario.organizations });
  }

  if (
    method === 'POST' &&
    path === `/v1/repositories/${REPOSITORY_SLUG}/ownership-transfer`
  ) {
    const targetOrgSlug = String(options?.body?.target_org_slug || '').trim();
    currentScenario.transferCalls.push({
      slug: REPOSITORY_SLUG,
      targetOrgSlug,
    });
    currentScenario.repositoryDetail = {
      ...currentScenario.repositoryDetail,
      owner_org_slug: targetOrgSlug,
      owner_org_name: 'Target Org',
    };
    return apiResponse({
      repository: {
        slug: REPOSITORY_SLUG,
        name: 'Source Packages',
      },
      owner: {
        slug: targetOrgSlug,
        name: 'Target Org',
      },
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

function createScenario(overrides: Partial<Scenario> = {}): Scenario {
  return {
    requests: [],
    transferCalls: [],
    repositoryDetail: {
      id: 'repo-1',
      slug: REPOSITORY_SLUG,
      name: 'Source Packages',
      description: 'Organization-owned release repository.',
      kind: 'release',
      visibility: 'private',
      owner_org_slug: 'source-org',
      owner_org_name: 'Source Org',
      can_manage: true,
      can_create_packages: true,
      can_transfer: true,
      created_at: '2026-04-20T00:00:00Z',
      updated_at: '2026-04-21T00:00:00Z',
      ...(overrides.repositoryDetail || {}),
    },
    packages: overrides.packages || [
      {
        id: 'pkg-1',
        ecosystem: 'npm',
        name: '@source/demo-widget',
        visibility: 'private',
        download_count: 42,
      },
    ],
    organizations: overrides.organizations || [
      {
        id: 'org-1',
        slug: 'source-org',
        name: 'Source Org',
        role: 'admin',
      },
      {
        id: 'org-2',
        slug: 'target-org',
        name: 'Target Org',
        role: 'owner',
      },
    ],
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

function queryRequiredSelect(target: HTMLElement, selector: string): HTMLSelectElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLSelectElement)) {
    throw new Error(`Missing select for selector ${selector}.`);
  }
  return element;
}

function queryRequiredForm(target: ParentNode, selector: string): HTMLFormElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLFormElement)) {
    throw new Error(`Missing form for selector ${selector}.`);
  }
  return element;
}

function queryRequiredCheckbox(target: ParentNode, selector: string): HTMLInputElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLInputElement)) {
    throw new Error(`Missing checkbox for selector ${selector}.`);
  }
  return element;
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
