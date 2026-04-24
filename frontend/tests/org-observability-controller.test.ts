/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';
import { fileURLToPath } from 'node:url';

import type { SecurityFinding } from '../src/api/packages';
import type { OrgObservabilityMutations } from '../src/pages/org-observability';
import {
  changeValue,
  click,
  renderSvelte,
  setChecked,
  submitForm,
} from './svelte-dom';

const HarnessPath = fileURLToPath(
  new URL('./fixtures/org-observability-harness.svelte', import.meta.url)
);

describe('org observability controller harness', () => {
  test('validates audit filters and navigates for audit filter actions', async () => {
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      initialSearch:
        'action=org_update&actor_user_id=11111111-1111-4111-8111-111111111111&actor_username=admin-user&occurred_from=2026-04-01&occurred_until=2026-04-10',
      mutations: createMutations(),
    });

    try {
      changeValue(queryRequiredInput(target, '#audit-filter-from'), '2026-04-11');
      changeValue(queryRequiredInput(target, '#audit-filter-until'), '2026-04-10');
      submitForm(queryRequiredForm(target, '#audit-filter-form'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain(
          'End date must be on or after the start date.'
        );
      });

      changeValue(queryRequiredSelect(target, '#audit-filter-action'), 'team_create');
      changeValue(queryRequiredInput(target, '#audit-filter-actor'), 'admin-user');
      changeValue(queryRequiredInput(target, '#audit-filter-from'), '2026-04-01');
      changeValue(queryRequiredInput(target, '#audit-filter-until'), '2026-04-10');
      submitForm(queryRequiredForm(target, '#audit-filter-form'));

      await waitFor(() => {
        flush();
        expect(queryRequiredText(target, '[data-test="last-navigation"]').textContent).toContain(
          '/orgs/source-org?action=team_create'
        );
        expect(queryRequiredText(target, '[data-test="last-navigation"]').textContent).toContain(
          'actor_user_id=11111111-1111-4111-8111-111111111111'
        );
      });

      click(queryRequiredButton(target, '#audit-clear-action'));
      await waitFor(() => {
        flush();
        expect(queryRequiredText(target, '[data-test="last-navigation"]').textContent).not.toContain(
          'action='
        );
      });

      click(queryRequiredButton(target, '#audit-focus-actor'));
      await waitFor(() => {
        flush();
        expect(queryRequiredText(target, '[data-test="last-navigation"]').textContent).toContain(
          'actor_username=admin-user'
        );
      });

      click(queryRequiredButton(target, '#audit-next-page'));
      await waitFor(() => {
        flush();
        expect(queryRequiredText(target, '[data-test="last-navigation"]').textContent).toContain(
          'page=2'
        );
      });
    } finally {
      unmount();
    }
  });

  test('exports audit logs and surfaces export failures', async () => {
    const success = await renderSvelte(HarnessPath, {
      initialSearch: 'action=org_update&actor_username=admin-user',
      mutations: createMutations({
        async exportOrgAuditLogsCsv(_slug, query) {
          expect(query?.action).toBe('org_update');
          return 'audit-log-csv';
        },
      }),
    });

    try {
      click(queryRequiredButton(success.target, '#audit-export'));

      await waitFor(() => {
        success.flush();
        expect(
          queryRequiredText(success.target, '[data-test="last-download-filename"]').textContent
        ).toContain('org-audit-source-org');
        expect(
          queryRequiredText(success.target, '[data-test="last-download-contents"]').textContent
        ).toBe('audit-log-csv');
        expect(
          queryRequiredText(
            success.target,
            '[data-test="last-download-content-type"]'
          ).textContent
        ).toBe('text/csv;charset=utf-8');
      });
    } finally {
      success.unmount();
    }

    const failure = await renderSvelte(HarnessPath, {
      mutations: createMutations({
        async exportOrgAuditLogsCsv() {
          throw new Error('Failed to export activity log.');
        },
      }),
    });

    try {
      click(queryRequiredButton(failure.target, '#audit-export'));

      await waitFor(() => {
        failure.flush();
        expect(failure.target.textContent).toContain('Failed to export activity log.');
      });
    } finally {
      failure.unmount();
    }
  });

  test('navigates for security filters and clear actions', async () => {
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      initialSearch:
        'security_severity=high&security_ecosystem=npm&security_package=security-package',
      mutations: createMutations(),
    });

    try {
      setChecked(queryRequiredInput(target, '#security-filter-severity-high'), true);
      changeValue(queryRequiredSelect(target, '#security-filter-ecosystem'), 'cargo');
      changeValue(queryRequiredInput(target, '#security-filter-package'), 'crates-audit');
      submitForm(queryRequiredForm(target, '#security-filter-form'));

      await waitFor(() => {
        flush();
        expect(queryRequiredText(target, '[data-test="last-navigation"]').textContent).toContain(
          'security_severity=high'
        );
        expect(queryRequiredText(target, '[data-test="last-navigation"]').textContent).toContain(
          'security_ecosystem=cargo'
        );
        expect(queryRequiredText(target, '[data-test="last-navigation"]').textContent).toContain(
          'security_package=crates-audit'
        );
      });

      click(queryRequiredButton(target, '#security-clear-severity'));
      await waitFor(() => {
        flush();
        expect(queryRequiredText(target, '[data-test="last-navigation"]').textContent).not.toContain(
          'security_severity='
        );
      });

      click(queryRequiredButton(target, '#security-clear-ecosystem'));
      await waitFor(() => {
        flush();
        expect(queryRequiredText(target, '[data-test="last-navigation"]').textContent).not.toContain(
          'security_ecosystem='
        );
      });

      click(queryRequiredButton(target, '#security-clear-package'));
      await waitFor(() => {
        flush();
        expect(queryRequiredText(target, '[data-test="last-navigation"]').textContent).not.toContain(
          'security_package='
        );
      });
    } finally {
      unmount();
    }
  });

  test('exports security findings and surfaces export failures', async () => {
    const success = await renderSvelte(HarnessPath, {
      initialSearch: 'security_severity=high&security_ecosystem=npm',
      mutations: createMutations({
        async exportOrgSecurityFindingsCsv(_slug, query) {
          expect(query?.severities).toEqual(['high']);
          expect(query?.ecosystem).toBe('npm');
          return 'security-csv';
        },
      }),
    });

    try {
      click(queryRequiredButton(success.target, '#security-export'));

      await waitFor(() => {
        success.flush();
        expect(
          queryRequiredText(success.target, '[data-test="last-download-filename"]').textContent
        ).toContain('org-security-source-org');
        expect(
          queryRequiredText(success.target, '[data-test="last-download-contents"]').textContent
        ).toBe('security-csv');
      });
    } finally {
      success.unmount();
    }

    const failure = await renderSvelte(HarnessPath, {
      mutations: createMutations({
        async exportOrgSecurityFindingsCsv() {
          throw new Error('Failed to export security findings.');
        },
      }),
    });

    try {
      click(queryRequiredButton(failure.target, '#security-export'));

      await waitFor(() => {
        failure.flush();
        expect(failure.target.textContent).toContain(
          'Failed to export security findings.'
        );
      });
    } finally {
      failure.unmount();
    }
  });

  test('loads package findings and surfaces load errors', async () => {
    const success = await renderSvelte(HarnessPath, {
      mutations: createMutations({
        async listSecurityFindings() {
          return [makeFinding('finding-1')];
        },
      }),
    });

    try {
      click(queryRequiredButton(success.target, '#security-findings-toggle'));

      await waitFor(() => {
        success.flush();
        expect(success.target.textContent).toContain('Finding finding-1');
        expect(
          queryRequiredText(success.target, '[data-test="security-state-loading"]').textContent
        ).toBe('idle');
      });
    } finally {
      success.unmount();
    }

    const failure = await renderSvelte(HarnessPath, {
      mutations: createMutations({
        async listSecurityFindings() {
          throw new Error('Failed to load package findings.');
        },
      }),
    });

    try {
      click(queryRequiredButton(failure.target, '#security-findings-toggle'));

      await waitFor(() => {
        failure.flush();
        expect(
          queryRequiredText(failure.target, '[data-test="security-load-error"]').textContent
        ).toBe('Failed to load package findings.');
      });
    } finally {
      failure.unmount();
    }
  });

  test('updates security finding resolution and refreshes the security overview on success', async () => {
    const updateCalls: Array<{ isResolved: boolean; note?: string }> = [];
    const { target, unmount, flush } = await renderSvelte(HarnessPath, {
      mutations: createMutations({
        async listSecurityFindings() {
          return [makeFinding('finding-1')];
        },
        async updateSecurityFinding(_ecosystem, _name, _findingId, input) {
          updateCalls.push({ ...input });
          return makeFinding('finding-1', {
            is_resolved: input.isResolved,
          });
        },
      }),
    });

    try {
      click(queryRequiredButton(target, '#security-findings-toggle'));

      await waitFor(() => {
        flush();
        expect(target.textContent).toContain('Finding finding-1');
      });

      changeValue(
        queryRequiredTextArea(target, '#finding-note-finding-1'),
        'Triaged by security team'
      );
      click(queryRequiredButton(target, '#finding-toggle-finding-1'));

      await waitFor(() => {
        flush();
        expect(
          queryRequiredText(target, '[data-test="security-state-notice"]').textContent
        ).toBe('Finding marked as resolved.');
        expect(target.textContent).toContain('resolved');
        expect(
          queryRequiredText(target, '[data-test="security-overview-reloads"]').textContent
        ).toBe('1');
      });

      expect(queryRequiredTextArea(target, '#finding-note-finding-1').value).toBe('');
      expect(updateCalls).toEqual([
        {
          isResolved: true,
          note: 'Triaged by security team',
        },
      ]);
    } finally {
      unmount();
    }
  });

  test('surfaces security finding resolution validation and request failures', async () => {
    const validation = await renderSvelte(HarnessPath, {
      mutations: createMutations({
        async listSecurityFindings() {
          return [makeFinding('finding-1')];
        },
      }),
    });

    try {
      click(queryRequiredButton(validation.target, '#security-findings-toggle'));

      await waitFor(() => {
        validation.flush();
        expect(validation.target.textContent).toContain('Finding finding-1');
      });

      changeValue(
        queryRequiredTextArea(validation.target, '#finding-note-finding-1'),
        'a'.repeat(2001)
      );
      click(queryRequiredButton(validation.target, '#finding-toggle-finding-1'));

      await waitFor(() => {
        validation.flush();
        expect(
          queryRequiredText(
            validation.target,
            '[data-test="security-state-error"]'
          ).textContent
        ).toBe('Security finding note must be 2000 characters or fewer.');
      });
    } finally {
      validation.unmount();
    }

    const failure = await renderSvelte(HarnessPath, {
      mutations: createMutations({
        async listSecurityFindings() {
          return [makeFinding('finding-1')];
        },
        async updateSecurityFinding() {
          throw new Error('Failed to update the security finding.');
        },
      }),
    });

    try {
      click(queryRequiredButton(failure.target, '#security-findings-toggle'));

      await waitFor(() => {
        failure.flush();
        expect(failure.target.textContent).toContain('Finding finding-1');
      });

      click(queryRequiredButton(failure.target, '#finding-toggle-finding-1'));

      await waitFor(() => {
        failure.flush();
        expect(
          queryRequiredText(
            failure.target,
            '[data-test="security-state-error"]'
          ).textContent
        ).toBe('Failed to update the security finding.');
      });
    } finally {
      failure.unmount();
    }
  });
});

