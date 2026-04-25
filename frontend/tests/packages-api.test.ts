/// <reference path="./bun-test.d.ts" />

import { afterEach, describe, expect, test } from 'bun:test';

import { api } from '../src/api/client';
import { updatePackage } from '../src/api/packages';

describe('packages api client helpers', () => {
  const originalPatch = api.patch;

  afterEach(() => {
    api.patch = originalPatch;
  });

  test('updatePackage preserves explicit null visibility values', async () => {
    const calls: Array<{ path: string; body: unknown }> = [];

    api.patch = (async <T>(path: string, options?: { body?: unknown }) => {
      calls.push({ path, body: options?.body });
      return {
        data: { status: 'ok' } as T,
        requestId: null,
      };
    }) as typeof api.patch;

    await updatePackage('npm', 'demo-widget', {
      visibility: null,
    });

    expect(calls).toEqual([
      {
        path: '/v1/packages/npm/demo-widget',
        body: {
          visibility: null,
        },
      },
    ]);
  });
});
