/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import './svelte-dom';

import type { OrganizationMembership } from '../src/api/orgs';
import type { RepositoryDetail } from '../src/api/repositories';
import {
  createRepositoryTransferController,
  loadRepositoryTransferState,
} from '../src/pages/repository-transfer';

describe('repository transfer helpers', () => {
  test('skips loading transfer targets outside the browser or without transfer access', async () => {
    const minimalRepository: RepositoryDetail = { can_transfer: true };

    await expect(
      loadRepositoryTransferState({
        isBrowser: false,
        repository: minimalRepository,
        dependencies: {
          getAuthToken: () => 'token',
          async listMyOrganizations() {
            throw new Error('should not load');
          },
        },
      })
    ).resolves.toEqual({
      showTransfer: false,
      organizations: [],
      loadError: null,
    });

    await expect(
      loadRepositoryTransferState({
        isBrowser: true,
        repository: { can_transfer: false },
        dependencies: {
          getAuthToken: () => 'token',
          async listMyOrganizations() {
            throw new Error('should not load');
          },
        },
      })
    ).resolves.toEqual({
      showTransfer: false,
      organizations: [],
      loadError: null,
    });
  });

  test('loads eligible transfer targets and excludes the current owner organization', async () => {
    const organizations: OrganizationMembership[] = [
      { slug: 'target-b', name: 'Target B', role: 'member' },
      { slug: 'target-a', name: 'Target A', role: 'admin' },
      { slug: 'source-org', name: 'Source Org', role: 'owner' },
      { slug: 'target-c', name: 'Target C', role: 'owner' },
    ];

    await expect(
      loadRepositoryTransferState({
        isBrowser: true,
        repository: {
          can_transfer: true,
          owner_org_slug: 'source-org',
        },
        dependencies: {
          getAuthToken: () => 'token',
          async listMyOrganizations() {
            return { organizations };
          },
        },
      })
    ).resolves.toEqual({
      showTransfer: true,
      organizations: [
        { slug: 'target-a', name: 'Target A', role: 'admin' },
        { slug: 'target-c', name: 'Target C', role: 'owner' },
      ],
      loadError: null,
    });
  });

  test('surfaces transfer-target loading failures without hiding the transfer UI', async () => {
    await expect(
      loadRepositoryTransferState({
        isBrowser: true,
        repository: { can_transfer: true },
        dependencies: {
          getAuthToken: () => 'token',
          async listMyOrganizations() {
            throw new Error('Organizations are unavailable.');
          },
        },
      })
    ).resolves.toEqual({
      showTransfer: true,
      organizations: [],
      loadError: 'Organizations are unavailable.',
    });
  });

  test('validates repository transfer confirmation before submitting', async () => {
    let notice: string | null = 'existing notice';
    let error: string | null = null;
    let transferring = false;

    const controller = createRepositoryTransferController({
      getRepository: () => ({ slug: 'repo', can_transfer: true }),
      getSlug: () => 'repo',
      getTargetOrgSlug: () => '',
      getTransferConfirmed: () => false,
      setNotice: (value) => {
        notice = value;
      },
      setError: (value) => {
        error = value;
      },
      setTransferringRepository: (value) => {
        transferring = value;
      },
      async loadRepositoryPage() {
        throw new Error('should not reload');
      },
      toErrorMessage: (_caughtError, fallback) => fallback,
      dependencies: {
        getAuthToken: () => 'token',
        async listMyOrganizations() {
          return { organizations: [] };
        },
        async transferRepositoryOwnership() {
          throw new Error('should not transfer');
        },
      },
    });

    const form = document.createElement('form');
    await controller.submit({
      preventDefault() {},
      currentTarget: form,
    } as unknown as SubmitEvent);

    expect(notice).toBeNull();
    expect(error).toBe('Select a target organization.');
    expect(transferring).toBeFalse();
  });

  test('submits repository transfer and reloads the page on success', async () => {
    const transferCalls: Array<{ slug: string; targetOrgSlug: string }> = [];
    const reloads: Array<{ notice?: string | null; error?: string | null }> = [];
    let transferring = false;

    const controller = createRepositoryTransferController({
      getRepository: () => ({ slug: 'repo', can_transfer: true }),
      getSlug: () => 'repo',
      getTargetOrgSlug: () => 'target-org',
      getTransferConfirmed: () => true,
      setNotice() {},
      setError() {},
      setTransferringRepository: (value) => {
        transferring = value;
      },
      async loadRepositoryPage(options) {
        reloads.push(options || {});
      },
      toErrorMessage: (_caughtError, fallback) => fallback,
      dependencies: {
        getAuthToken: () => 'token',
        async listMyOrganizations() {
          return { organizations: [] };
        },
        async transferRepositoryOwnership(slug, input) {
          transferCalls.push({ slug, targetOrgSlug: input.targetOrgSlug });
          return {
            owner: {
              slug: input.targetOrgSlug,
            },
          };
        },
      },
    });

    const form = document.createElement('form');
    await controller.submit({
      preventDefault() {},
      currentTarget: form,
    } as unknown as SubmitEvent);

    expect(transferCalls).toEqual([{ slug: 'repo', targetOrgSlug: 'target-org' }]);
    expect(reloads).toEqual([
      {
        notice: 'Repository ownership transferred to target-org.',
      },
    ]);
    expect(transferring).toBeFalse();
  });

  test('keeps transfer state active when the mutation fails', async () => {
    let error: string | null = null;
    let transferring = false;
    let reloadCount = 0;

    const controller = createRepositoryTransferController({
      getRepository: () => ({ slug: 'repo', can_transfer: true }),
      getSlug: () => 'repo',
      getTargetOrgSlug: () => 'target-org',
      getTransferConfirmed: () => true,
      setNotice() {},
      setError: (value) => {
        error = value;
      },
      setTransferringRepository: (value) => {
        transferring = value;
      },
      async loadRepositoryPage() {
        reloadCount += 1;
      },
      toErrorMessage: (_caughtError, fallback) => fallback,
      dependencies: {
        getAuthToken: () => 'token',
        async listMyOrganizations() {
          return { organizations: [] };
        },
        async transferRepositoryOwnership() {
          throw new Error('boom');
        },
      },
    });

    const form = document.createElement('form');
    await controller.submit({
      preventDefault() {},
      currentTarget: form,
    } as unknown as SubmitEvent);

    expect(error).toBe('Failed to transfer repository ownership.');
    expect(transferring).toBeFalse();
    expect(reloadCount).toBe(0);
  });
});
