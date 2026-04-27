/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';
import { fileURLToPath } from 'node:url';

import type { NamespaceClaim } from '../src/api/namespaces';
import type { Team } from '../src/api/orgs';
import type { OrgPackageSummary, OrgRepositorySummary } from '../src/api/orgs';
import type { OrgDestructiveActionsMutations } from '../src/pages/org-destructive-actions';
import { renderPackageSelectionValue } from '../src/pages/org-workspace-actions';
import {
  changeValue,
  click,
  renderSvelte,
  setChecked,
  submitForm,
} from './svelte-dom';

const HarnessPath = fileURLToPath(
  new URL('./fixtures/org-destructive-actions-harness.svelte', import.meta.url)
);

describe('org destructive actions controller harness', () => {
  test('requires confirmation before deleting a team and removes it on success', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async deleteTeam(_slug, teamSlug) {
          scenario.teamDeleteCalls.push(teamSlug);
          const deletedTeam = scenario.teams.find((team) => team.slug === teamSlug);
          scenario.teams = scenario.teams.filter((team) => team.slug !== teamSlug);
          return deletedTeam || {};
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Release Engineering');
      });

      click(queryRequiredButton(target, '#team-delete-toggle-release-engineering'));
      await waitFor(() => {
        flush();
        expect(
          queryRequiredForm(target, '#team-delete-form-release-engineering')
        ).toBeDefined();
      });

      submitForm(queryRequiredForm(target, '#team-delete-form-release-engineering'));
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Please confirm that you understand deleting this team revokes its delegated access.'
        );
      });

      setChecked(
        queryRequiredInput(target, '#team-delete-confirm-release-engineering'),
        true
      );
      submitForm(queryRequiredForm(target, '#team-delete-form-release-engineering'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Deleted team release-engineering.');
        expect(target.querySelector('[data-test="team-release-engineering"]')).toBeNull();
      });

      expect(scenario.teamDeleteCalls).toEqual(['release-engineering']);
    } finally {
      unmount();
    }
  });

  test('keeps the team delete confirmation open when deletion fails', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async deleteTeam() {
          throw new Error('Failed to delete team.');
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Release Engineering');
      });

      click(queryRequiredButton(target, '#team-delete-toggle-release-engineering'));
      await waitFor(() => {
        flush();
        expect(
          queryRequiredForm(target, '#team-delete-form-release-engineering')
        ).toBeDefined();
      });

      setChecked(
        queryRequiredInput(target, '#team-delete-confirm-release-engineering'),
        true
      );
      submitForm(queryRequiredForm(target, '#team-delete-form-release-engineering'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Failed to delete team.');
        expect(
          queryRequiredForm(target, '#team-delete-form-release-engineering')
        ).toBeDefined();
      });
    } finally {
      unmount();
    }
  });

  test('requires confirmation before deleting a namespace claim and removes it on success', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async deleteNamespaceClaim(claimId) {
          scenario.namespaceDeleteCalls.push(claimId);
          scenario.namespaces = scenario.namespaces.filter(
            (claim) => claim.id !== claimId
          );
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('@source-org');
      });

      click(queryRequiredButton(target, '#namespace-delete-toggle-claim-1'));
      await waitFor(() => {
        flush();
        expect(queryRequiredForm(target, '#namespace-delete-form-claim-1')).toBeDefined();
      });

      submitForm(queryRequiredForm(target, '#namespace-delete-form-claim-1'));
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Please confirm that you understand deleting this namespace claim is immediate and cannot be undone.'
        );
      });

      setChecked(queryRequiredInput(target, '#namespace-delete-confirm-claim-1'), true);
      submitForm(queryRequiredForm(target, '#namespace-delete-form-claim-1'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Deleted namespace claim @source-org.');
        expect(target.querySelector('[data-test="namespace-claim-1"]')).toBeNull();
      });

      expect(scenario.namespaceDeleteCalls).toEqual(['claim-1']);
    } finally {
      unmount();
    }
  });

  test('keeps the namespace delete confirmation open when deletion fails', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async deleteNamespaceClaim() {
          throw new Error('Failed to delete namespace claim.');
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('@source-org');
      });

      click(queryRequiredButton(target, '#namespace-delete-toggle-claim-1'));
      await waitFor(() => {
        flush();
        expect(queryRequiredForm(target, '#namespace-delete-form-claim-1')).toBeDefined();
      });

      setChecked(queryRequiredInput(target, '#namespace-delete-confirm-claim-1'), true);
      submitForm(queryRequiredForm(target, '#namespace-delete-form-claim-1'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Failed to delete namespace claim.');
        expect(queryRequiredForm(target, '#namespace-delete-form-claim-1')).toBeDefined();
      });
    } finally {
      unmount();
    }
  });

  test('requires confirmation before transferring a namespace claim and records the transfer on success', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async transferNamespaceClaim(claimId, input) {
          scenario.namespaceTransferCalls.push({ claimId, targetOrgSlug: input.targetOrgSlug });
          return {
            namespace_claim: {
              id: claimId,
              namespace: '@source-org',
            },
            owner: {
              slug: input.targetOrgSlug,
            },
          };
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Transfer a namespace');
      });

      changeValue(queryRequiredInput(target, '#org-namespace-transfer-claim'), 'claim-1');
      changeValue(queryRequiredInput(target, '#org-namespace-transfer-target'), 'target-org');
      flush();
      click(queryRequiredButton(target, '#org-namespace-transfer-toggle'));

      await waitFor(() => {
        flush();
        expect(queryRequiredInput(target, '#org-namespace-transfer-confirm')).toBeDefined();
      });

      changeValue(queryRequiredInput(target, '#org-namespace-transfer-claim'), 'claim-1');
      changeValue(queryRequiredInput(target, '#org-namespace-transfer-target'), 'target-org');
      flush();
      submitForm(queryRequiredForm(queryRequiredInput(target, '#org-namespace-transfer-claim').closest('form')));
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Please confirm the namespace transfer.');
      });

      setChecked(queryRequiredInput(target, '#org-namespace-transfer-confirm'), true);
      submitForm(queryRequiredForm(queryRequiredSelect(target, '#org-namespace-transfer-claim').closest('form')));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Transferred @source-org to target-org.');
      });

      expect(scenario.namespaceTransferCalls).toEqual([
        { claimId: 'claim-1', targetOrgSlug: 'target-org' },
      ]);
    } finally {
      unmount();
    }
  });

  test('keeps the namespace transfer confirmation open when transfer fails', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async transferNamespaceClaim() {
          throw new Error('Failed to transfer namespace claim ownership.');
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Transfer a namespace');
      });

      changeValue(queryRequiredInput(target, '#org-namespace-transfer-claim'), 'claim-1');
      changeValue(queryRequiredInput(target, '#org-namespace-transfer-target'), 'target-org');
      flush();
      click(queryRequiredButton(target, '#org-namespace-transfer-toggle'));

      await waitFor(() => {
        flush();
        expect(queryRequiredInput(target, '#org-namespace-transfer-confirm')).toBeDefined();
      });

      changeValue(queryRequiredInput(target, '#org-namespace-transfer-claim'), 'claim-1');
      changeValue(queryRequiredInput(target, '#org-namespace-transfer-target'), 'target-org');
      flush();
      setChecked(queryRequiredInput(target, '#org-namespace-transfer-confirm'), true);
      submitForm(queryRequiredForm(queryRequiredInput(target, '#org-namespace-transfer-claim').closest('form')));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Failed to transfer namespace claim ownership.'
        );
        expect(queryRequiredButton(target, '#org-namespace-transfer-submit')).toBeDefined();
      });
    } finally {
      unmount();
    }
  });

  test('requires confirmation before transferring a repository and records the transfer on success', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async transferRepositoryOwnership(repositorySlug, input) {
          scenario.repositoryTransferCalls.push({
            repositorySlug,
            targetOrgSlug: input.targetOrgSlug,
          });
          return {
            repository: {
              slug: repositorySlug,
              name: 'Repository Alpha',
            },
            owner: {
              slug: input.targetOrgSlug,
            },
          };
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Transfer repository ownership');
      });

      changeValue(
        queryRequiredInput(target, '#org-repository-transfer-repository'),
        'repo-alpha'
      );
      changeValue(queryRequiredInput(target, '#org-repository-transfer-target'), 'target-org');
      flush();
      click(queryRequiredButton(target, '#org-repository-transfer-toggle'));

      await waitFor(() => {
        flush();
        expect(queryRequiredInput(target, '#org-repository-transfer-confirm')).toBeDefined();
      });

      changeValue(
        queryRequiredInput(target, '#org-repository-transfer-repository'),
        'repo-alpha'
      );
      changeValue(queryRequiredInput(target, '#org-repository-transfer-target'), 'target-org');
      flush();
      submitForm(queryRequiredForm(queryRequiredInput(target, '#org-repository-transfer-repository').closest('form')));
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Please confirm the repository transfer.');
      });

      setChecked(queryRequiredInput(target, '#org-repository-transfer-confirm'), true);
      submitForm(queryRequiredForm(queryRequiredSelect(target, '#org-repository-transfer-repository').closest('form')));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Transferred Repository Alpha to target-org.'
        );
      });

      expect(scenario.repositoryTransferCalls).toEqual([
        { repositorySlug: 'repo-alpha', targetOrgSlug: 'target-org' },
      ]);
    } finally {
      unmount();
    }
  });

  test('keeps the repository transfer confirmation open when transfer fails', async () => {
    const scenario = createScenario();
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async transferRepositoryOwnership() {
          throw new Error('Failed to transfer repository ownership.');
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Transfer repository ownership');
      });

      changeValue(
        queryRequiredInput(target, '#org-repository-transfer-repository'),
        'repo-alpha'
      );
      changeValue(queryRequiredInput(target, '#org-repository-transfer-target'), 'target-org');
      flush();
      click(queryRequiredButton(target, '#org-repository-transfer-toggle'));

      await waitFor(() => {
        flush();
        expect(queryRequiredInput(target, '#org-repository-transfer-confirm')).toBeDefined();
      });

      changeValue(
        queryRequiredInput(target, '#org-repository-transfer-repository'),
        'repo-alpha'
      );
      changeValue(queryRequiredInput(target, '#org-repository-transfer-target'), 'target-org');
      flush();
      setChecked(queryRequiredInput(target, '#org-repository-transfer-confirm'), true);
      submitForm(queryRequiredForm(queryRequiredInput(target, '#org-repository-transfer-repository').closest('form')));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Failed to transfer repository ownership.');
        expect(queryRequiredButton(target, '#org-repository-transfer-submit')).toBeDefined();
      });
    } finally {
      unmount();
    }
  });

  test('requires confirmation before transferring a package and records the transfer on success', async () => {
    const scenario = createScenario();
    const packageKey = renderPackageSelectionValue('npm', 'source-package');
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async transferPackageOwnership(ecosystem, packageName, input) {
          scenario.packageTransferCalls.push({
            ecosystem,
            packageName,
            targetOrgSlug: input.targetOrgSlug,
          });
          return {
            package: {
              ecosystem,
              name: packageName,
            },
            owner: {
              slug: input.targetOrgSlug,
            },
          };
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Transfer package ownership');
      });

      changeValue(queryRequiredInput(target, '#org-package-transfer-package'), packageKey);
      changeValue(queryRequiredInput(target, '#org-package-transfer-target'), 'target-org');
      flush();
      click(queryRequiredButton(target, '#org-package-transfer-toggle'));

      await waitFor(() => {
        flush();
        expect(queryRequiredInput(target, '#org-package-transfer-confirm')).toBeDefined();
      });

      changeValue(queryRequiredInput(target, '#org-package-transfer-package'), packageKey);
      changeValue(queryRequiredInput(target, '#org-package-transfer-target'), 'target-org');
      flush();
      submitForm(queryRequiredForm(queryRequiredInput(target, '#org-package-transfer-package').closest('form')));
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Please confirm the package transfer.');
      });

      setChecked(queryRequiredInput(target, '#org-package-transfer-confirm'), true);
      submitForm(queryRequiredForm(queryRequiredSelect(target, '#org-package-transfer-package').closest('form')));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'Transferred source-package to target-org.'
        );
      });

      expect(scenario.packageTransferCalls).toEqual([
        {
          ecosystem: 'npm',
          packageName: 'source-package',
          targetOrgSlug: 'target-org',
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('keeps the package transfer confirmation open when transfer fails', async () => {
    const scenario = createScenario();
    const packageKey = renderPackageSelectionValue('npm', 'source-package');
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      loadState: createLoadState(scenario),
      mutations: createMutations({
        async transferPackageOwnership() {
          throw new Error('Failed to transfer package ownership.');
        },
      }),
    });

    try {
      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Transfer package ownership');
      });

      changeValue(queryRequiredInput(target, '#org-package-transfer-package'), packageKey);
      changeValue(queryRequiredInput(target, '#org-package-transfer-target'), 'target-org');
      flush();
      click(queryRequiredButton(target, '#org-package-transfer-toggle'));

      await waitFor(() => {
        flush();
        expect(queryRequiredInput(target, '#org-package-transfer-confirm')).toBeDefined();
      });

      changeValue(queryRequiredInput(target, '#org-package-transfer-package'), packageKey);
      changeValue(queryRequiredInput(target, '#org-package-transfer-target'), 'target-org');
      flush();
      setChecked(queryRequiredInput(target, '#org-package-transfer-confirm'), true);
      submitForm(queryRequiredForm(queryRequiredInput(target, '#org-package-transfer-package').closest('form')));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Failed to transfer package ownership.');
        expect(queryRequiredButton(target, '#org-package-transfer-submit')).toBeDefined();
      });
    } finally {
      unmount();
    }
  });
});

