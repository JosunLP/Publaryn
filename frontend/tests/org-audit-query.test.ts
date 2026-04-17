import { describe, expect, test } from 'bun:test';

import {
  buildOrgAuditExportFilename,
  buildOrgAuditPath,
  formatAuditActorQueryLabel,
  getAuditViewFromQuery,
} from '../src/pages/org-audit-query';

const ACTOR_USER_ID = '123e4567-e89b-42d3-a456-426614174000';

describe('org audit query helpers', () => {
  test('parses valid action, actor, and page filters from the query string', () => {
    const view = getAuditViewFromQuery(
      new URLSearchParams(
        `action=team_update&actor_user_id=${ACTOR_USER_ID}&actor_username=alice&occurred_from=2024-01-10&occurred_until=2024-01-20&page=3`
      )
    );

    expect(view).toEqual({
      action: 'team_update',
      actorUserId: ACTOR_USER_ID,
      actorUsername: 'alice',
      occurredFrom: '2024-01-10',
      occurredUntil: '2024-01-20',
      page: 3,
    });
  });

  test('drops invalid action, actor, and page filters back to safe defaults', () => {
    const view = getAuditViewFromQuery(
      new URLSearchParams(
        'action=definitely_not_real&actor_user_id=not-a-uuid&actor_username=alice&page=0'
      )
    );

    expect(view).toEqual({
      action: '',
      actorUserId: '',
      actorUsername: '',
      occurredFrom: '',
      occurredUntil: '',
      page: 1,
    });
  });

  test('builds org audit paths while preserving unrelated query params', () => {
    const path = buildOrgAuditPath(
      'acme-corp',
      {
        action: 'org_member_add',
        actorUserId: ACTOR_USER_ID,
        actorUsername: 'alice',
        occurredFrom: '2024-01-01',
        occurredUntil: '2024-01-31',
        page: 2,
      },
      '?tab=activity'
    );

    const url = new URL(path, 'https://example.test');

    expect(url.pathname).toBe('/orgs/acme-corp');
    expect(url.searchParams.get('tab')).toBe('activity');
    expect(url.searchParams.get('action')).toBe('org_member_add');
    expect(url.searchParams.get('actor_user_id')).toBe(ACTOR_USER_ID);
    expect(url.searchParams.get('actor_username')).toBe('alice');
    expect(url.searchParams.get('occurred_from')).toBe('2024-01-01');
    expect(url.searchParams.get('occurred_until')).toBe('2024-01-31');
    expect(url.searchParams.get('page')).toBe('2');
  });

  test('clears actor and action filters while keeping unrelated params', () => {
    const path = buildOrgAuditPath(
      'acme-corp',
      {
        action: '',
        actorUserId: '',
        actorUsername: '',
        occurredFrom: '',
        occurredUntil: '',
        page: 1,
      },
      `?tab=activity&action=org_update&actor_user_id=${ACTOR_USER_ID}&actor_username=alice&occurred_from=2024-01-01&occurred_until=2024-01-31&page=4`
    );

    const url = new URL(path, 'https://example.test');

    expect(url.pathname).toBe('/orgs/acme-corp');
    expect(url.searchParams.get('tab')).toBe('activity');
    expect(url.searchParams.get('action')).toBeNull();
    expect(url.searchParams.get('actor_user_id')).toBeNull();
    expect(url.searchParams.get('actor_username')).toBeNull();
    expect(url.searchParams.get('occurred_from')).toBeNull();
    expect(url.searchParams.get('occurred_until')).toBeNull();
    expect(url.searchParams.get('page')).toBeNull();
  });

  test('formats actor labels for summaries and filter controls', () => {
    expect(formatAuditActorQueryLabel('alice')).toBe('@alice');
    expect(formatAuditActorQueryLabel('   ')).toBe('the selected actor');
  });

  test('keeps namespace claim audit actions when building and parsing filters', () => {
    const path = buildOrgAuditPath(
      'acme-corp',
      {
        action: 'namespace_claim_create',
        actorUserId: '',
        actorUsername: '',
        page: 1,
      },
      '?tab=activity'
    );

    const url = new URL(path, 'https://example.test');
    const view = getAuditViewFromQuery(url.searchParams);

    expect(url.searchParams.get('action')).toBe('namespace_claim_create');
    expect(view.action).toBe('namespace_claim_create');
  });

  test('drops invalid audit dates back to empty filters', () => {
    const view = getAuditViewFromQuery(
      new URLSearchParams(
        'occurred_from=2024-02-30&occurred_until=definitely-not-a-date'
      )
    );

    expect(view.occurredFrom).toBe('');
    expect(view.occurredUntil).toBe('');
  });

  test('builds stable CSV export filenames from the applied audit filters', () => {
    expect(
      buildOrgAuditExportFilename(
        'acme-corp',
        {
          action: 'org_member_add',
          actorUsername: 'Alice Example',
          occurredFrom: '2024-01-01',
          occurredUntil: '2024-01-31',
        },
        new Date('2026-04-17T09:15:00Z')
      )
    ).toBe(
      'org-audit-acme-corp--org_member_add--actor-alice-example--2024-01-01_to_2024-01-31--2026-04-17.csv'
    );
  });

  test('falls back to a minimal CSV export filename when optional filters are empty', () => {
    expect(
      buildOrgAuditExportFilename(
        'acme-corp',
        {
          action: '',
          actorUsername: '',
          occurredFrom: '',
          occurredUntil: '',
        },
        new Date('2026-04-17T09:15:00Z')
      )
    ).toBe('org-audit-acme-corp--2026-04-17.csv');
  });
});
