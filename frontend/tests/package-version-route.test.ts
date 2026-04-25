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
  body?: unknown;
  headers?: Record<string, string>;
}

interface MutationCall {
  method: string;
  path: string;
  query?: Record<string, unknown>;
  body?: unknown;
  headers?: Record<string, string>;
}

interface ScenarioErrors {
  publish?: string;
  uploadArtifact?: string;
  yank?: string;
  restore?: string;
  deprecate?: string;
  undeprecate?: string;
}

interface Scenario {
  release: JsonRecord;
  artifacts: JsonRecord[];
  mutations: string[];
  mutationCalls: MutationCall[];
  errors: ScenarioErrors;
}

class MockApiError<TBody = unknown> extends Error {
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

const ECOSYSTEM = 'npm';
const PACKAGE_NAME = 'demo-widget';
const VERSION = '1.2.3';
const ARTIFACT_FILENAME = `${PACKAGE_NAME}-${VERSION}.tgz`;
const apiClientModuleUrl = new URL('../src/api/client.ts', import.meta.url)
  .href;
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

mock.module(apiClientModuleUrl, () => ({
  ApiError: MockApiError,
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
}));

const VersionPage =
  await import('../src/routes/packages/[ecosystem]/[name]/versions/[version]/+page.svelte');

afterEach(() => {
  currentScenario = null;
  pageStore.set(
    buildPageState(
      `https://example.test/packages/${ECOSYSTEM}/${PACKAGE_NAME}/versions/${VERSION}`
    )
  );
});

describe('package version route', () => {
  test('renders normalized dependency overview from ecosystem metadata', async () => {
    currentScenario = createScenario({
      release: {
        ecosystem_metadata: {
          kind: 'composer',
          details: {
            require: {
              php: '^8.3',
              'psr/log': '^3.0',
            },
            'require-dev': {
              phpunit: '^11.0',
            },
          },
        },
      },
    });

    const { target, unmount } = await renderSvelte(VersionPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Dependency overview');
        expect(target.textContent).toContain('3 total');
        expect(target.textContent).toContain('Runtime require');
        expect(target.textContent).toContain('Development require');
        expect(target.textContent).toContain('psr/log');
        expect(target.textContent).toContain('phpunit');
      });
    } finally {
      unmount();
    }
  });

