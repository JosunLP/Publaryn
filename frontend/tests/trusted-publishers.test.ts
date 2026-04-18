import { describe, expect, test } from 'bun:test';

import {
  normalizeTrustedPublisherInput,
  trustedPublisherBindingFields,
  trustedPublisherHeading,
} from '../src/utils/trusted-publishers';

describe('trusted publisher helpers', () => {
  test('normalizes required and optional trusted publisher input values', () => {
    expect(
      normalizeTrustedPublisherInput({
        issuer: ' https://token.actions.githubusercontent.com ',
        subject: ' repo:acme/demo-widget:ref:refs/heads/main ',
        repository: ' acme/demo-widget ',
        workflowRef: '   ',
        environment: ' production ',
      })
    ).toEqual({
      issuer: 'https://token.actions.githubusercontent.com',
      subject: 'repo:acme/demo-widget:ref:refs/heads/main',
      repository: 'acme/demo-widget',
      workflowRef: undefined,
      environment: 'production',
    });
  });

  test('requires issuer and subject values', () => {
    expect(() =>
      normalizeTrustedPublisherInput({
        issuer: ' ',
        subject: 'repo:acme/demo-widget:ref:refs/heads/main',
      })
    ).toThrow('Issuer is required.');

    expect(() =>
      normalizeTrustedPublisherInput({
        issuer: 'https://token.actions.githubusercontent.com',
        subject: '',
      })
    ).toThrow('Subject is required.');
  });

  test('builds stable headings and binding metadata for display', () => {
    expect(
      trustedPublisherHeading({
        repository: 'acme/demo-widget',
        subject: 'repo:acme/demo-widget:ref:refs/heads/main',
      })
    ).toBe('acme/demo-widget');

    expect(
      trustedPublisherBindingFields({
        repository: 'acme/demo-widget',
        workflow_ref: '.github/workflows/publish.yml@refs/heads/main',
        environment: 'production',
      })
    ).toEqual([
      { label: 'Repository', value: 'acme/demo-widget' },
      {
        label: 'Workflow',
        value: '.github/workflows/publish.yml@refs/heads/main',
      },
      { label: 'Environment', value: 'production' },
    ]);
  });
});
