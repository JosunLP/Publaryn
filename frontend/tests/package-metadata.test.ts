/// <reference path="./bun-test.d.ts" />

import { describe, expect, test } from 'bun:test';

import type { PackageDetail } from '../src/api/packages';
import {
    buildPackageMetadataUpdateInput,
    createPackageMetadataFormValues,
    normalizePackageMetadataInput,
    normalizePackageMetadataKeywords,
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
      })
    ).toEqual({
      description: 'Updated description',
      readme: '## Demo\n\nNew readme body.\n',
      homepage: 'https://docs.example.test/demo-widget',
      repositoryUrl: 'https://github.com/acme/demo-widget',
      license: 'Apache-2.0',
      keywords: ['docs', 'API', 'cli'],
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
      })
    ).toEqual({
      description: null,
      readme: null,
      homepage: null,
      repositoryUrl: null,
      license: null,
      keywords: null,
    });
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
      })
    ).toBe(true);
  });
});