  test('renders risk posture from bundle analysis signals', async () => {
    currentScenario = createScenario({
      release: {
        bundle_analysis: {
          direct_dependency_count: 12,
          install_script_count: 1,
          has_native_code: true,
          risk: {
            score: 54,
            level: 'high',
            unresolved_finding_count: 1,
            worst_unresolved_severity: 'high',
            factors: [
              '1 unresolved security finding (worst severity: high)',
              '1 install lifecycle script runs during install',
              'Native build tooling is detected',
              '12 direct dependencies increase the supply-chain surface',
            ],
          },
        },
      },
    });

    const { target, unmount } = await renderSvelte(VersionPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Risk posture');
        expect(target.textContent).toContain('High risk');
        expect(target.textContent).toContain('Worst unresolved finding');
        expect(target.textContent).toContain(
          '1 unresolved security finding (worst severity: high)'
        );
        expect(target.textContent).toContain(
          'Native build tooling is detected'
        );
      });
    } finally {
      unmount();
    }
  });

  test('publishes a quarantine release and keeps the success notice visible', async () => {
    currentScenario = createScenario({
      release: {
        status: 'quarantine',
        published_at: null,
      },
      artifacts: [
        {
          filename: ARTIFACT_FILENAME,
          kind: 'tarball',
          content_type: 'application/octet-stream',
          size_bytes: 512,
          sha256: 'release-sha',
          uploaded_at: '2026-04-02T00:00:00Z',
          is_signed: false,
          signature_key_id: null,
        },
      ],
    });

    const { target, unmount, flush } = await renderSvelte(VersionPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Publish release');
        expect(target.textContent).toContain('ready to publish');
      });

      click(queryRequiredButtonByText(target, 'Publish release'));

      await waitFor(() => {
        expect(target.textContent).toContain('Release submitted for scanning.');
        expect(target.textContent).toContain('being scanned');
        expect(target.textContent).not.toContain('Publish release');
      });

      expect(currentScenario?.mutations).toEqual([
        `POST /v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/publish`,
      ]);
    } finally {
      unmount();
    }
  });

  test('surfaces publish failures without hiding the publish action', async () => {
    currentScenario = createScenario({
      release: {
        status: 'quarantine',
        published_at: null,
      },
      errors: {
        publish: 'Scan queue unavailable',
      },
    });

    const { target, unmount, flush } = await renderSvelte(VersionPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Publish release');
      });

      click(queryRequiredButtonByText(target, 'Publish release'));

      await waitFor(() => {
        expect(target.textContent).toContain('Scan queue unavailable');
        expect(target.textContent).toContain('Publish release');
      });

      expect(currentScenario?.mutations).toEqual([
        `POST /v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/publish`,
      ]);
    } finally {
      unmount();
    }
  });

  test('uploads a signed artifact and refreshes the artifact list', async () => {
    currentScenario = createScenario({
      release: {
        status: 'quarantine',
        published_at: null,
      },
      artifacts: [],
    });

    const { target, unmount, flush } = await renderSvelte(VersionPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('No artifacts available.');
      });

      const artifactFile = new File(['artifact bytes'], ARTIFACT_FILENAME, {
        type: 'application/octet-stream',
      });

      setSelectedFile(
        queryRequiredInput(target, '#artifact-file'),
        artifactFile
      );
      changeValue(queryRequiredInput(target, '#artifact-sha256'), 'deadbeef');
      setChecked(queryRequiredCheckbox(target, 'input[type="checkbox"]'), true);
      flush();

      await waitFor(() => {
        expect(target.querySelector('#signature-key-id')).not.toBeNull();
      });

      changeValue(queryRequiredInput(target, '#signature-key-id'), 'sig-key-1');
      flush();
      submitForm(
        queryRequiredForm(
          queryRequiredInput(target, '#artifact-file').closest('form')
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(`Uploaded ${ARTIFACT_FILENAME}.`);
        expect(target.textContent).toContain(ARTIFACT_FILENAME);
        expect(target.textContent).toContain('Tarball');
        expect(target.textContent).toContain('signed');
      });

      expect(currentScenario?.mutations).toEqual([
        `PUT /v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/artifacts/${ARTIFACT_FILENAME}`,
      ]);
      expect(currentScenario?.mutationCalls).toHaveLength(1);
      expect(currentScenario?.mutationCalls[0]?.query).toEqual({
        kind: 'tarball',
        sha256: 'deadbeef',
        is_signed: true,
        signature_key_id: 'sig-key-1',
      });
      expect(currentScenario?.mutationCalls[0]?.headers).toEqual({
        'Content-Type': 'application/octet-stream',
      });
      expect(currentScenario?.mutationCalls[0]?.body).toBeInstanceOf(File);
      expect((currentScenario?.mutationCalls[0]?.body as File).name).toBe(
        ARTIFACT_FILENAME
      );
    } finally {
      unmount();
    }
  });

  test('validates that an artifact file is selected before upload', async () => {
    currentScenario = createScenario({
      release: {
        status: 'quarantine',
        published_at: null,
      },
      artifacts: [],
    });

    const { target, unmount } = await renderSvelte(VersionPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Upload artifact');
      });

      submitForm(
        queryRequiredForm(
          queryRequiredInput(target, '#artifact-file').closest('form')
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Choose an artifact file to upload.'
        );
      });

      expect(currentScenario?.mutations).toEqual([]);
    } finally {
      unmount();
    }
  });

  test('yanks a published release with a reason and offers a restore action', async () => {
    currentScenario = createScenario();

    const { target, unmount } = await renderSvelte(VersionPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Yank release');
      });

      changeValue(
        queryRequiredInput(target, '#yank-reason'),
        'Supply chain issue'
      );
      click(queryRequiredButtonByText(target, 'Yank release'));

      await waitFor(() => {
        expect(target.textContent).toContain('Release yanked successfully.');
        expect(target.textContent).toContain('Restore release');
        expect(target.textContent).toContain('yank reason: Supply chain issue');
      });

      expect(currentScenario?.mutations).toEqual([
        `PUT /v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/yank`,
      ]);
      expect(currentScenario?.mutationCalls[0]?.body).toEqual({
        reason: 'Supply chain issue',
      });
    } finally {
      unmount();
    }
  });

  test('restores a yanked release and keeps the success notice visible', async () => {
    currentScenario = createScenario({
      release: {
        status: 'yanked',
        is_yanked: true,
        yank_reason: 'Supply chain issue',
      },
    });

    const { target, unmount } = await renderSvelte(VersionPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Restore release');
        expect(target.textContent).toContain('yanked');
      });

      click(queryRequiredButtonByText(target, 'Restore release'));

      await waitFor(() => {
        expect(target.textContent).toContain('Release restored successfully.');
        expect(target.textContent).toContain('Yank release');
        expect(target.textContent).not.toContain('Restore release');
      });

      expect(currentScenario?.mutations).toEqual([
        `PUT /v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/unyank`,
      ]);
    } finally {
      unmount();
    }
  });

  test('deprecates a published release and shows the remove deprecation action', async () => {
    currentScenario = createScenario();

    const { target, unmount } = await renderSvelte(VersionPage.default);

    try {
      await waitFor(() => {
        expect(target.textContent).toContain('Deprecate release');
      });

      changeValue(
        queryRequiredTextarea(target, '#deprecation-message'),
        'Use 2.0.0 instead'
      );
      submitForm(
        queryRequiredForm(
          queryRequiredTextarea(target, '#deprecation-message').closest('form')
        )
      );

      await waitFor(() => {
        expect(target.textContent).toContain(
          'Release deprecated successfully.'
        );
        expect(target.textContent).toContain('Remove deprecation');
        expect(target.textContent).toContain('deprecation: Use 2.0.0 instead');
      });

      expect(currentScenario?.mutations).toEqual([
        `PUT /v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/deprecate`,
      ]);
      expect(currentScenario?.mutationCalls[0]?.body).toEqual({
        message: 'Use 2.0.0 instead',
      });
    } finally {
      unmount();
    }
  });

  test('removes deprecation from a deprecated release', async () => {
    currentScenario = createScenario({
      release: {
        status: 'deprecated',
        is_deprecated: true,
        deprecation_message: 'Use 2.0.0 instead',
      },
    });

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

      expect(currentScenario?.mutations).toEqual([
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
  options?: ApiRequestOptions
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
    method === 'POST' &&
    path ===
      `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/publish`
  ) {
    currentScenario.mutations.push(`${method} ${path}`);
    currentScenario.mutationCalls.push({ method, path, ...options });

    if (currentScenario.errors.publish) {
      throw new MockApiError(503, { error: currentScenario.errors.publish });
    }

    currentScenario.release = {
      ...currentScenario.release,
      status: 'scanning',
      published_at: null,
    };

    return apiResponse({
      message: 'Release submitted for scanning.',
      version: VERSION,
      status: 'scanning',
    });
  }

  if (
    method === 'PUT' &&
    path.startsWith(
      `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/artifacts/`
    )
  ) {
    currentScenario.mutations.push(`${method} ${path}`);
    currentScenario.mutationCalls.push({ method, path, ...options });

    if (currentScenario.errors.uploadArtifact) {
      throw new MockApiError(500, {
        error: currentScenario.errors.uploadArtifact,
      });
    }

    const filename = decodeURIComponent(
      path.split('/').at(-1) || 'artifact.bin'
    );
    const fileBody = options?.body;
    const artifact = {
      filename,
      kind:
        typeof options?.query?.kind === 'string'
          ? options.query.kind
          : 'tarball',
      content_type:
        options?.headers?.['Content-Type'] ||
        (fileBody instanceof File ? fileBody.type : '') ||
        'application/octet-stream',
      size_bytes: fileBody instanceof File ? fileBody.size : null,
      sha256:
        typeof options?.query?.sha256 === 'string'
          ? options.query.sha256
          : null,
      uploaded_at: '2026-04-03T00:00:00Z',
      is_signed: options?.query?.is_signed === true,
      signature_key_id:
        typeof options?.query?.signature_key_id === 'string'
          ? options.query.signature_key_id
          : null,
    };

    currentScenario.artifacts = [...currentScenario.artifacts, artifact];
    return apiResponse(artifact);
  }

  if (
    method === 'PUT' &&
    path ===
      `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/yank`
  ) {
    currentScenario.mutations.push(`${method} ${path}`);
    currentScenario.mutationCalls.push({ method, path, ...options });

    if (currentScenario.errors.yank) {
      throw new MockApiError(500, { error: currentScenario.errors.yank });
    }

    const reason =
      options?.body &&
      typeof options.body === 'object' &&
      'reason' in (options.body as Record<string, unknown>)
        ? ((options.body as Record<string, unknown>).reason ?? null)
        : null;

    currentScenario.release = {
      ...currentScenario.release,
      status: 'yanked',
      is_yanked: true,
      yank_reason: reason,
    };

    return apiResponse({
      message: 'Release yanked successfully.',
      version: VERSION,
      status: 'yanked',
    });
  }

  if (
    method === 'PUT' &&
    path ===
      `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/unyank`
  ) {
    currentScenario.mutations.push(`${method} ${path}`);
    currentScenario.mutationCalls.push({ method, path, ...options });

    if (currentScenario.errors.restore) {
      throw new MockApiError(500, { error: currentScenario.errors.restore });
    }

    currentScenario.release = {
      ...currentScenario.release,
      status: currentScenario.release.is_deprecated
        ? 'deprecated'
        : 'published',
      is_yanked: false,
      yank_reason: null,
    };

    return apiResponse({
      message: 'Release restored successfully.',
      version: VERSION,
      status: currentScenario.release.is_deprecated
        ? 'deprecated'
        : 'published',
    });
  }

  if (
    method === 'PUT' &&
    path ===
      `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/deprecate`
  ) {
    currentScenario.mutations.push(`${method} ${path}`);
    currentScenario.mutationCalls.push({ method, path, ...options });

    if (currentScenario.errors.deprecate) {
      throw new MockApiError(500, { error: currentScenario.errors.deprecate });
    }

    const message =
      options?.body &&
      typeof options.body === 'object' &&
      'message' in (options.body as Record<string, unknown>)
        ? ((options.body as Record<string, unknown>).message ?? null)
        : null;

    currentScenario.release = {
      ...currentScenario.release,
      status: 'deprecated',
      is_deprecated: true,
      deprecation_message: message,
    };

    return apiResponse({
      message: 'Release deprecated successfully.',
      version: VERSION,
      status: 'deprecated',
    });
  }

  if (
    method === 'PUT' &&
    path ===
      `/v1/packages/${ECOSYSTEM}/${PACKAGE_NAME}/releases/${VERSION}/undeprecate`
  ) {
    currentScenario.mutations.push(`${method} ${path}`);
    currentScenario.mutationCalls.push({ method, path, ...options });

    if (currentScenario.errors.undeprecate) {
      throw new MockApiError(500, {
        error: currentScenario.errors.undeprecate,
      });
    }

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

function createScenario(
  overrides: Partial<Scenario> & {
    release?: JsonRecord;
    errors?: ScenarioErrors;
  } = {}
): Scenario {
  const baseRelease: JsonRecord = {
    ecosystem: ECOSYSTEM,
    name: PACKAGE_NAME,
    version: VERSION,
    status: 'published',
    is_deprecated: false,
    is_yanked: false,
    deprecation_message: null,
    yank_reason: null,
    can_manage_releases: true,
    created_at: '2026-04-01T00:00:00Z',
    published_at: '2026-04-02T00:00:00Z',
    source_ref: 'refs/tags/v1.2.3',
    description: 'Demo release used for route coverage.',
  };

  return {
    release: {
      ...baseRelease,
      ...(overrides.release || {}),
    },
    artifacts: overrides.artifacts || [
      {
        filename: ARTIFACT_FILENAME,
        kind: 'tarball',
        content_type: 'application/octet-stream',
        size_bytes: 512,
        sha256: 'release-sha',
        uploaded_at: '2026-04-02T00:00:00Z',
        is_signed: false,
        signature_key_id: null,
      },
    ],
    mutations: overrides.mutations || [],
    mutationCalls: overrides.mutationCalls || [],
    errors: {
      ...(overrides.errors || {}),
    },
  };
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

function queryRequiredButtonByText(
  target: ParentNode,
  text: string
): HTMLButtonElement {
  const button = Array.from(target.querySelectorAll('button')).find(
    (element) => element.textContent?.trim() === text
  );

  if (!(button instanceof HTMLButtonElement)) {
    throw new Error(`Missing button with text ${text}.`);
  }

  return button;
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

function queryRequiredCheckbox(
  target: ParentNode,
  selector: string
): HTMLInputElement {
  const element = queryRequiredInput(target, selector);
  if (element.type !== 'checkbox') {
    throw new Error(`Expected checkbox for selector ${selector}.`);
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

function queryRequiredTextarea(
  target: ParentNode,
  selector: string
): HTMLTextAreaElement {
  const element = target.querySelector(selector);
  if (!(element instanceof HTMLTextAreaElement)) {
    throw new Error(`Missing textarea for selector ${selector}.`);
  }
  return element;
}

function queryRequiredForm(element: Element | null): HTMLFormElement {
  if (!(element instanceof HTMLFormElement)) {
    throw new Error('Missing form.');
  }
  return element;
}

function setSelectedFile(input: HTMLInputElement, file: File): void {
  Object.defineProperty(input, 'files', {
    configurable: true,
    value: [file] as unknown as FileList,
  });
  input.dispatchEvent(new Event('input', { bubbles: true }));
  input.dispatchEvent(new Event('change', { bubbles: true }));
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