function createMutations(
  overrides: Partial<OrgObservabilityMutations> = {}
): OrgObservabilityMutations {
  return {
    async exportOrgAuditLogsCsv() {
      return 'default-audit-csv';
    },
    async exportOrgSecurityFindingsCsv() {
      return 'default-security-csv';
    },
    async listSecurityFindings() {
      return [makeFinding('finding-1')];
    },
    async updateSecurityFinding(_ecosystem, _name, findingId, input) {
      return makeFinding(findingId, {
        is_resolved: input.isResolved,
      });
    },
    ...overrides,
  };
}

function makeFinding(
  id: string,
  overrides: Partial<SecurityFinding> = {}
): SecurityFinding {
  return {
    id,
    kind: 'vulnerability',
    severity: 'high',
    title: `Finding ${id}`,
    is_resolved: false,
    detected_at: '2026-04-24T00:00:00Z',
    ...overrides,
  };
}

function queryRequiredForm(target: ParentNode, selector: string): HTMLFormElement {
  const form = target.querySelector(selector);
  expect(form).not.toBeNull();
  return form as HTMLFormElement;
}

function queryRequiredInput(target: ParentNode, selector: string): HTMLInputElement {
  const input = target.querySelector(selector);
  expect(input).not.toBeNull();
  return input as HTMLInputElement;
}

function queryRequiredSelect(target: ParentNode, selector: string): HTMLSelectElement {
  const select = target.querySelector(selector);
  expect(select).not.toBeNull();
  return select as HTMLSelectElement;
}

function queryRequiredButton(target: ParentNode, selector: string): HTMLButtonElement {
  const button = target.querySelector(selector);
  expect(button).not.toBeNull();
  return button as HTMLButtonElement;
}

function queryRequiredTextArea(
  target: ParentNode,
  selector: string
): HTMLTextAreaElement {
  const textarea = target.querySelector(selector);
  expect(textarea).not.toBeNull();
  return textarea as HTMLTextAreaElement;
}

function queryRequiredText(target: ParentNode, selector: string): HTMLElement {
  const element = target.querySelector(selector);
  expect(element).not.toBeNull();
  return element as HTMLElement;
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
