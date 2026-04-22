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

interface ApiRequestOptions {
  query?: Record<string, unknown>;
  body?: JsonRecord;
}

interface Scenario {
  requests: string[];
  packageDetail: JsonRecord;
  findings: JsonRecord[];
  organizations: JsonRecord[];
  teams: JsonRecord[];
}

const ECOSYSTEM = 'npm';
const PACKAGE_NAME = 'demo-widget';
const ORG_SLUG = 'acme';
const apiClientModuleUrl = new URL('../src/api/client.ts', import.meta.url).href;
const pageStore = writable<TestPageState>(
  buildPageState(
    `https://example.test/packages/${ECOSYSTEM}/${PACKAGE_NAME}?tab=security`
  )
);
let currentScenario: Scenario | null = null;

mock.module('$app/stores', () => ({
  page: {
    subscribe: pageStore.subscribe,
  },
}));

mock.module('$app/navigation', () => ({
  async goto(): Promise<void> {},
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

const PackagePage = await import('../src/routes/packages/[ecosystem]/[name]/+page.svelte');

afterEach(() => {
  currentScenario = null;
  pageStore.set(
    buildPageState(
      `https://example.test/packages/${ECOSYSTEM}/${PACKAGE_NAME}?tab=security`
    )
  );
});

describe('package detail security access route', () => {
  test('surfaces delegated security review teams and current triage access', async () => {
    currentScenario = createScenario();

    const { target, unmount } = await renderSvelte(PackagePage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Security review access');
        expect(normalizeWhitespace(target.textContent)).toContain(
          'Teams with Security review or Admin package grants can resolve and reopen findings for this package.'
        );
        expect(target.textContent).toContain('You can triage findings');
        expect(target.textContent).toContain(
          'Review and triage package security findings.'
        );
      });

      const securityAccessSection = querySectionByHeading(
        target,
        'Security review access'
      );
      expect(securityAccessSection.textContent).toContain('Security Team');
      expect(securityAccessSection.textContent).toContain('Owners Team');
      expect(securityAccessSection.textContent).not.toContain('Readers Team');
      expect(securityAccessSection.textContent).toContain('Security Review');
      expect(securityAccessSection.textContent).toContain('Admin');

      const delegatedAccessSection = querySectionByHeading(
        target,
        'Delegated team access'
      );
      expect(delegatedAccessSection.textContent).toContain('Readers Team');
      expect(delegatedAccessSection.textContent).toContain('Can triage findings');

      expect(currentScenario.requests).toEqual([
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}`,
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases`,
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/tags`,
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/security-findings`,
        '/v1/users/me/organizations',
        `/v1/orgs/${ORG_SLUG}/teams`,
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

  if (method === 'GET' && path === `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}`) {
    return apiResponse(currentScenario.packageDetail);
  }

  if (method === 'GET' && path === `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases`) {
    expect(options?.query).toEqual({
      page: undefined,
      per_page: 20,
    });
    return apiResponse({ releases: [] });
  }

  if (method === 'GET' && path === `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/tags`) {
    return apiResponse({ tags: {} });
  }

  if (
    method === 'GET' &&
    path === `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/security-findings`
  ) {
    expect(options?.query).toEqual({
      include_resolved: undefined,
    });
    return apiResponse({ findings: currentScenario.findings });
  }

  if (method === 'GET' && path === '/v1/users/me/organizations') {
    return apiResponse({ organizations: currentScenario.organizations });
  }

  if (method === 'GET' && path === `/v1/orgs/${ORG_SLUG}/teams`) {
    return apiResponse({ teams: currentScenario.teams });
  }

  throw new Error(`Unhandled request: ${method} ${path}`);
}

function apiResponse(data: JsonRecord): { data: JsonRecord; requestId: null } {
  return {
    data,
    requestId: null,
  };
}

function createScenario(): Scenario {
  return {
    requests: [],
    packageDetail: {
      ecosystem: ECOSYSTEM,
      name: PACKAGE_NAME,
      display_name: 'Demo Widget',
      description: 'Package used for security delegation route coverage.',
      owner_org_slug: ORG_SLUG,
      latest_version: '1.2.3',
      can_manage_security: true,
      can_manage_metadata: false,
      can_manage_releases: false,
      can_manage_trusted_publishers: false,
      can_transfer: false,
      team_access: [
        {
          team_id: 'team-security',
          team_slug: 'security-team',
          team_name: 'Security Team',
          permissions: ['security_review'],
          granted_at: '2026-04-10T00:00:00Z',
        },
        {
          team_id: 'team-owners',
          team_slug: 'owners-team',
          team_name: 'Owners Team',
          permissions: ['admin', 'publish'],
          granted_at: '2026-04-11T00:00:00Z',
        },
        {
          team_id: 'team-readers',
          team_slug: 'readers-team',
          team_name: 'Readers Team',
          permissions: ['read_private'],
          granted_at: '2026-04-12T00:00:00Z',
        },
      ],
    },
    findings: [
      {
        id: 'finding-1',
        kind: 'vulnerability',
        severity: 'high',
        title: 'Prototype pollution',
        description: 'User-controlled merge input can pollute object prototypes.',
        advisory_id: 'CVE-2026-0001',
        is_resolved: false,
        detected_at: '2026-04-13T00:00:00Z',
        release_version: '1.2.3',
        artifact_filename: 'demo-widget-1.2.3.tgz',
      },
    ],
    organizations: [
      {
        id: 'org-1',
        slug: ORG_SLUG,
        name: 'Acme',
        role: 'admin',
      },
    ],
    teams: [
      {
        id: 'team-security',
        slug: 'security-team',
        name: 'Security Team',
        description: 'Owns security review workflows.',
        created_at: '2026-04-01T00:00:00Z',
      },
    ],
  };
}

function buildPageState(href: string): TestPageState {
  const url = new URL(href);
  return {
    url,
    params: {
      ecosystem: ECOSYSTEM,
      name: PACKAGE_NAME,
    },
    route: { id: '/packages/[ecosystem]/[name]' },
    status: 200,
    error: null,
    data: {},
    form: null,
  };
}

function querySectionByHeading(target: HTMLElement, headingText: string): HTMLElement {
  const heading = Array.from(target.querySelectorAll('h2, h3, h4')).find(
    (element) => element.textContent?.trim() === headingText
  );

  if (!(heading instanceof HTMLElement)) {
    throw new Error(`Missing heading '${headingText}'.`);
  }

  const container =
    heading.closest('.surface-card') ||
    heading.closest('.sidebar-section') ||
    heading.closest('.settings-subsection') ||
    heading.parentElement;

  if (!(container instanceof HTMLElement)) {
    throw new Error(`Missing section container for heading '${headingText}'.`);
  }

  return container;
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

function normalizeWhitespace(value: string | null | undefined): string {
  return (value || '').replace(/\s+/g, ' ').trim();
}
