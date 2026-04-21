/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import TeamAccessGrantForm from '../src/lib/components/TeamAccessGrantForm.svelte';
import { renderPackageSelectionValue } from '../src/pages/org-workspace-actions';
import {
  changeValue,
  renderSvelte,
  setChecked,
  submitForm,
} from './svelte-dom';

const PERMISSION_OPTIONS = [
  {
    value: 'publish',
    label: 'Publish',
    description: 'Create releases and publish artifacts.',
  },
  {
    value: 'read_private',
    label: 'Read private',
    description: 'Read non-public package data.',
  },
] as const;

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

describe('team access grant form', () => {
  test('submits selected repository grants from the rendered form', async () => {
    const submissions: Record<string, string[]>[] = [];
    const { target, unmount } = await renderSvelte(TeamAccessGrantForm, {
      fieldId: 'team-repository-core',
      selectLabel: 'Organization repository',
      selectName: 'repository_slug',
      placeholderLabel: 'Select a repository',
      emptyMessage: 'Create a repository before delegating repository-wide access.',
      submitLabel: 'Save repository access',
      permissionOptions: PERMISSION_OPTIONS,
      options: [
        {
          value: 'release-packages',
          label: 'Release Packages · Release · Private',
        },
      ],
      handleSubmit(event: SubmitEvent) {
        event.preventDefault();
        submissions.push(
          collectFormValues(event.currentTarget as HTMLFormElement)
        );
      },
    });

    const select = target.querySelector('select');
    const publish = target.querySelector('input[value="publish"]');
    const readPrivate = target.querySelector('input[value="read_private"]');
    const form = target.querySelector('form');

    if (
      !(select instanceof HTMLSelectElement) ||
      !(publish instanceof HTMLInputElement) ||
      !(readPrivate instanceof HTMLInputElement) ||
      !(form instanceof HTMLFormElement)
    ) {
      throw new Error('Failed to render repository grant form controls.');
    }

    changeValue(select, 'release-packages');
    setChecked(publish, true);
    setChecked(readPrivate, true);
    submitForm(form);

    expect(submissions).toEqual([
      {
        repository_slug: ['release-packages'],
        permissions: ['publish', 'read_private'],
      },
    ]);

    unmount();
  });

  test('submits selected package grants from the rendered form', async () => {
    const submissions: Record<string, string[]>[] = [];
    const packageKey = renderPackageSelectionValue('npm', '@scope/widget');
    const { target, unmount } = await renderSvelte(TeamAccessGrantForm, {
      fieldId: 'team-package-core',
      selectLabel: 'Organization package',
      selectName: 'package_key',
      placeholderLabel: 'Select a package',
      emptyMessage: 'Create or transfer a package before delegating access.',
      submitLabel: 'Save package access',
      permissionOptions: PERMISSION_OPTIONS,
      options: [
        {
          value: packageKey,
          label: 'npm · @scope/widget',
        },
      ],
      handleSubmit(event: SubmitEvent) {
        event.preventDefault();
        submissions.push(
          collectFormValues(event.currentTarget as HTMLFormElement)
        );
      },
    });

    const select = target.querySelector('select');
    const publish = target.querySelector('input[value="publish"]');
    const form = target.querySelector('form');

    if (
      !(select instanceof HTMLSelectElement) ||
      !(publish instanceof HTMLInputElement) ||
      !(form instanceof HTMLFormElement)
    ) {
      throw new Error('Failed to render package grant form controls.');
    }

    changeValue(select, packageKey);
    setChecked(publish, true);
    submitForm(form);

    expect(submissions).toEqual([
      {
        package_key: [packageKey],
        permissions: ['publish'],
      },
    ]);

    unmount();
  });

  test('submits selected namespace grants from the rendered form', async () => {
    const submissions: Record<string, string[]>[] = [];
    const { target, unmount } = await renderSvelte(TeamAccessGrantForm, {
      fieldId: 'team-namespace-core',
      selectLabel: 'Organization namespace claim',
      selectName: 'claim_id',
      placeholderLabel: 'Select a namespace claim',
      emptyMessage: 'Create or transfer a namespace claim before delegating access.',
      submitLabel: 'Save namespace access',
      permissionOptions: PERMISSION_OPTIONS,
      options: [
        {
          value: '123e4567-e89b-42d3-a456-426614174000',
          label: 'npm · @scope',
        },
      ],
      handleSubmit(event: SubmitEvent) {
        event.preventDefault();
        submissions.push(
          collectFormValues(event.currentTarget as HTMLFormElement)
        );
      },
    });

    const select = target.querySelector('select');
    const publish = target.querySelector('input[value="publish"]');
    const form = target.querySelector('form');

    if (
      !(select instanceof HTMLSelectElement) ||
      !(publish instanceof HTMLInputElement) ||
      !(form instanceof HTMLFormElement)
    ) {
      throw new Error('Failed to render namespace grant form controls.');
    }

    changeValue(select, '123e4567-e89b-42d3-a456-426614174000');
    setChecked(publish, true);
    submitForm(form);

    expect(submissions).toEqual([
      {
        claim_id: ['123e4567-e89b-42d3-a456-426614174000'],
        permissions: ['publish'],
      },
    ]);

    unmount();
  });

  test('shows the empty state and disables submission when no targets are available', async () => {
    const { target, unmount } = await renderSvelte(TeamAccessGrantForm, {
      fieldId: 'team-package-empty',
      selectLabel: 'Organization package',
      selectName: 'package_key',
      placeholderLabel: 'Select a package',
      emptyMessage: 'Create or transfer a package before delegating access.',
      submitLabel: 'Save package access',
      permissionOptions: PERMISSION_OPTIONS,
      options: [],
    });

    const button = target.querySelector('button[type="submit"]');
    const checkbox = target.querySelector('input[type="checkbox"]');

    expect(target.textContent).toContain(
      'Create or transfer a package before delegating access.'
    );
    expect(target.querySelector('select')).toBeNull();
    expect(button).toBeTruthy();
    expect(checkbox).toBeTruthy();
    expect((button as HTMLButtonElement).disabled).toBe(true);
    expect((checkbox as HTMLInputElement).disabled).toBe(true);

    unmount();
  });
});
