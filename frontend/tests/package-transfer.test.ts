import { describe, expect, test } from 'bun:test';

import {
  selectPackageTransferTargets,
  selectTransferablePackages,
} from '../src/utils/package-transfer';

describe('package transfer helpers', () => {
  test('filters transfer targets to admin roles outside the current owner organization', () => {
    const targets = selectPackageTransferTargets(
      [
        { slug: 'viewer-org', name: 'Viewer Org', role: 'viewer' },
        { slug: 'target-b', name: 'Zulu Org', role: 'owner' },
        { slug: 'source-org', name: 'Source Org', role: 'admin' },
        { slug: 'target-a', name: 'Alpha Org', role: 'admin' },
        { slug: null, name: 'Missing slug', role: 'owner' },
      ],
      'source-org'
    );

    expect(targets).toEqual([
      { slug: 'target-a', name: 'Alpha Org', role: 'admin' },
      { slug: 'target-b', name: 'Zulu Org', role: 'owner' },
    ]);
  });

  test('keeps only transferable packages and sorts them by ecosystem and name', () => {
    const packages = selectTransferablePackages([
      { ecosystem: 'npm', name: 'zeta-widget', can_transfer: true },
      { ecosystem: 'pypi', name: 'alpha-widget', can_transfer: false },
      { ecosystem: 'cargo', name: 'beta-widget', can_transfer: true },
      { ecosystem: 'npm', name: 'alpha-widget', can_transfer: true },
      { ecosystem: 'npm', name: '', can_transfer: true },
    ]);

    expect(packages).toEqual([
      { ecosystem: 'cargo', name: 'beta-widget', can_transfer: true },
      { ecosystem: 'npm', name: 'alpha-widget', can_transfer: true },
      { ecosystem: 'npm', name: 'zeta-widget', can_transfer: true },
    ]);
  });
});
