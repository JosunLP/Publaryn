import { describe, expect, test } from 'bun:test';

import { formatSearchResultRepository } from '../src/pages/search-results';

describe('search result repository formatting', () => {
  test('includes repository name and slug when both are available', () => {
    expect(
      formatSearchResultRepository({
        repository_name: 'Release Packages',
        repository_slug: 'release-packages',
      })
    ).toBe('Release Packages (release-packages)');
  });

  test('falls back to whichever repository field is available', () => {
    expect(
      formatSearchResultRepository({
        repository_name: 'Release Packages',
        repository_slug: null,
      })
    ).toBe('Release Packages');

    expect(
      formatSearchResultRepository({
        repository_name: '  ',
        repository_slug: 'release-packages',
      })
    ).toBe('release-packages');

    expect(
      formatSearchResultRepository({
        repository_name: undefined,
        repository_slug: undefined,
      })
    ).toBe('');
  });
});
