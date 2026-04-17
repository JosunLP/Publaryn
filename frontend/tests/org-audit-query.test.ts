import { describe, expect, test } from 'bun:test';

import {
  buildOrgAuditPath,
  formatAuditActorQueryLabel,
  getAuditViewFromQuery,
} from '../src/pages/org-audit-query';

const ACTOR_USER_ID = '123e4567-e89b-42d3-a456-426614174000';

describe('org audit query helpers', () => {
  test('parses valid action, actor, and page filters from the query string', () => {
    const view = getAuditViewFromQuery(
      new URLSearchParams(
        `action=team_update&actor_user_id=${ACTOR_USER_ID}&actor_username=alice&page=3`
      )
    );

    expect(view).toEqual({
      action: 'team_update',
      actorUserId: ACTOR_USER_ID,
      actorUsername: 'alice',
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
    expect(url.searchParams.get('page')).toBe('2');
  });

  test('clears actor and action filters while keeping unrelated params', () => {
    const path = buildOrgAuditPath(
      'acme-corp',
      {
        action: '',
        actorUserId: '',
        actorUsername: '',
        page: 1,
      },
      `?tab=activity&action=org_update&actor_user_id=${ACTOR_USER_ID}&actor_username=alice&page=4`
    );

    const url = new URL(path, 'https://example.test');

    expect(url.pathname).toBe('/orgs/acme-corp');
    expect(url.searchParams.get('tab')).toBe('activity');
    expect(url.searchParams.get('action')).toBeNull();
    expect(url.searchParams.get('actor_user_id')).toBeNull();
    expect(url.searchParams.get('actor_username')).toBeNull();
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
});
