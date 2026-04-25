/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';
import { fileURLToPath } from 'node:url';

import type { NamespaceClaim } from '../src/api/namespaces';
import type { Team } from '../src/api/orgs';
import type { OrgPackageSummary, OrgRepositorySummary } from '../src/api/orgs';
import type { OrgNonDestructiveActionsMutations } from '../src/pages/org-non-destructive-actions';
import {
  changeValue,
  renderSvelte,
  submitForm,
} from './svelte-dom';

const HarnessPath = fileURLToPath(
  new URL('./fixtures/org-non-destructive-actions-harness.svelte', import.meta.url)
);

describe('org non-destructive actions controller harness', () => {
  test('creates teams and resets the form on success', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async createTeam(_slug, input) {
          scenario.createTeamCalls.push({
            name: input.name,
            slug: input.slug,
            description:
              typeof input.description === 'string' ? input.description : undefined,
          });
          scenario.teams = [
            ...scenario.teams,
            {
              slug: input.slug,
              name: input.name,
              description: input.description || null,
            },
          ];
          return scenario.teams.at(-1) || {};
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Release Engineering');
      });

      changeValue(queryRequiredInput(target, '#team-create-name'), 'Quality Engineering');
      changeValue(queryRequiredInput(target, '#team-create-slug'), 'quality-engineering');
      changeValue(
        queryRequiredTextArea(target, '#team-create-description'),
        'Quality gate owners'
      );
      submitForm(queryRequiredForm(target, '#team-create-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Team created successfully.');
        expect(target.textContent).toContain('Quality Engineering');
      });

      expect(queryRequiredInput(target, '#team-create-name').value).toBe('');
      expect(queryRequiredInput(target, '#team-create-slug').value).toBe('');
      expect(scenario.createTeamCalls).toEqual([
        {
          name: 'Quality Engineering',
          slug: 'quality-engineering',
          description: 'Quality gate owners',
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('surfaces team creation failures', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async createTeam() {
          throw new Error('Failed to create team.');
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Release Engineering');
      });

      changeValue(queryRequiredInput(target, '#team-create-name'), 'Quality Engineering');
      changeValue(queryRequiredInput(target, '#team-create-slug'), 'quality-engineering');
      submitForm(queryRequiredForm(target, '#team-create-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Failed to create team.');
      });

      expect(target.textContent).not.toContain('Quality Engineering');
    } finally {
      unmount();
    }
  });

  test('creates namespace claims, creates repositories, and updates repository settings on success', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async createNamespaceClaim(input) {
          scenario.createNamespaceCalls.push({ ...input });
          scenario.namespaces = [
            ...scenario.namespaces,
            {
              id: 'claim-2',
              ecosystem: input.ecosystem,
              namespace: input.namespace,
            },
          ];
          return scenario.namespaces.at(-1) || {};
        },
        async createRepository(input) {
          scenario.createRepositoryCalls.push({ ...input });
          scenario.repositories = [
            ...scenario.repositories,
            {
              id: 'repo-2',
              slug: input.slug,
              name: input.name,
              kind: input.kind,
              visibility: input.visibility,
              description: input.description || null,
            },
          ];
          return scenario.repositories.at(-1) || {};
        },
        async updateRepository(repositorySlug, input) {
          scenario.updateRepositoryCalls.push({
            repositorySlug,
            ...input,
          });
          scenario.repositories = scenario.repositories.map((repository) =>
            repository.slug === repositorySlug
              ? {
                  ...repository,
                  description: input.description || null,
                  visibility: input.visibility || repository.visibility,
                }
              : repository
          );
          return {};
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Repository Alpha');
      });

      changeValue(queryRequiredSelect(target, '#namespace-ecosystem'), 'cargo');
      changeValue(queryRequiredInput(target, '#namespace-value'), 'acme_tools');
      submitForm(queryRequiredForm(target, '#namespace-create-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Created the Cargo namespace claim acme_tools.'
        );
        expect(target.textContent).toContain('acme_tools');
      });

      changeValue(queryRequiredInput(target, '#repository-create-name'), 'Repository Beta');
      changeValue(queryRequiredInput(target, '#repository-create-slug'), 'repo-beta');
      changeValue(queryRequiredSelect(target, '#repository-create-kind'), 'release');
      changeValue(queryRequiredSelect(target, '#repository-create-visibility'), 'private');
      changeValue(
        queryRequiredTextArea(target, '#repository-create-description'),
        'New private release repository'
      );
      submitForm(queryRequiredForm(target, '#repository-create-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Repository created successfully.');
        expect(target.textContent).toContain('Repository Beta');
      });

      changeValue(queryRequiredSelect(target, '#repository-visibility-repo-alpha'), 'quarantined');
      changeValue(
        queryRequiredTextArea(target, '#repository-description-repo-alpha'),
        'Updated quarantine staging'
      );
      submitForm(queryRequiredForm(target, '#repository-update-form-repo-alpha'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Updated repository repo-alpha.');
        expect(target.textContent).toContain('Updated quarantine staging');
        expect(target.textContent).toContain('quarantined');
      });

      expect(scenario.createNamespaceCalls).toEqual([
        {
          ecosystem: 'cargo',
          namespace: 'acme_tools',
          ownerOrgId: 'org-1',
        },
      ]);
      expect(scenario.createRepositoryCalls).toEqual([
        {
          name: 'Repository Beta',
          slug: 'repo-beta',
          kind: 'release',
          visibility: 'private',
          description: 'New private release repository',
          ownerOrgId: 'org-1',
        },
      ]);
      expect(scenario.updateRepositoryCalls).toEqual([
        {
          repositorySlug: 'repo-alpha',
          description: 'Updated quarantine staging',
          visibility: 'quarantined',
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('surfaces namespace and repository creation/update failures', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async createNamespaceClaim() {
          throw new Error('Failed to create namespace claim.');
        },
        async createRepository() {
          throw new Error('Failed to create repository.');
        },
        async updateRepository() {
          throw new Error('Failed to update repository.');
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Repository Alpha');
      });

      changeValue(queryRequiredInput(target, '#namespace-value'), 'acme_tools');
      submitForm(queryRequiredForm(target, '#namespace-create-form'));
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Failed to create namespace claim.');
      });

      changeValue(queryRequiredInput(target, '#repository-create-name'), 'Repository Beta');
      changeValue(queryRequiredInput(target, '#repository-create-slug'), 'repo-beta');
      submitForm(queryRequiredForm(target, '#repository-create-form'));
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Failed to create repository.');
      });

      changeValue(
        queryRequiredTextArea(target, '#repository-description-repo-alpha'),
        'Updated quarantine staging'
      );
      submitForm(queryRequiredForm(target, '#repository-update-form-repo-alpha'));
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Failed to update repository.');
      });
    } finally {
      unmount();
    }
  });

  test('creates packages and resets the package draft on success', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async createPackage(input) {
          scenario.createPackageCalls.push({
            ecosystem: input.ecosystem,
            name: input.name,
            repositorySlug: input.repositorySlug,
            visibility:
              typeof input.visibility === 'string' ? input.visibility : undefined,
            displayName:
              typeof input.displayName === 'string' ? input.displayName : null,
            description:
              typeof input.description === 'string' ? input.description : null,
          });
          scenario.packages = [
            ...scenario.packages,
            {
              id: 'pkg-2',
              ecosystem: input.ecosystem,
              name: input.name,
              description:
                typeof input.description === 'string' ? input.description : null,
            },
          ];
          return scenario.packages.at(-1) || {};
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(queryRequiredSelect(target, '#package-create-repository').value).toBe(
          'repo-alpha'
        );
      });

      changeValue(queryRequiredSelect(target, '#package-create-ecosystem'), 'cargo');
      changeValue(queryRequiredInput(target, '#package-create-name'), 'acme_tools');
      changeValue(
        queryRequiredInput(target, '#package-create-display-name'),
        'Acme Tools'
      );
      const visibilitySelect = queryRequiredSelect(
        target,
        '#package-create-visibility'
      );
      const spacedVisibilityOption = document.createElement('option');
      spacedVisibilityOption.value = ' private ';
      spacedVisibilityOption.textContent = 'Private (spaced)';
      visibilitySelect.append(spacedVisibilityOption);
      changeValue(visibilitySelect, ' private ');
      changeValue(
        queryRequiredTextArea(target, '#package-create-description'),
        'Private cargo package'
      );
      submitForm(queryRequiredForm(target, '#package-create-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Created Cargo package acme_tools in Repository Alpha.'
        );
        expect(target.textContent).toContain('acme_tools');
      });

      expect(queryRequiredInput(target, '#package-create-name').value).toBe('');
      expect(queryRequiredInput(target, '#package-create-display-name').value).toBe(
        ''
      );
      expect(queryRequiredTextArea(target, '#package-create-description').value).toBe(
        ''
      );
      expect(scenario.createPackageCalls).toEqual([
        {
          ecosystem: 'cargo',
          name: 'acme_tools',
          repositorySlug: 'repo-alpha',
          visibility: 'private',
          displayName: 'Acme Tools',
          description: 'Private cargo package',
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('surfaces package creation validation and request failures', async () => {
    const emptyRepositoryScenario = createScenario();
    emptyRepositoryScenario.repositories = [];
    const emptyRender = await renderSvelte(HarnessPath, {
      loadState: createLoadState(emptyRepositoryScenario),
      mutations: createMutations(),
    });

    try {
      await waitFor(() => {
        emptyRender.flush();
        expect(emptyRender.target.textContent).toContain('Packages');
      });

      changeValue(
        queryRequiredInput(emptyRender.target, '#package-create-name'),
        'source-package'
      );
      submitForm(queryRequiredForm(emptyRender.target, '#package-create-form'));

      await waitFor(() => {
        emptyRender.flush();
        expect(emptyRender.target.textContent).toContain(
          'Create an eligible repository before creating a package.'
        );
      });
    } finally {
      emptyRender.unmount();
    }

    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async createPackage() {
          throw new Error('Failed to create package.');
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(queryRequiredSelect(target, '#package-create-repository').value).toBe(
          'repo-alpha'
        );
      });

      changeValue(queryRequiredInput(target, '#package-create-name'), 'source-package');
      submitForm(queryRequiredForm(target, '#package-create-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Failed to create package.');
      });

      expect(queryRequiredInput(target, '#package-create-name').value).toBe(
        'source-package'
      );
    } finally {
      unmount();
    }
  });
});

