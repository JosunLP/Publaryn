/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import type { SecurityFinding } from '../src/api/packages';
import OrgSecurityFindingTriageControls from '../src/lib/components/OrgSecurityFindingTriageControls.svelte';
import { changeValue, click, renderSvelte } from './svelte-dom';

function makeFinding(
  id: string,
  overrides: Partial<SecurityFinding> = {}
): SecurityFinding {
  return {
    id,
    kind: 'vulnerability',
    severity: 'high',
    title: `Finding ${id}`,
    description: 'Investigate the vulnerable dependency.',
    is_resolved: false,
    detected_at: '2026-04-20T12:00:00Z',
    ...overrides,
  };
}

describe('org security finding triage controls', () => {
  test('updates note text and toggles unresolved findings from rendered controls', async () => {
    const noteUpdates: Array<{ findingId: string; value: string }> = [];
    const toggles: string[] = [];
    const finding = makeFinding('finding-1', {
      release_version: '1.2.3',
    });

    const { target, unmount } = await renderSvelte(
      OrgSecurityFindingTriageControls,
      {
        findings: [finding],
        notePlaceholder: 'Optional note (recorded in audit log)',
        formatDateValue: (value: string | null | undefined) => value || '',
        normalizeSeverity: (value: string) => value.toLowerCase(),
        formatKindLabel: (value: string) => value.toUpperCase(),
        handleNoteInput(findingId: string, value: string) {
          noteUpdates.push({ findingId, value });
        },
        handleToggleResolution(selectedFinding: SecurityFinding) {
          toggles.push(selectedFinding.id);
        },
      }
    );

    const textarea = target.querySelector('textarea');
    const button = Array.from(target.querySelectorAll('button')).find(
      (element) => element.textContent?.trim() === 'Mark resolved'
    );

    if (
      !(textarea instanceof HTMLTextAreaElement) ||
      !(button instanceof HTMLButtonElement)
    ) {
      throw new Error('Failed to render triage controls.');
    }

    changeValue(textarea, 'Needs verification from the reviewer');
    click(button);

    expect(noteUpdates).toEqual([
      {
        findingId: 'finding-1',
        value: 'Needs verification from the reviewer',
      },
    ]);
    expect(toggles).toEqual(['finding-1']);
    expect(target.textContent).toContain('VULNERABILITY');
    expect(target.textContent).toContain('1.2.3');

    unmount();
  });

  test('renders deep links back to package security when a builder is provided', async () => {
    const finding = makeFinding('finding-3', {
      advisory_id: 'PUB-2026-0007',
      title: 'Prototype pollution',
    });

    const { target, unmount } = await renderSvelte(
      OrgSecurityFindingTriageControls,
      {
        findings: [finding],
        notePlaceholder: 'Optional note (recorded in audit log)',
        formatDateValue: (value: string | null | undefined) => value || '',
        normalizeSeverity: (value: string) => value.toLowerCase(),
        buildPackageSecurityHref: () =>
          '/packages/npm/demo-widget?tab=security&security_search=PUB-2026-0007',
      }
    );

    const link = target.querySelector('a');
    if (!link || link.tagName !== 'A') {
      throw new Error('Expected a package security link.');
    }

    expect(link.textContent?.trim()).toBe('Open package security');
    expect(link.getAttribute('href')).toBe(
      '/packages/npm/demo-widget?tab=security&security_search=PUB-2026-0007'
    );

    unmount();
  });

  test('shows reopening state for an updating resolved finding', async () => {
    const finding = makeFinding('finding-2', {
      is_resolved: true,
      resolved_at: '2026-04-20T14:00:00Z',
    });

    const { target, unmount } = await renderSvelte(
      OrgSecurityFindingTriageControls,
      {
        findings: [finding],
        findingNotes: { 'finding-2': 'Already mitigated' },
        updatingFindingId: 'finding-2',
        notePlaceholder: 'Optional note (recorded in audit log)',
        formatDateValue: (value: string | null | undefined) => value || '',
        normalizeSeverity: (value: string) => value.toLowerCase(),
      }
    );

    const textarea = target.querySelector('textarea');
    const button = Array.from(target.querySelectorAll('button')).find(
      (element) => element.textContent?.trim() === 'Reopening…'
    );

    if (
      !(textarea instanceof HTMLTextAreaElement) ||
      !(button instanceof HTMLButtonElement)
    ) {
      throw new Error('Failed to render resolved triage state.');
    }

    expect(textarea.value).toBe('Already mitigated');
    expect(button.disabled).toBe(true);
    expect(target.textContent).toContain('Resolved');

    unmount();
  });

  test('disables other triage buttons while a finding update is in flight', async () => {
    const findings = [
      makeFinding('finding-1'),
      makeFinding('finding-2', { is_resolved: true }),
    ];

    const { target, unmount } = await renderSvelte(
      OrgSecurityFindingTriageControls,
      {
        findings,
        updatingFindingId: 'finding-2',
        notePlaceholder: 'Optional note (recorded in audit log)',
        formatDateValue: (value: string | null | undefined) => value || '',
        normalizeSeverity: (value: string) => value.toLowerCase(),
      }
    );

    const buttons = Array.from(target.querySelectorAll('button')).map((button) =>
      button.textContent?.trim()
    );

    expect(buttons).toEqual(['Mark resolved', 'Reopening…']);
    for (const button of target.querySelectorAll('button')) {
      expect((button as HTMLButtonElement).disabled).toBe(true);
    }

    unmount();
  });
});
