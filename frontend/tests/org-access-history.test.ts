/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import type { OrgAccessHistoryEntry } from '../src/api/orgs';
import {
  accessHistorySummary,
  buildOrgAccessHistoryExportFilename,
  formatAccessHistoryActor,
  formatAccessHistoryEvent,
  formatAccessHistoryPermissionDelta,
  formatAccessHistoryScope,
  formatAccessHistoryTarget,
  formatAccessHistoryTeam,
} from '../src/pages/org-access-history';

describe('org access history helpers', () => {
  test('formats access history entries for delegated grant timelines', () => {
    const entry: OrgAccessHistoryEntry = {
      scope: 'package',
      event: 'updated',
      team_name: 'Release Engineering',
      target_label: 'npm · pkg-core',
      previous_permissions: ['write_metadata'],
      permissions: ['publish', 'admin'],
      actor_display_name: 'Admin User',
      actor_username: 'admin-user',
      summary:
        'Changed Release Engineering access for npm · pkg-core from write metadata to admin, publish.',
    };

    expect(formatAccessHistoryScope(entry.scope)).toBe('Package access');
    expect(formatAccessHistoryEvent(entry.event)).toBe('Updated');
    expect(formatAccessHistoryTarget(entry)).toBe('npm · pkg-core');
    expect(formatAccessHistoryTeam(entry)).toBe('Release Engineering');
    expect(formatAccessHistoryActor(entry)).toBe('Admin User (@admin-user)');
    expect(formatAccessHistoryPermissionDelta(entry)).toBe(
      'Write Metadata → Admin, Publish'
    );
    expect(accessHistorySummary(entry)).toBe(
      'Changed Release Engineering access for npm · pkg-core from write metadata to admin, publish.'
    );
  });

  test('falls back to target metadata and stable export filenames', () => {
    const entry: OrgAccessHistoryEntry = {
      scope: 'namespace',
      event: 'revoked',
      team_slug: 'security-reviewers',
      target: {
        ecosystem: 'npm',
        namespace: '@source-org',
      },
      previous_permissions: ['admin'],
      permissions: [],
      actor_username: 'auditor',
    };

    expect(formatAccessHistoryTarget(entry)).toBe('npm · @source-org');
    expect(formatAccessHistoryActor(entry)).toBe('@auditor');
    expect(accessHistorySummary(entry)).toBe(
      'Revoked delegated access from security-reviewers for npm · @source-org.'
    );
    expect(
      buildOrgAccessHistoryExportFilename(
        'source-org',
        new Date('2026-04-25T12:00:00Z')
      )
    ).toBe('org-access-history-source-org-2026-04-25.csv');
    expect(
      buildOrgAccessHistoryExportFilename(
        ' Source Org: Team/One ',
        new Date('2026-04-25T12:00:00Z')
      )
    ).toBe('org-access-history-source-org-team-one-2026-04-25.csv');
    expect(
      buildOrgAccessHistoryExportFilename(
        ' / ',
        new Date('2026-04-25T12:00:00Z')
      )
    ).toBe('org-access-history-organization-2026-04-25.csv');
  });

  test('normalizes event and scope identifiers consistently across labels and summaries', () => {
    const entry: OrgAccessHistoryEntry = {
      scope: 'Package',
      event: 'GRANTED',
      team_name: 'Publishers',
      target: {
        ecosystem: 'npm',
        normalized_name: 'demo-widget',
      },
      permissions: ['publish'],
    };

    expect(formatAccessHistoryScope(entry.scope)).toBe('Package access');
    expect(formatAccessHistoryEvent(entry.event)).toBe('Granted');
    expect(formatAccessHistoryTarget(entry)).toBe('npm · demo-widget');
    expect(accessHistorySummary(entry)).toBe(
      'Granted Publishers publish access to npm · demo-widget.'
    );
  });
});
