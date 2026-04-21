const DEFAULT_PAGE_SIZE = 100;
const DEFAULT_MAX_PAGES = 1000;

export interface CollectPaginatedItemsOptions {
  perPage?: number;
  maxPages?: number;
}

function resolvePositiveIntegerOption(
  name: 'perPage' | 'maxPages',
  value: number | undefined,
  fallback: number
): number {
  if (value === undefined) {
    return fallback;
  }

  if (!Number.isInteger(value) || value <= 0) {
    throw new Error(`${name} must be a positive integer.`);
  }

  return value;
}

export async function collectPaginatedItems<TItem>(
  fetchPage: (page: number, perPage: number) => Promise<TItem[]>,
  options: CollectPaginatedItemsOptions = {}
): Promise<TItem[]> {
  const perPage = resolvePositiveIntegerOption(
    'perPage',
    options.perPage,
    DEFAULT_PAGE_SIZE
  );
  const maxPages = resolvePositiveIntegerOption(
    'maxPages',
    options.maxPages,
    DEFAULT_MAX_PAGES
  );
  const items: TItem[] = [];

  for (let page = 1; page <= maxPages; page += 1) {
    const pageItems = await fetchPage(page, perPage);
    items.push(...pageItems);

    if (pageItems.length < perPage) {
      return items;
    }
  }

  throw new Error(`Exceeded maximum pagination depth of ${maxPages} pages.`);
}
