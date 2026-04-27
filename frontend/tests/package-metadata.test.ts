/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import type { PackageDetail } from '../src/api/packages';
import {
  PACKAGE_VISIBILITY_VALUES_HINT,
  buildPackageMetadataUpdateInput,
  createPackageMetadataFormValues,
  getPackageMetadataChangeState,
  normalizePackageMetadataInput,
  normalizePackageMetadataKeywords,
  normalizePackageVisibilityInput,
  packageMetadataHasChanges,
} from '../src/utils/package-metadata';

const BASE_PACKAGE: PackageDetail = {
  name: 'demo-widget',
  description: 'Existing description',
  readme: '# Demo Widget\n',
  homepage: 'https://packages.example.test/demo-widget',
  repository_url: 'https://github.com/acme/demo-widget',
  license: 'MIT',
  keywords: ['docs', 'API'],
  visibility: 'public',
};

describe('package metadata helpers', () => {
  test('builds form values from package detail data', () => {
    expect(createPackageMetadataFormValues(BASE_PACKAGE)).toEqual({
      description: 'Existing description',
      readme: '# Demo Widget\n',
      homepage: 'https://packages.example.test/demo-widget',
      repositoryUrl: 'https://github.com/acme/demo-widget',
      license: 'MIT',
      keywords: 'docs, API',
      visibility: 'public',
    });
  });

  test('normalizes trimmed values and preserves non-empty readme content', () => {
    expect(
      normalizePackageMetadataInput({
        description: '  Updated description  ',
        readme: '## Demo\n\nNew readme body.\n',
        homepage: ' https://docs.example.test/demo-widget ',
        repositoryUrl: ' https://github.com/acme/demo-widget ',
        license: ' Apache-2.0 ',
        keywords: ' docs, API, docs,\ncli ',
        visibility: ' Unlisted ',
      })
    ).toEqual({
      description: 'Updated description',
      readme: '## Demo\n\nNew readme body.\n',
      homepage: 'https://docs.example.test/demo-widget',
      repositoryUrl: 'https://github.com/acme/demo-widget',
      license: 'Apache-2.0',
      keywords: ['docs', 'API', 'cli'],
      visibility: 'unlisted',
    });
  });

  test('treats blank fields and blank keyword input as cleared values', () => {
    expect(
      normalizePackageMetadataInput({
        description: '   ',
        readme: '\n\n',
        homepage: '',
        repositoryUrl: ' \t ',
        license: '',
        keywords: ' , \n ',
        visibility: ' ',
      })
    ).toEqual({
      description: null,
      readme: null,
      homepage: null,
      repositoryUrl: null,
      license: null,
      keywords: null,
      visibility: null,
    });
  });

  test('throws for invalid visibility values instead of silently ignoring them', () => {
    expect(() =>
      normalizePackageMetadataInput({
        ...createPackageMetadataFormValues(BASE_PACKAGE),
        visibility: 'unknown',
      })
    ).toThrow(
      'Invalid package visibility: unknown. Allowed values: public, private, internal_org, unlisted, quarantined.'
    );
  });

  test('normalizes allowed visibility values, including hyphenated input', () => {
    expect(normalizePackageVisibilityInput('public')).toBe('public');
    expect(normalizePackageVisibilityInput('PRIVATE')).toBe('private');
    expect(normalizePackageVisibilityInput('internal-org')).toBe(
      'internal_org'
    );
    expect(normalizePackageVisibilityInput(' quarantined ')).toBe(
      'quarantined'
    );
  });

  test('treats blank and omitted visibility input distinctly', () => {
    expect(normalizePackageVisibilityInput(' \n\t ')).toBeNull();
    expect(normalizePackageVisibilityInput(undefined)).toBeUndefined();
    expect(normalizePackageVisibilityInput(null)).toBeUndefined();
  });

  test('throws a useful visibility validation error for unknown values', () => {
    expect(() => normalizePackageVisibilityInput('internal-team')).toThrow(
      `Invalid package visibility: internal-team. Allowed values: ${PACKAGE_VISIBILITY_VALUES_HINT}. Normalized: internal_team.`
    );
  });

  test('normalizes keyword text into a stable unique list', () => {
    expect(
      normalizePackageMetadataKeywords(' docs, API, docs, api, cli ')
    ).toEqual(['docs', 'API', 'cli']);
    expect(normalizePackageMetadataKeywords(' , \n ')).toBeNull();
  });

  test('detects no-op submissions against the current package metadata', () => {
    const formValues = createPackageMetadataFormValues(BASE_PACKAGE);

    expect(packageMetadataHasChanges(BASE_PACKAGE, formValues)).toBe(false);
    expect(getPackageMetadataChangeState(BASE_PACKAGE, formValues)).toEqual({
      hasChanges: false,
      hasValidationError: false,
    });
    expect(buildPackageMetadataUpdateInput(BASE_PACKAGE, formValues)).toEqual(
      {}
    );
  });

  test('builds a patch payload only for changed fields, including clears', () => {
    const input = buildPackageMetadataUpdateInput(BASE_PACKAGE, {
      description: '  Updated description  ',
      readme: '   ',
      homepage: 'https://packages.example.test/demo-widget',
      repositoryUrl: ' https://github.com/acme/demo-widget-next ',
      license: 'MIT',
      keywords: 'docs, cli',
      visibility: 'public',
    });

    expect(input).toEqual({
      description: 'Updated description',
      readme: null,
      repositoryUrl: 'https://github.com/acme/demo-widget-next',
      keywords: ['docs', 'cli'],
    });
    expect(
      packageMetadataHasChanges(BASE_PACKAGE, {
        description: '  Updated description  ',
        readme: '   ',
        homepage: 'https://packages.example.test/demo-widget',
        repositoryUrl: ' https://github.com/acme/demo-widget-next ',
        license: 'MIT',
        keywords: 'docs, cli',
        visibility: 'public',
      })
    ).toBe(true);
  });

  test('includes visibility only when the package visibility changed', () => {
    expect(
      buildPackageMetadataUpdateInput(BASE_PACKAGE, {
        ...createPackageMetadataFormValues(BASE_PACKAGE),
        visibility: 'internal-org',
      })
    ).toEqual({
      visibility: 'internal_org',
    });
  });

  test('allows visibility to be cleared back to the default state', () => {
    const privatePackage = {
      ...BASE_PACKAGE,
      visibility: 'private',
    };

    expect(
      buildPackageMetadataUpdateInput(privatePackage, {
        ...createPackageMetadataFormValues(privatePackage),
        visibility: '',
      })
    ).toEqual({
      visibility: null,
    });
    expect(
      packageMetadataHasChanges(privatePackage, {
        ...createPackageMetadataFormValues(privatePackage),
        visibility: '',
      })
    ).toBe(true);
  });

  test('treats invalid visibility values as actionable validation errors', () => {
    const privatePackage = {
      ...BASE_PACKAGE,
      visibility: 'private',
    };

    expect(() =>
      buildPackageMetadataUpdateInput(privatePackage, {
        ...createPackageMetadataFormValues(privatePackage),
        visibility: 'definitely-not-valid',
      })
    ).toThrow(
      'Invalid package visibility: definitely-not-valid. Allowed values: public, private, internal_org, unlisted, quarantined. Normalized: definitely_not_valid.'
    );
    expect(() =>
      packageMetadataHasChanges(privatePackage, {
        ...createPackageMetadataFormValues(privatePackage),
        visibility: 'definitely-not-valid',
      })
    ).not.toThrow();
    expect(
      packageMetadataHasChanges(privatePackage, {
        ...createPackageMetadataFormValues(privatePackage),
        visibility: 'definitely-not-valid',
      })
    ).toBe(false);
    expect(
      getPackageMetadataChangeState(privatePackage, {
        ...createPackageMetadataFormValues(privatePackage),
        visibility: 'definitely-not-valid',
      })
    ).toEqual({
      hasChanges: false,
      hasValidationError: true,
    });
  });

  test('treats omitted visibility input as no change instead of clearing', () => {
    const privatePackage = {
      ...BASE_PACKAGE,
      visibility: 'private',
    };

    expect(
      buildPackageMetadataUpdateInput(privatePackage, {
        ...createPackageMetadataFormValues(privatePackage),
        visibility: undefined as unknown as string,
      })
    ).toEqual({});
    expect(
      packageMetadataHasChanges(privatePackage, {
        ...createPackageMetadataFormValues(privatePackage),
        visibility: undefined as unknown as string,
      })
    ).toBe(false);
  });
});