function createLoadState(scenario: Scenario) {
  return async () => ({
    orgId: scenario.orgId,
    teams: scenario.teams.map((team) => ({ ...team })),
    namespaces: scenario.namespaces.map((claim) => ({ ...claim })),
    repositories: scenario.repositories.map((repository) => ({ ...repository })),
    packages: scenario.packages.map((pkg) => ({ ...pkg })),
  });
}

function createMutations(
  overrides: Partial<OrgNonDestructiveActionsMutations> = {}
): OrgNonDestructiveActionsMutations {
  return {
    async createTeam(_slug, input) {
      return {
        slug: input.slug,
        name: input.name,
      };
    },
    async createNamespaceClaim(input) {
      return {
        ecosystem: input.ecosystem,
        namespace: input.namespace,
      };
    },
    async createRepository(input) {
      return {
        slug: input.slug,
        name: input.name,
      };
    },
    async updateRepository() {
      return {};
    },
    async createPackage(input) {
      return {
        ecosystem: input.ecosystem,
        name: input.name,
        repository_slug: input.repositorySlug,
      };
    },
    ...overrides,
  };
}

interface Scenario {
  orgId: string | null;
  teams: Team[];
  namespaces: NamespaceClaim[];
  repositories: OrgRepositorySummary[];
  packages: OrgPackageSummary[];
  createTeamCalls: Array<{
    name: string;
    slug: string;
    description?: string;
  }>;
  createNamespaceCalls: Array<{
    ecosystem: string;
    namespace: string;
    ownerOrgId?: string;
  }>;
  createRepositoryCalls: Array<{
    name: string;
    slug: string;
    kind: string;
    visibility: string;
    description?: string | null;
    ownerOrgId?: string;
  }>;
  updateRepositoryCalls: Array<{
    repositorySlug: string;
    description?: string | null;
    visibility?: string;
  }>;
  createPackageCalls: Array<{
    ecosystem: string;
    name: string;
    repositorySlug: string;
    visibility?: string;
    displayName?: string | null;
    description?: string | null;
  }>;
}

