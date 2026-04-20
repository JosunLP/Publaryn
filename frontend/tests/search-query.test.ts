import { describe, expect, test } from 'bun:test';

import {
  buildSearchPath,
  getSearchViewFromQuery,
  normalizeSearchEcosystem,
} from '../src/pages/search-query';

describe('search query helpers', () => {
  test('parses query, ecosystem, org, and page filters from the URL', () => {
    const view = getSearchViewFromQuery(
      new URLSearchParams('q=widget&ecosystem=npm&org=acme-org-search&page=3')
    );

    expect(view).toEqual({
      q: 'widget',
      ecosystem: 'npm',
      org: 'acme-org-search',
      page: 3,
    });
  });

  test('normalizes bun aliases and drops invalid ecosystems and pages', () => {
    expect(normalizeSearchEcosystem('bun')).toBe('npm');
    expect(
      getSearchViewFromQuery(
        new URLSearchParams('q=widget&ecosystem=invalid&org=%20&page=-2')
      )
    ).toEqual({
      q: 'widget',
      ecosystem: '',
      org: '',
      page: 1,
    });
  });

  test('builds search paths while preserving unrelated query params', () => {
    expect(
      buildSearchPath(
        {
          q: 'private widget',
          ecosystem: 'npm',
          org: 'acme-org-search',
          page: 2,
        },
        'tab=packages&theme=dark'
      )
    ).toBe(
      '/search?tab=packages&theme=dark&q=private+widget&ecosystem=npm&org=acme-org-search&page=2'
    );
  });

  test('clears optional filters and drops page 1 from the URL', () => {
    expect(
      buildSearchPath(
        {
          q: '',
          ecosystem: '',
          org: '',
          page: 1,
        },
        'q=widget&ecosystem=npm&org=acme-org-search&page=4'
      )
    ).toBe('/search');
  });
});
