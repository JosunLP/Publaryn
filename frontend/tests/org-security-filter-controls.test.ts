/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import OrgSecurityFilterControls from '../src/lib/components/OrgSecurityFilterControls.svelte';
import {
  changeValue,
  click,
  renderSvelte,
  setChecked,
  submitForm,
} from './svelte-dom';

function collectFormValues(form: HTMLFormElement): Record<string, string[]> {
  const formData = new FormData(form);
  const values = new Map<string, string[]>();

  for (const [key, value] of formData.entries()) {
    const entries = values.get(key) || [];
    entries.push(value.toString());
    values.set(key, entries);
  }

  return Object.fromEntries(values);
}

describe('org security filter controls', () => {
  test('submits rendered security filter values from the form', async () => {
    const submissions: Record<string, string[]>[] = [];
    const { target, unmount } = await renderSvelte(OrgSecurityFilterControls, {
      severityOptions: [
        { value: 'critical', label: 'Critical' },
        { value: 'high', label: 'High' },
      ],
      selectedSeverities: [],
      ecosystemOptions: [
        { value: 'npm', label: 'npm / Bun' },
        { value: 'cargo', label: 'Cargo' },
      ],
      packageOptions: [{ value: '@scope/widget', label: 'npm · @scope/widget' }],
      summary: 'Showing unresolved findings',
      handleSubmit(event: SubmitEvent) {
        event.preventDefault();
        submissions.push(
          collectFormValues(event.currentTarget as HTMLFormElement)
        );
      },
    });

    const critical = target.querySelector('input[value="critical"]');
    const high = target.querySelector('input[value="high"]');
    const ecosystem = target.querySelector('select[name="security_ecosystem"]');
    const packageInput = target.querySelector('input[name="security_package"]');
    const form = target.querySelector('form');

    if (
      !(critical instanceof HTMLInputElement) ||
      !(high instanceof HTMLInputElement) ||
      !(ecosystem instanceof HTMLSelectElement) ||
      !(packageInput instanceof HTMLInputElement) ||
      !(form instanceof HTMLFormElement)
    ) {
      throw new Error('Failed to render security filter controls.');
    }

    setChecked(critical, true);
    setChecked(high, true);
    changeValue(ecosystem, 'cargo');
    changeValue(packageInput, '@scope/widget');
    submitForm(form);

    expect(submissions).toEqual([
      {
        security_severity: ['critical', 'high'],
        security_ecosystem: ['cargo'],
        security_package: ['@scope/widget'],
      },
    ]);

    unmount();
  });

  test('invokes export and clear handlers from rendered buttons', async () => {
    const calls: string[] = [];
    const { target, unmount } = await renderSvelte(OrgSecurityFilterControls, {
      severityOptions: [{ value: 'critical', label: 'Critical' }],
      selectedSeverities: ['critical'],
      ecosystemOptions: [{ value: 'npm', label: 'npm / Bun' }],
      ecosystemValue: 'npm',
      packageValue: '@scope/widget',
      packageOptions: [{ value: '@scope/widget', label: 'npm · @scope/widget' }],
      summary: 'Showing filtered unresolved findings',
      showSeverityClear: true,
      showEcosystemClear: true,
      showPackageClear: true,
      handleExport() {
        calls.push('export');
      },
      clearSeverity() {
        calls.push('clear-severity');
      },
      clearEcosystem() {
        calls.push('clear-ecosystem');
      },
      clearPackage() {
        calls.push('clear-package');
      },
    });

    const exportButton = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Export CSV'
    );
    const clearSeverityButton = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Clear severity'
    );
    const clearEcosystemButton = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Clear ecosystem'
    );
    const clearPackageButton = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Clear package'
    );

    if (
      !(exportButton instanceof HTMLButtonElement) ||
      !(clearSeverityButton instanceof HTMLButtonElement) ||
      !(clearEcosystemButton instanceof HTMLButtonElement) ||
      !(clearPackageButton instanceof HTMLButtonElement)
    ) {
      throw new Error('Failed to render security action buttons.');
    }

    click(exportButton);
    click(clearSeverityButton);
    click(clearEcosystemButton);
    click(clearPackageButton);

    expect(calls).toEqual([
      'export',
      'clear-severity',
      'clear-ecosystem',
      'clear-package',
    ]);

    unmount();
  });

  test('shows exporting state and hides clear buttons when filters are inactive', async () => {
    const { target, unmount } = await renderSvelte(OrgSecurityFilterControls, {
      severityOptions: [],
      selectedSeverities: [],
      ecosystemOptions: [],
      packageOptions: [],
      exporting: true,
      summary: 'Showing unresolved findings',
    });

    const exportButton = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Exporting…'
    );

    expect(exportButton).toBeTruthy();
    expect((exportButton as HTMLButtonElement).disabled).toBe(true);
    expect(target.textContent).not.toContain('Clear severity');
    expect(target.textContent).not.toContain('Clear ecosystem');
    expect(target.textContent).not.toContain('Clear package');

    unmount();
  });
});
