import { describe, expect, test } from 'bun:test';

import { buildReleaseDependencyOverview } from '../src/utils/release-metadata';

describe('release dependency overview helpers', () => {
  test('summarizes Cargo dependency kinds', () => {
    expect(
      buildReleaseDependencyOverview({
        kind: 'cargo',
        details: {
          dependencies: [
            { name: 'serde' },
            { name: 'cc', kind: 'build' },
            { name: 'insta', kind: 'dev' },
          ],
          features: {},
        },
      })
    ).toEqual({
      ecosystem: 'cargo',
      total: 3,
      groups: [
        {
          label: 'Runtime dependencies',
          count: 1,
          names: ['serde'],
        },
        {
          label: 'Build dependencies',
          count: 1,
          names: ['cc'],
        },
        {
          label: 'Development dependencies',
          count: 1,
          names: ['insta'],
        },
      ],
    });
  });

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

  test('summarizes RubyGems runtime and development dependencies', () => {
    expect(
      buildReleaseDependencyOverview({
        kind: 'rubygems',
        details: {
          platform: 'ruby',
          authors: ['Alice'],
          licenses: ['MIT'],
          runtime_dependencies: [{ name: 'rack' }],
          development_dependencies: [{ name: 'rspec' }],
        },
      })
    ).toEqual({
      ecosystem: 'rubygems',
      total: 2,
      groups: [
        {
          label: 'Runtime dependencies',
          count: 1,
          names: ['rack'],
        },
        {
          label: 'Development dependencies',
          count: 1,
          names: ['rspec'],
        },
      ],
    });
  });

  test('summarizes Maven dependency scopes and limits unique names', () => {
    expect(
      buildReleaseDependencyOverview({
        kind: 'maven',
        details: {
          dependencies: [
            { group_id: 'org.example', artifact_id: 'alpha', scope: 'runtime' },
            { group_id: 'org.example', artifact_id: 'beta', scope: 'runtime' },
            { group_id: 'org.example', artifact_id: 'gamma', scope: 'runtime' },
            { group_id: 'org.example', artifact_id: 'delta', scope: 'runtime' },
            { group_id: 'org.example', artifact_id: 'epsilon', scope: 'runtime' },
            { group_id: 'org.example', artifact_id: 'zeta', scope: 'runtime' },
            { group_id: 'org.example', artifact_id: 'eta', scope: 'runtime' },
            { group_id: 'org.example', artifact_id: 'alpha', scope: 'runtime' },
            { artifact_id: 'junit', scope: 'test' },
          ],
        },
      })
    ).toEqual({
      ecosystem: 'maven',
      total: 9,
      groups: [
        {
          label: 'Runtime dependencies',
          count: 8,
          names: [
            'org.example:alpha',
            'org.example:beta',
            'org.example:gamma',
            'org.example:delta',
            'org.example:epsilon',
            'org.example:zeta',
          ],
        },
        {
          label: 'Test dependencies',
          count: 1,
          names: ['junit'],
        },
      ],
    });
  });

  test('returns no overview for empty, malformed, or unsupported metadata', () => {
    expect(
      buildReleaseDependencyOverview({
        kind: 'cargo',
        details: {
          dependencies: 'not-an-array',
          features: {},
        },
      })
    ).toBeNull();
    expect(
      buildReleaseDependencyOverview({
        kind: 'nuget',
        details: {
          dependency_groups: [{ dependencies: 'not-an-array' }],
          tags: [],
          package_types: [],
          is_listed: true,
        },
      })
    ).toBeNull();
    expect(
      buildReleaseDependencyOverview({
        kind: 'oci',
        details: {
          references: [],
        },
      })
    ).toBeNull();
    expect(buildReleaseDependencyOverview(null)).toBeNull();
  });
});
