import { describe, expect, test } from 'bun:test';

import { buildReleaseDependencyOverview } from '../src/utils/release-metadata';

describe('release dependency overview helpers', () => {
  test('summarizes Composer runtime and development requirements', () => {
    expect(
      buildReleaseDependencyOverview({
        kind: 'composer',
        details: {
          require: {
            php: '^8.3',
            'psr/log': '^3.0',
          },
          'require-dev': {
            phpunit: '^11.0',
          },
        },
      })
    ).toEqual({
      ecosystem: 'composer',
      total: 3,
      groups: [
        {
          label: 'Runtime require',
          count: 2,
          names: ['php', 'psr/log'],
        },
        {
          label: 'Development require',
          count: 1,
          names: ['phpunit'],
        },
      ],
    });
  });

  test('summarizes NuGet target framework dependency groups', () => {
    expect(
      buildReleaseDependencyOverview({
        kind: 'nuget',
        details: {
          authors: 'Alice',
          dependency_groups: [
            {
              target_framework: 'net8.0',
              dependencies: [
                { id: 'Newtonsoft.Json', version_range: '[13.0.3, )' },
                { id: 'Serilog', version_range: '[3.1.1, )' },
              ],
            },
          ],
          is_listed: true,
          package_types: [],
          tags: [],
        },
      })
    ).toEqual({
      ecosystem: 'nuget',
      total: 2,
      groups: [
        {
          label: 'net8.0',
          count: 2,
          names: ['Newtonsoft.Json', 'Serilog'],
        },
      ],
    });
  });
});
