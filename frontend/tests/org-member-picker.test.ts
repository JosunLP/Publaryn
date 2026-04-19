import { describe, expect, test } from 'bun:test';

import {
  buildOrgMemberPickerOptions,
  resolveOrgMemberPickerInput,
} from '../src/pages/org-member-picker';

describe('org member picker helpers', () => {
  test('builds sorted unique options from valid members only', () => {
    expect(
      buildOrgMemberPickerOptions([
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
          user_id: '11111111-1111-4111-8111-111111111111',
          username: 'alpha',
          display_name: 'Alpha Admin',
        },
        {
          user_id: '33333333-3333-4333-8333-333333333333',
          username: '   ',
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

  test('excludes usernames that are already in the target team', () => {
    expect(
      buildOrgMemberPickerOptions(
        [
          {
            user_id: '11111111-1111-4111-8111-111111111111',
            username: 'alpha',
          },
          {
            user_id: '22222222-2222-4222-8222-222222222222',
            username: 'bravo',
          },
        ],
        ['bravo']
      )
    ).toEqual([
      {
        userId: '11111111-1111-4111-8111-111111111111',
        username: 'alpha',
        label: '@alpha',
      },
    ]);
  });

  test('resolves pasted user ids back to usernames for API submissions', () => {
    const options = buildOrgMemberPickerOptions([
      {
        user_id: '11111111-1111-4111-8111-111111111111',
        username: 'alpha',
      },
    ]);

    expect(
      resolveOrgMemberPickerInput(
        '11111111-1111-4111-8111-111111111111',
        options
      )
    ).toBe('alpha');
    expect(resolveOrgMemberPickerInput('alpha', options)).toBe('alpha');
    expect(resolveOrgMemberPickerInput('  unknown-user  ', options)).toBe(
      'unknown-user'
    );
  });
});