function createLoadState(scenario: Scenario) {
  return async () => ({
    teams: scenario.teams.map((team) => ({ ...team })),
    namespaces: scenario.namespaces.map((claim) => ({ ...claim })),
    repositories: scenario.repositories.map((repository) => ({ ...repository })),
    packages: scenario.packages.map((pkg) => ({ ...pkg })),
  });
}

function createMutations(
  overrides: Partial<OrgDestructiveActionsMutations> = {}
): OrgDestructiveActionsMutations {
  return {
    async deleteTeam(_slug, teamSlug) {
      return {
        slug: teamSlug,
      };
    },
    async deleteNamespaceClaim() {},
    async transferNamespaceClaim(claimId, input) {
      return {
        namespace_claim: {
          id: claimId,
        },
        owner: {
          slug: input.targetOrgSlug,
        },
      };
    },
    async transferRepositoryOwnership(repositorySlug, input) {
      return {
        repository: {
          slug: repositorySlug,
        },
        owner: {
          slug: input.targetOrgSlug,
        },
      };
    },
    async transferPackageOwnership(ecosystem, name, input) {
      return {
        package: {
          ecosystem,
          name,
        },
        owner: {
          slug: input.targetOrgSlug,
        },
      };
    },
    ...overrides,
  };
}

interface Scenario {
  teams: Team[];
  namespaces: NamespaceClaim[];
  repositories: OrgRepositorySummary[];
  packages: OrgPackageSummary[];
  teamDeleteCalls: string[];
  namespaceDeleteCalls: string[];
  namespaceTransferCalls: Array<{ claimId: string; targetOrgSlug: string }>;
  repositoryTransferCalls: Array<{
    repositorySlug: string;
    targetOrgSlug: string;
  }>;
  packageTransferCalls: Array<{
    ecosystem: string;
    packageName: string;
    targetOrgSlug: string;
  }>;
}

function createScenario(): Scenario {
  return {
    teams: [
      {
        name: 'Release Engineering',
        slug: 'release-engineering',
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
      },
    ],
    packages: [
      {
        id: 'pkg-1',
        ecosystem: 'npm',
        name: 'source-package',
      },
    ],
    teamDeleteCalls: [],
    namespaceDeleteCalls: [],
    namespaceTransferCalls: [],
    repositoryTransferCalls: [],
    packageTransferCalls: [],
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

function queryRequiredButton(
  target: HTMLElement,
  selector: string
): HTMLButtonElement {
  const button = target.querySelector(selector);
  expect(button).not.toBeNull();
  return button as HTMLButtonElement;
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
