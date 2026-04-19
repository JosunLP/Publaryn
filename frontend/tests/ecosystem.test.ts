/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import {
  formatVersionLabel,
  installCommand,
  isDigestReference,
} from '../src/utils/ecosystem';

describe('ecosystem helpers', () => {
  test('detects OCI digests', () => {
    expect(
      isDigestReference(
        'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
      )
    ).toBe(true);
    expect(isDigestReference('1.2.3')).toBe(false);
  });

  test('uses digest references for OCI install commands', () => {
    expect(
      installCommand(
        'oci',
        'ghcr.io/acme/demo-widget',
        'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
      )
    ).toBe(
      'docker pull ghcr.io/acme/demo-widget@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
    );

    expect(installCommand('oci', 'ghcr.io/acme/demo-widget', 'latest')).toBe(
      'docker pull ghcr.io/acme/demo-widget:latest'
    );
  });

  test('formats OCI digests without a v prefix', () => {
    expect(
      formatVersionLabel(
        'oci',
        'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
      )
    ).toBe(
      'sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
    );

    expect(formatVersionLabel('npm', '1.2.3')).toBe('v1.2.3');
  });
});