function createScenario(): Scenario {
  return {
    orgId: 'org-1',
    teams: [
      {
        slug: 'release-engineering',
        name: 'Release Engineering',
        description: 'Release automation owners',
      },
    ],
    namespaces: [
      {
        id: 'claim-1',
        ecosystem: 'npm',
        namespace: '@source-org',
      },
    ],
    repositories: [
      {
        id: 'repo-1',
        slug: 'repo-alpha',
        name: 'Repository Alpha',
        kind: 'private',
        visibility: 'private',
        description: 'Initial repo description',
      },
    ],
    packages: [
      {
        id: 'pkg-1',
        ecosystem: 'npm',
        name: 'source-package',
        description: 'Existing package',
      },
    ],
    createTeamCalls: [],
    createNamespaceCalls: [],
    createRepositoryCalls: [],
    updateRepositoryCalls: [],
    createPackageCalls: [],
  };
}

function queryRequiredInput(target: HTMLElement, selector: string): HTMLInputElement {
  const input = target.querySelector(selector);
  expect(input).not.toBeNull();
  return input as HTMLInputElement;
}

function queryRequiredSelect(target: HTMLElement, selector: string): HTMLSelectElement {
  const select = target.querySelector(selector);
  expect(select).not.toBeNull();
  return select as HTMLSelectElement;
}

function queryRequiredTextArea(
  target: HTMLElement,
  selector: string
): HTMLTextAreaElement {
  const textarea = target.querySelector(selector);
  expect(textarea).not.toBeNull();
  return textarea as HTMLTextAreaElement;
}

function queryRequiredForm(
  target: ParentNode | HTMLElement | null,
  selector?: string
): HTMLFormElement {
  if (selector) {
    expect(target).not.toBeNull();
    const form = (target as ParentNode).querySelector(selector);
    expect(form).not.toBeNull();
    return form as HTMLFormElement;
  }

  expect(target).not.toBeNull();
  return target as HTMLFormElement;
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

  throw lastError instanceof Error
    ? lastError
    : new Error('Timed out waiting for assertion.');
}
