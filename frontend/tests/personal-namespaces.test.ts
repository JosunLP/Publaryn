import { describe, expect, test } from 'bun:test';

import type { NamespaceClaim } from '../src/api/namespaces';
import {
  formatNamespaceClaimStatusLabel,
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
});
