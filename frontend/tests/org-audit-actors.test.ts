import { describe, expect, test } from 'bun:test';

import {
  buildAuditActorOptions,
  buildRemoteAuditActorOptions,
  nextAuditActorInputState,
} from '../src/pages/org-audit-actors';

describe('org audit actor helpers', () => {
  test('builds sorted actor options from valid members only', () => {
    expect(
      buildRemoteAuditActorOptions([
        {
          user_id: '22222222-2222-4222-8222-222222222222',
          username: 'zulu',
          display_name: 'Zulu Zebra',
        },
        {
          user_id: '11111111-1111-4111-8111-111111111111',
          username: 'alpha',
        },
        {
          user_id: '33333333-3333-4333-8333-333333333333',
          username: '   ',
          display_name: 'Ignored',
        },
      ])
    ).toEqual([
      {
        userId: '11111111-1111-4111-8111-111111111111',
        username: 'alpha',
        label: '@alpha',
      },
      {
        userId: '22222222-2222-4222-8222-222222222222',
        username: 'zulu',
        label: 'Zulu Zebra (@zulu)',
      },
    ]);
  });

  test('merges base and remote actor options while keeping the first entry per user id', () => {
    const members = [
      {
        user_id: '11111111-1111-4111-8111-111111111111',
        username: 'alpha',
        display_name: 'Alpha Admin',
      },
      {
        user_id: '22222222-2222-4222-8222-222222222222',
        username: 'bravo',
      },
    ];

    const remoteOptions = [
      {
        userId: '22222222-2222-4222-8222-222222222222',
        username: 'bravo',
        label: 'Remote Bravo (@bravo)',
      },
      {
        userId: '33333333-3333-4333-8333-333333333333',
        username: 'charlie',
        label: 'Charlie Check (@charlie)',
      },
    ];

    expect(buildAuditActorOptions(members, remoteOptions)).toEqual([
      {
        userId: '11111111-1111-4111-8111-111111111111',
        username: 'alpha',
        label: 'Alpha Admin (@alpha)',
      },
      {
        userId: '22222222-2222-4222-8222-222222222222',
        username: 'bravo',
        label: '@bravo',
      },
      {
        userId: '33333333-3333-4333-8333-333333333333',
        username: 'charlie',
        label: 'Charlie Check (@charlie)',
      },
    ]);
  });

  test('updates the input state when the selected audit actor changes', () => {
    expect(
      nextAuditActorInputState(
        '',
        '',
        '11111111-1111-4111-8111-111111111111',
        'alpha'
      )
    ).toEqual({
      syncKey: '11111111-1111-4111-8111-111111111111|alpha',
      input: 'alpha',
    });

    expect(
      nextAuditActorInputState(
        '',
        '',
        '11111111-1111-4111-8111-111111111111',
        ''
      )
    ).toEqual({
      syncKey: '11111111-1111-4111-8111-111111111111|',
      input: '11111111-1111-4111-8111-111111111111',
    });
  });

  test('preserves a locally edited input while the selected audit actor stays the same', () => {
    expect(
      nextAuditActorInputState(
        '11111111-1111-4111-8111-111111111111|alpha',
        'alp',
        '11111111-1111-4111-8111-111111111111',
        'alpha'
      )
    ).toEqual({
      syncKey: '11111111-1111-4111-8111-111111111111|alpha',
      input: 'alp',
    });
  });
});
