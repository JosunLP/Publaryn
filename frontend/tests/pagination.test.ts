import { describe, expect, test } from 'bun:test';

import { collectPaginatedItems } from '../src/api/pagination';

describe('pagination helpers', () => {
  test('collects items across multiple full pages until a short final page', async () => {
    const requestedPages: number[] = [];

    const items = await collectPaginatedItems(async (page, perPage) => {
      requestedPages.push(page);

      if (page === 1) {
        return Array.from({ length: perPage }, (_, index) => `page-1-${index}`);
      }

      if (page === 2) {
        return ['page-2-a', 'page-2-b'];
      }

      return [];
    });

    expect(requestedPages).toEqual([1, 2]);
    expect(items).toHaveLength(102);
    expect(items[0]).toBe('page-1-0');
    expect(items.at(-1)).toBe('page-2-b');
  });

  test('stops after the first partial page', async () => {
    const requestedPages: number[] = [];

    const items = await collectPaginatedItems(async (page) => {
      requestedPages.push(page);
      return page === 1 ? ['first', 'second'] : ['unexpected'];
    });

    expect(requestedPages).toEqual([1]);
    expect(items).toEqual(['first', 'second']);
  });

  test('throws when the pagination depth guard is exceeded', async () => {
    await expect(
      collectPaginatedItems(async (_page, perPage) => {
        return Array.from({ length: perPage }, () => 'still-full');
      }, { perPage: 2, maxPages: 2 })
    ).rejects.toThrow('Exceeded maximum pagination depth of 2 pages.');
  });
});
