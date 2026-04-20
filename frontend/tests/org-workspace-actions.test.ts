/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import type { OrgAuditActorOption } from '../src/pages/org-audit-actors';
import {
  buildAuditExportQuery,
  buildSecurityExportQuery,
  decodePackageSelection,
  renderPackageSelectionValue,
  resolveAuditFilterSubmission,
  resolveSecurityFilterSubmission,
  resolveTeamPackageAccessSubmission,
  resolveTeamRepositoryAccessSubmission,
} from '../src/pages/org-workspace-actions';

const AUDIT_ACTORS: OrgAuditActorOption[] = [
  {
    userId: '123e4567-e89b-42d3-a456-426614174000',
    username: 'alice',
    label: 'Alice Example (@alice)',
  },
];

function buildFormData(
  values: Record<string, string | string[] | undefined>
): FormData {
  const formData = new FormData();

  for (const [key, value] of Object.entries(values)) {
    if (Array.isArray(value)) {
      for (const entry of value) {
        formData.append(key, entry);
      }
      continue;
    }

    if (typeof value === 'string') {
      formData.append(key, value);
    }
  }

  return formData;
}

describe('org workspace action helpers', () => {
  test('resolves audit form submissions using selected actor usernames', () => {
    const result = resolveAuditFilterSubmission(
      buildFormData({
        action: ' team_member_add ',
        actor_query: 'ALICE',
        occurred_from: '2026-04-01',
        occurred_until: '2026-04-30',
      }),
      AUDIT_ACTORS
    );

    expect(result).toEqual({
      ok: true,
      value: {
        action: 'team_member_add',
        actorUserId: '123e4567-e89b-42d3-a456-426614174000',
        actorUsername: 'alice',
        occurredFrom: '2026-04-01',
        occurredUntil: '2026-04-30',
        page: 1,
      },
    });
  });

  test('rejects audit form submissions with inverted date ranges', () => {
    expect(
      resolveAuditFilterSubmission(
        buildFormData({
          occurred_from: '2026-04-30',
          occurred_until: '2026-04-01',
        }),
        AUDIT_ACTORS
      )
    ).toEqual({
      ok: false,
      error: 'End date must be on or after the start date.',
    });
  });

  test('builds audit export queries without empty filters', () => {
    expect(
      buildAuditExportQuery({
        action: '',
        actorUserId: '123e4567-e89b-42d3-a456-426614174000',
        occurredFrom: '',
        occurredUntil: '2026-04-30',
      })
    ).toEqual({
      action: undefined,
      actorUserId: '123e4567-e89b-42d3-a456-426614174000',
      occurredFrom: undefined,
      occurredUntil: '2026-04-30',
    });
  });

  test('normalizes security filter submissions from multi-select form data', () => {
    expect(
      resolveSecurityFilterSubmission(
        buildFormData({
          security_severity: [' critical ', 'high'],
          security_ecosystem: ' bun ',
          security_package: ' demo-widget ',
        })
      )
    ).toEqual({
      severities: ['critical', 'high'],
      ecosystem: 'bun',
      packageQuery: 'demo-widget',
    });
  });

  test('builds security export queries without empty filters', () => {
    expect(
      buildSecurityExportQuery({
        severities: [],
        ecosystem: 'npm',
        packageQuery: '',
      })
    ).toEqual({
      severities: undefined,
      ecosystem: 'npm',
      package: undefined,
    });
  });

  test('round-trips package selection values for delegated package access', () => {
    const encoded = renderPackageSelectionValue('npm', '@scope/demo widget');

    expect(encoded).toBe('npm:%40scope%2Fdemo%20widget');
    expect(decodePackageSelection(encoded)).toEqual({
      ecosystem: 'npm',
      name: '@scope/demo widget',
    });
  });

  test('validates delegated package access submissions', () => {
    expect(
      resolveTeamPackageAccessSubmission(
        buildFormData({
          package_key: renderPackageSelectionValue('cargo', 'demo-crate'),
          permissions: [' publish ', 'admin'],
        })
      )
    ).toEqual({
      ok: true,
      value: {
        ecosystem: 'cargo',
        name: 'demo-crate',
        permissions: ['publish', 'admin'],
      },
    });

    expect(
      resolveTeamPackageAccessSubmission(
        buildFormData({
          package_key: '',
          permissions: ['publish'],
        })
      )
    ).toEqual({
      ok: false,
      error: 'Select a package to manage access.',
    });

    expect(
      resolveTeamPackageAccessSubmission(
        buildFormData({
          package_key: renderPackageSelectionValue('npm', 'demo-widget'),
        })
      )
    ).toEqual({
      ok: false,
      error: 'Select at least one delegated package permission.',
    });
  });

  test('validates delegated repository access submissions', () => {
    expect(
      resolveTeamRepositoryAccessSubmission(
        buildFormData({
          repository_slug: ' release-packages ',
          permissions: [' read_private ', 'publish'],
        })
      )
    ).toEqual({
      ok: true,
      value: {
        repositorySlug: 'release-packages',
        permissions: ['read_private', 'publish'],
      },
    });

    expect(
      resolveTeamRepositoryAccessSubmission(
        buildFormData({
          permissions: ['admin'],
        })
      )
    ).toEqual({
      ok: false,
      error: 'Select a repository to manage access.',
    });

    expect(
      resolveTeamRepositoryAccessSubmission(
        buildFormData({
          repository_slug: 'release-packages',
        })
      )
    ).toEqual({
      ok: false,
      error: 'Select at least one delegated repository permission.',
    });
  });
});
