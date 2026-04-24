/// <reference path="./bun-test.d.ts" />

import { afterEach, describe, expect, mock, test } from 'bun:test';
import { writable } from 'svelte/store';

import { click, renderSvelte } from './svelte-dom';

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
  release: JsonRecord;
  artifacts: JsonRecord[];
  mutations: string[];
}

const ECOSYSTEM = 'npm';
const PACKAGE_NAME = 'demo-widget';
const VERSION = '1.2.3';
const apiClientModuleUrl = new URL('../src/api/client.ts', import.meta.url).href;
const pageStore = writable<TestPageState>(
  buildPageState(
    `https://example.test/packages/${ECOSYSTEM}/${PACKAGE_NAME}/versions/${VERSION}`
  )
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
      put: (path: string, options?: ApiRequestOptions) =>
        handleApiRequest('PUT', path, options),
      post: (path: string, options?: ApiRequestOptions) =>
        handleApiRequest('POST', path, options),
    },
  };
});

const VersionPage = await import(
  '../src/routes/packages/[ecosystem]/[name]/versions/[version]/+page.svelte'
);

afterEach(() => {
  currentScenario = null;
  pageStore.set(
    buildPageState(
      `https://example.test/packages/${ECOSYSTEM}/${PACKAGE_NAME}/versions/${VERSION}`
    )
  );
});

describe('package version route', () => {
  test('removes deprecation from a deprecated release', async () => {
    currentScenario = {
      release: {
        ecosystem: ECOSYSTEM,
        name: PACKAGE_NAME,
        version: VERSION,
        status: 'deprecated',
        is_deprecated: true,
        is_yanked: false,
        deprecation_message: 'Use 2.0.0 instead',
        can_manage_releases: true,
        created_at: '2026-04-01T00:00:00Z',
        published_at: '2026-04-02T00:00:00Z',
      },
      artifacts: [],
      mutations: [],
    };

    const { target, unmount } = await renderSvelte(VersionPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Remove deprecation');
        expect(target.textContent).toContain('deprecated');
      });

      click(queryRequiredButton(target, '#release-undeprecate'));

      await waitFor(() => {
        expect(target.textContent).toContain('Release undeprecated');
        expect(target.textContent).not.toContain('Remove deprecation');
      });

      expect(currentScenario.mutations).toEqual([
        `PUT /v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/undeprecate`,
      ]);
    } finally {
      unmount();
    }
  });
});

async function handleApiRequest(
  method: string,
  path: string,
  _options?: ApiRequestOptions
): Promise<{ data: JsonRecord; requestId: null }> {
  if (!currentScenario) {
    throw new Error('Missing test scenario.');
  }

  if (
    method === 'GET' &&
    path === `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}`
  ) {
    return apiResponse(currentScenario.release);
  }

  if (
    method === 'GET' &&
    path ===
      `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/artifacts`
  ) {
    return apiResponse({ artifacts: currentScenario.artifacts });
  }

  if (
    method === 'PUT' &&
    path ===
      `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/undeprecate`
  ) {
    currentScenario.mutations.push(`${method} ${path}`);
    currentScenario.release = {
      ...currentScenario.release,
      status: 'published',
      is_deprecated: false,
      deprecation_message: null,
    };
    return apiResponse({
      message: 'Release undeprecated',
      version: VERSION,
      status: 'published',
    });
  }

  throw new Error(`Unhandled request: ${method} ${path}`);
}

function apiResponse(data: JsonRecord): { data: JsonRecord; requestId: null } {
  return { data, requestId: null };
}

function buildPageState(href: string): TestPageState {
  const url = new URL(href);
  return {
    url,
    params: {
      ecosystem: ECOSYSTEM,
      name: PACKAGE_NAME,
      version: VERSION,
    },
    route: { id: '/packages/[ecosystem]/[name]/versions/[version]' },
    status: 200,
    error: null,
    data: {},
    form: null,
  };
}

function queryRequiredButton(target: ParentNode, selector: string): HTMLButtonElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLButtonElement)) {
    throw new Error(`Missing button for selector ${selector}.`);
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
