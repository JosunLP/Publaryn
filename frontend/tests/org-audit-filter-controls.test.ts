/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import OrgAuditFilterControls from '../src/lib/components/OrgAuditFilterControls.svelte';
import {
  changeValue,
  click,
  renderSvelte,
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

describe('org audit filter controls', () => {
  test('submits rendered audit filter values from the form', async () => {
    const submissions: Record<string, string[]>[] = [];
    const { target, unmount } = await renderSvelte(OrgAuditFilterControls, {
      actionOptions: [
        { value: 'team_create', label: 'Team create' },
        { value: 'team_update', label: 'Team update' },
      ],
      actionValue: '',
      actorInput: '',
      actorOptions: [
        {
          userId: '11111111-1111-4111-8111-111111111111',
          username: 'alex',
          label: 'Alex Example (@alex)',
        },
      ],
      summary: 'Showing page 1 with up to 20 events',
      handleSubmit(event: SubmitEvent) {
        event.preventDefault();
        submissions.push(
          collectFormValues(event.currentTarget as HTMLFormElement)
        );
      },
    });

    const action = target.querySelector('select[name="action"]');
    const actor = target.querySelector('input[name="actor_query"]');
    const occurredFrom = target.querySelector('input[name="occurred_from"]');
    const occurredUntil = target.querySelector('input[name="occurred_until"]');
    const form = target.querySelector('form');

    if (
      !(action instanceof HTMLSelectElement) ||
      !(actor instanceof HTMLInputElement) ||
      !(occurredFrom instanceof HTMLInputElement) ||
      !(occurredUntil instanceof HTMLInputElement) ||
      !(form instanceof HTMLFormElement)
    ) {
      throw new Error('Failed to render audit filter form controls.');
    }

    changeValue(action, 'team_update');
    changeValue(actor, 'alex');
    changeValue(occurredFrom, '2026-04-01');
    changeValue(occurredUntil, '2026-04-10');
    submitForm(form);

    expect(submissions).toEqual([
      {
        action: ['team_update'],
        actor_query: ['alex'],
        occurred_from: ['2026-04-01'],
        occurred_until: ['2026-04-10'],
      },
    ]);

    unmount();
  });

  test('invokes export and clear handlers from rendered buttons', async () => {
    const calls: string[] = [];
    const { target, unmount } = await renderSvelte(OrgAuditFilterControls, {
      actionOptions: [{ value: 'team_update', label: 'Team update' }],
      actionValue: 'team_update',
      actorInput: 'alex',
      actorOptions: [],
      occurredFrom: '2026-04-01',
      summary: 'Showing filtered events',
      showActionClear: true,
      showActorClear: true,
      showDateClear: true,
      handleExport() {
        calls.push('export');
      },
      clearAction() {
        calls.push('clear-action');
      },
      clearActor() {
        calls.push('clear-actor');
      },
      clearDates() {
        calls.push('clear-dates');
      },
    });

    const exportButton = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Export CSV'
    );
    const clearActionButton = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Clear action'
    );
    const clearActorButton = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Clear actor'
    );
    const clearDatesButton = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Clear dates'
    );

    if (
      !(exportButton instanceof HTMLButtonElement) ||
      !(clearActionButton instanceof HTMLButtonElement) ||
      !(clearActorButton instanceof HTMLButtonElement) ||
      !(clearDatesButton instanceof HTMLButtonElement)
    ) {
      throw new Error('Failed to render audit action buttons.');
    }

    click(exportButton);
    click(clearActionButton);
    click(clearActorButton);
    click(clearDatesButton);

    expect(calls).toEqual([
      'export',
      'clear-action',
      'clear-actor',
      'clear-dates',
    ]);

    unmount();
  });

  test('shows exporting state and hides clear buttons when filters are inactive', async () => {
    const { target, unmount } = await renderSvelte(OrgAuditFilterControls, {
      actionOptions: [],
      actorInput: '',
      actorOptions: [],
      exporting: true,
      summary: 'Showing page 1 with up to 20 events',
    });

    const exportButton = Array.from(target.querySelectorAll('button')).find(
      (button) => button.textContent?.trim() === 'Exporting…'
    );

    expect(exportButton).toBeTruthy();
    expect((exportButton as HTMLButtonElement).disabled).toBe(true);
    expect(target.textContent).not.toContain('Clear action');
    expect(target.textContent).not.toContain('Clear actor');
    expect(target.textContent).not.toContain('Clear dates');

    unmount();
  });
});
