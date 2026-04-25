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

interface ApiRequestOptions {
  query?: Record<string, unknown>;
  body?: JsonRecord;
}

interface MutationCall {
  path: string;
  body?: JsonRecord;
}

interface Scenario {
  requests: string[];
  gotoCalls: string[];
  packageMutations: MutationCall[];
  packageArchiveCalls: string[];
  packageDetail: JsonRecord;
  releases: JsonRecord[];
  findings: JsonRecord[];
  organizations: JsonRecord[];
  teams: JsonRecord[];
  tags: Record<string, { version: string }>;
  tagMutations: string[];
  releaseMutations: string[];
}

const ECOSYSTEM = 'npm';
const PACKAGE_NAME = 'demo-widget';
const ORG_SLUG = 'acme';
const apiClientModuleUrl = new URL('../src/api/client.ts', import.meta.url)
  .href;
const pageStore = writable<TestPageState>(
  buildPageState(
    `https://example.test/packages/${ECOSYSTEM}/${PACKAGE_NAME}?tab=security`
  )
);
let currentScenario: Scenario | null = null;

function setPageHref(href: string): void {
  pageStore.set(buildPageState(href));
}

mock.module('$app/stores', () => ({
  page: {
    subscribe: pageStore.subscribe,
  },
}));

mock.module('$app/navigation', () => ({
  async goto(href: string): Promise<void> {
    if (!currentScenario) {
      return;
    }

    const nextUrl = new URL(href, 'https://example.test');
    currentScenario.gotoCalls.push(`${nextUrl.pathname}${nextUrl.search}`);
    setPageHref(nextUrl.toString());
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

const PackagePage =
  await import('../src/routes/packages/[ecosystem]/[name]/+page.svelte');

afterEach(() => {
  currentScenario = null;
  setPageHref(
    `https://example.test/packages/${ECOSYSTEM}/${PACKAGE_NAME}?tab=security`
  );
});

describe('package detail security access route', () => {
  test('surfaces delegated security review teams and filters large finding sets for triage', async () => {
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
      expect(delegatedAccessSection.textContent).toContain(
        'Can triage findings'
      );

      await waitFor(() => {
        expect(normalizeWhitespace(target.textContent)).toContain(
          'Showing 2 of 2 loaded findings, in the unresolved triage queue.'
        );
        expect(target.textContent).toContain('Prototype pollution');
        expect(target.textContent).toContain('Unsigned artifact');
        expect(target.textContent).not.toContain('Known malicious payload');
      });

      setChecked(
        queryRequiredCheckbox(target, '#package-security-include-resolved'),
        true
      );

      await waitFor(() => {
        expect(normalizeWhitespace(target.textContent)).toContain(
          'Showing 2 of 4 loaded findings, in the unresolved triage queue.'
        );
        expect(target.textContent).not.toContain('Known malicious payload');
      });

      changeValue(
        queryRequiredInput(target, '#package-security-search'),
        'prototype'
      );

      await waitFor(() => {
        expect(normalizeWhitespace(target.textContent)).toContain(
          'Showing 1 of 4 loaded findings, in the unresolved triage queue, matching "prototype".'
        );
        expect(target.textContent).toContain('Prototype pollution');
        expect(target.textContent).not.toContain('Unsigned artifact');
      });

      setChecked(
        queryRequiredCheckbox(target, 'input[type="checkbox"][value="high"]'),
        true
      );

      await waitFor(() => {
        expect(normalizeWhitespace(target.textContent)).toContain(
          'Showing 1 of 4 loaded findings, in the unresolved triage queue, filtered to High severity, matching "prototype".'
        );
      });

      expect(currentScenario.requests).toEqual([
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}`,
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases`,
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/tags`,
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/security-findings`,
        '/v1/users/me/organizations',
        `/v1/orgs/${ORG_SLUG}/teams`,
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/security-findings`,
      ]);
      expect(currentScenario.gotoCalls).toEqual([
        `/packages/${ECOSYSTEM}/${PACKAGE_NAME}?tab=security&security_include_resolved=true`,
        `/packages/${ECOSYSTEM}/${PACKAGE_NAME}?tab=security&security_include_resolved=true&security_search=prototype`,
        `/packages/${ECOSYSTEM}/${PACKAGE_NAME}?tab=security&security_include_resolved=true&security_search=prototype&security_severity=high`,
      ]);
    } finally {
      unmount();
    }
  });

  test('hydrates package security filters from the URL on initial load', async () => {
    currentScenario = createScenario();
    setPageHref(
      `https://example.test/packages/${ECOSYSTEM}/${PACKAGE_NAME}?tab=security&security_focus=resolved&security_include_resolved=true&security_search=pub-2026-0007&security_severity=critical`
    );

    const { target, unmount } = await renderSvelte(PackagePage.default);

    try {
      await waitFor(() => {
        expect(normalizeWhitespace(target.textContent)).toContain(
          'Showing 1 of 4 loaded findings, from resolved history, filtered to Critical severity, matching "pub-2026-0007".'
        );
        expect(target.textContent).toContain('Known malicious payload');
        expect(target.textContent).not.toContain('Prototype pollution');
      });

      expect(queryRequiredSelect(target, '#package-security-focus').value).toBe(
        'resolved'
      );
      expect(
        queryRequiredCheckbox(target, '#package-security-include-resolved')
          .checked
      ).toBe(true);
      expect(queryRequiredInput(target, '#package-security-search').value).toBe(
        'pub-2026-0007'
      );
      expect(
        queryRequiredCheckbox(
          target,
          'input[type="checkbox"][value="critical"]'
        ).checked
      ).toBe(true);

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

  test('loads and saves package metadata from the dedicated settings tab', async () => {
    currentScenario = createScenario({
      packageDetail: {
        can_manage_metadata: true,
        description: 'Original package summary.',
        readme: '# Demo Widget\n\nOriginal readme content.',
        homepage: 'https://example.test/demo-widget',
        repository_url: 'https://github.com/acme/demo-widget',
        license: 'MIT',
        keywords: ['widgets', 'cli'],
      },
    });
    setPageHref(
      `https://example.test/packages/${ECOSYSTEM}/${PACKAGE_NAME}?tab=settings`
    );

    const { target, unmount } = await renderSvelte(PackagePage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Package settings');
        expect(
          queryRequiredTextArea(target, '#package-settings-description').value
        ).toBe('Original package summary.');
        expect(
          queryRequiredInput(target, '#package-settings-homepage').value
        ).toBe('https://example.test/demo-widget');
        expect(
          queryRequiredInput(target, '#package-settings-keywords').value
        ).toBe('widgets, cli');
      });

      changeValue(
        queryRequiredTextArea(target, '#package-settings-description'),
        'Updated package summary for release automation.'
      );
      changeValue(
        queryRequiredInput(target, '#package-settings-homepage'),
        'https://example.test/demo-widget/v2'
      );
      changeValue(
        queryRequiredInput(target, '#package-settings-keywords'),
        'widgets, cli, release'
      );
      submitForm(queryRequiredForm(target, '#package-settings-form'));

      await waitFor(() => {
        expect(target.textContent).toContain('Package updated');
        expect(
          queryRequiredTextArea(target, '#package-settings-description').value
        ).toBe('Updated package summary for release automation.');
        expect(
          queryRequiredInput(target, '#package-settings-homepage').value
        ).toBe('https://example.test/demo-widget/v2');
        expect(
          queryRequiredInput(target, '#package-settings-keywords').value
        ).toBe('widgets, cli, release');
      });

      expect(currentScenario?.packageMutations).toEqual([
        {
          path: `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}`,
          body: {
            description: 'Updated package summary for release automation.',
            homepage: 'https://example.test/demo-widget/v2',
            keywords: ['widgets', 'cli', 'release'],
          },
        },
      ]);
      expect(currentScenario?.requests).toEqual([
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}`,
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases`,
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/tags`,
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/security-findings`,
        '/v1/users/me/organizations',
        `/v1/orgs/${ORG_SLUG}/teams`,
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

  test('archives a package from the settings tab after explicit confirmation', async () => {
    currentScenario = createScenario({
      packageDetail: {
        can_manage_metadata: true,
        is_archived: false,
      },
    });
    setPageHref(
      `https://example.test/packages/${ECOSYSTEM}/${PACKAGE_NAME}?tab=settings`
    );

    const { target, unmount } = await renderSvelte(PackagePage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Archive package');
        expect(
          queryRequiredCheckbox(target, '#package-archive-confirm').checked
        ).toBe(false);
      });

      submitForm(queryRequiredForm(target, '#package-archive-form'));

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Please confirm that you understand archiving marks this package as archived.'
        );
      });

      setChecked(
        queryRequiredCheckbox(target, '#package-archive-confirm'),
        true
      );
      submitForm(queryRequiredForm(target, '#package-archive-form'));

      await waitFor(() => {
        expect(target.textContent).toContain('Package archived');
        expect(target.textContent).toContain(
          'This package is already archived.'
        );
        expect(target.querySelector('#package-archive-form')).toBeNull();
      });

      expect(currentScenario?.packageArchiveCalls).toEqual([
        `DELETE /v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}`,
      ]);
      expect(currentScenario?.packageDetail.is_archived).toBe(true);
      expect(currentScenario?.requests).toEqual([
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}`,
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases`,
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/tags`,
        `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/security-findings`,
        '/v1/users/me/organizations',
        `/v1/orgs/${ORG_SLUG}/teams`,
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

  test('manages package tags from the package sidebar', async () => {
    currentScenario = createScenario({
      packageDetail: {
        can_manage_releases: true,
      },
      releases: [
        {
          version: '1.2.3',
          status: 'published',
        },
        {
          version: '1.3.0',
          status: 'published',
        },
      ],
      tags: {
        latest: { version: '1.2.3' },
      },
    });

    let confirmCalls = 0;
    const originalConfirm = window.confirm;
    window.confirm = (() => {
      confirmCalls += 1;
      return true;
    }) as typeof window.confirm;

    const { target, unmount } = await renderSvelte(PackagePage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Create or retarget a tag');
        expect(target.textContent).toContain('latest');
        expect(target.textContent).toContain('1.2.3');
      });

      changeValue(queryRequiredInput(target, '#package-tag-name'), 'beta');
      changeValue(queryRequiredSelect(target, '#package-tag-version'), '1.3.0');
      submitForm(queryRequiredForm(target, '#package-tag-form'));

      await waitFor(() => {
        expect(target.textContent).toContain('Tag updated');
        expect(target.textContent).toContain('beta');
        expect(target.textContent).toContain('1.3.0');
      });

      click(queryRequiredButton(target, '[data-tag-delete="latest"]'));

      await waitFor(() => {
        expect(target.textContent).toContain('Tag deleted');
        const tagsSection = querySectionByHeading(target, 'Tags');
        expect(tagsSection.textContent).not.toContain('latest');
        expect(tagsSection.textContent).toContain('beta');
      });

      expect(confirmCalls).toBe(1);
      expect(currentScenario?.tagMutations).toEqual([
        `PUT /v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/tags/beta`,
        `DELETE /v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/tags/latest`,
      ]);
    } finally {
      window.confirm = originalConfirm;
      unmount();
    }
  });

  test('removes deprecation from a release in the versions tab', async () => {
    currentScenario = createScenario({
      packageDetail: {
        can_manage_releases: true,
      },
      releases: [
        {
          version: '1.2.3',
          status: 'deprecated',
          is_deprecated: true,
          is_yanked: false,
          deprecation_message: 'Use 2.0.0 instead',
        },
      ],
    });
    setPageHref(
      `https://example.test/packages/${ECOSYSTEM}/${PACKAGE_NAME}?tab=versions`
    );

    const { target, unmount } = await renderSvelte(PackagePage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Remove deprecation');
        expect(target.textContent).toContain('deprecated');
      });

      click(queryRequiredButton(target, '[data-release-undeprecate="1.2.3"]'));

      await waitFor(() => {
        expect(target.textContent).toContain('Release undeprecated');
        expect(target.textContent).not.toContain('Remove deprecation');
      });

      expect(currentScenario?.releaseMutations).toEqual([
        `PUT /v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/1.2.3/undeprecate`,
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

  if (
    method === 'GET' &&
    path === `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}`
  ) {
    return apiResponse(currentScenario.packageDetail);
  }

  if (
    method === 'PATCH' &&
    path === `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}`
  ) {
    currentScenario.packageMutations.push({
      path,
      body: options?.body,
    });
    currentScenario.packageDetail = {
      ...currentScenario.packageDetail,
      ...(Object.prototype.hasOwnProperty.call(
        options?.body || {},
        'description'
      )
        ? { description: options?.body?.description ?? null }
        : {}),
      ...(Object.prototype.hasOwnProperty.call(options?.body || {}, 'readme')
        ? { readme: options?.body?.readme ?? null }
        : {}),
      ...(Object.prototype.hasOwnProperty.call(options?.body || {}, 'homepage')
        ? { homepage: options?.body?.homepage ?? null }
        : {}),
      ...(Object.prototype.hasOwnProperty.call(
        options?.body || {},
        'repository_url'
      )
        ? { repository_url: options?.body?.repository_url ?? null }
        : {}),
      ...(Object.prototype.hasOwnProperty.call(options?.body || {}, 'license')
        ? { license: options?.body?.license ?? null }
        : {}),
      ...(Object.prototype.hasOwnProperty.call(options?.body || {}, 'keywords')
        ? { keywords: options?.body?.keywords ?? null }
        : {}),
    };
    return apiResponse({ message: 'Package updated' });
  }

  if (
    method === 'DELETE' &&
    path === `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}`
  ) {
    currentScenario.packageArchiveCalls.push(`${method} ${path}`);
    currentScenario.packageDetail = {
      ...currentScenario.packageDetail,
      is_archived: true,
    };
    return apiResponse({ message: 'Package archived' });
  }

  if (
    method === 'GET' &&
    path === `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases`
  ) {
    expect(options?.query).toEqual({
      page: undefined,
      per_page: 20,
    });
    return apiResponse({ releases: currentScenario.releases });
  }

  if (
    method === 'GET' &&
    path === `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/tags`
  ) {
    return apiResponse({ tags: currentScenario.tags });
  }

  if (
    method === 'PUT' &&
    path.startsWith(`/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/`) &&
    path.endsWith('/undeprecate')
  ) {
    const version = decodeURIComponent(path.split('/').at(-2) || '');
    currentScenario.releaseMutations.push(`${method} ${path}`);
    currentScenario.releases = currentScenario.releases.map((release) =>
      release.version === version
        ? {
            ...release,
            status: 'published',
            is_deprecated: false,
            deprecation_message: null,
          }
        : release
    );
    return apiResponse({
      message: 'Release undeprecated',
      version,
      status: 'published',
    });
  }

  if (
    method === 'PUT' &&
    path.startsWith(`/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/tags/`)
  ) {
    const tag = decodeURIComponent(path.split('/').at(-1) || '');
    const version = String(options?.body?.version || '').trim();
    if (!tag || !version) {
      throw new Error(`Invalid tag mutation body for ${path}.`);
    }

    currentScenario.tagMutations.push(`${method} ${path}`);
    currentScenario.tags[tag] = { version };
    return apiResponse({ message: 'Tag updated', tag, version });
  }

  if (
    method === 'DELETE' &&
    path.startsWith(`/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/tags/`)
  ) {
    const tag = decodeURIComponent(path.split('/').at(-1) || '');
    currentScenario.tagMutations.push(`${method} ${path}`);

    if (!(tag in currentScenario.tags)) {
      throw new Error(`Missing tag ${tag}.`);
    }

    delete currentScenario.tags[tag];
    return apiResponse({ message: 'Tag deleted', tag });
  }

  if (
    method === 'GET' &&
    path === `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/security-findings`
  ) {
    const includeResolved = options?.query?.include_resolved === true;
    return apiResponse({
      findings: includeResolved
        ? currentScenario.findings
        : currentScenario.findings.filter(
            (finding) => finding.is_resolved !== true
          ),
    });
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

function createScenario(overrides: Partial<Scenario> = {}): Scenario {
  return {
    requests: [],
    gotoCalls: [],
    packageMutations: [],
    packageArchiveCalls: [],
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
      ...(overrides.packageDetail || {}),
    },
    releases: overrides.releases || [],
    findings: overrides.findings || [
      {
        id: 'finding-1',
        kind: 'vulnerability',
        severity: 'high',
        title: 'Prototype pollution',
        description:
          'User-controlled merge input can pollute object prototypes.',
        advisory_id: 'CVE-2026-0001',
        is_resolved: false,
        detected_at: '2026-04-13T00:00:00Z',
        release_version: '1.2.3',
        artifact_filename: 'demo-widget-1.2.3.tgz',
      },
      {
        id: 'finding-3',
        kind: 'policy_violation',
        severity: 'low',
        title: 'Unsigned artifact',
        description: 'Artifact was published without a detached signature.',
        is_resolved: false,
        detected_at: '2026-04-12T00:00:00Z',
        release_version: '1.2.2',
        artifact_filename: 'demo-widget-1.2.2.tgz',
      },
      {
        id: 'finding-2',
        kind: 'malware',
        severity: 'critical',
        title: 'Known malicious payload',
        description: 'Scanner detected a malicious embedded payload.',
        advisory_id: 'PUB-2026-0007',
        is_resolved: true,
        resolved_at: '2026-04-15T00:00:00Z',
        detected_at: '2026-04-14T00:00:00Z',
        release_version: '1.2.4',
        artifact_filename: 'demo-widget-1.2.4.tgz',
      },
      {
        id: 'finding-4',
        kind: 'license_issue',
        severity: 'info',
        title: 'Legacy advisory',
        description: 'Legacy advisory retained for audit visibility.',
        advisory_id: 'ADV-2026-0009',
        is_resolved: true,
        resolved_at: '2026-04-16T00:00:00Z',
        detected_at: '2026-04-10T00:00:00Z',
        release_version: '1.2.1',
        artifact_filename: 'demo-widget-1.2.1.tgz',
      },
    ],
    organizations: overrides.organizations || [
      {
        id: 'org-1',
        slug: ORG_SLUG,
        name: 'Acme',
        role: 'admin',
      },
    ],
    teams: overrides.teams || [
      {
        id: 'team-security',
        slug: 'security-team',
        name: 'Security Team',
        description: 'Owns security review workflows.',
        created_at: '2026-04-01T00:00:00Z',
      },
    ],
    tags: overrides.tags || {},
    tagMutations: [],
    releaseMutations: [],
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

function querySectionByHeading(
  target: HTMLElement,
  headingText: string
): HTMLElement {
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

function queryRequiredInput(
  target: HTMLElement,
  selector: string
): HTMLInputElement {
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

function queryRequiredSelect(
  target: HTMLElement,
  selector: string
): HTMLSelectElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLSelectElement)) {
    throw new Error(`Missing select for selector ${selector}.`);
  }
  return element;
}

function queryRequiredButton(
  target: ParentNode,
  selector: string
): HTMLButtonElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLButtonElement)) {
    throw new Error(`Missing button for selector ${selector}.`);
  }
  return element;
}

function queryRequiredForm(
  target: ParentNode,
  selector: string
): HTMLFormElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLFormElement)) {
    throw new Error(`Missing form for selector ${selector}.`);
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
