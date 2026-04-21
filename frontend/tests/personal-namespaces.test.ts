import { describe, expect, test } from 'bun:test';

import type { NamespaceClaim } from '../src/api/namespaces';
import {
  formatNamespaceClaimStatusLabel,
  selectNamespaceTransferTargets,
  sortNamespaceClaims,
} from '../src/pages/personal-namespaces';

describe('personal namespace helpers', () => {
  test('sorts namespace claims by ecosystem and namespace', () => {
    const claims: NamespaceClaim[] = [
      { ecosystem: 'pypi', namespace: 'zeta' },
      { ecosystem: 'npm', namespace: '@zeta' },
      { ecosystem: 'npm', namespace: '@acme' },
    ];

    expect(sortNamespaceClaims(claims).map((claim) => claim.namespace)).toEqual([
      '@acme',
      '@zeta',
      'zeta',
    ]);
  });

  test('formats verification labels for personal namespace claims', () => {
    expect(formatNamespaceClaimStatusLabel({ is_verified: true })).toBe(
      'Verified'
    );
    expect(formatNamespaceClaimStatusLabel({ is_verified: false })).toBe(
      'Pending verification'
    );
    expect(formatNamespaceClaimStatusLabel(null)).toBe('Pending verification');
  });

  test('filters namespace transfer targets to admin roles outside the current owner organization', () => {
    const targets = selectNamespaceTransferTargets(
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
});
