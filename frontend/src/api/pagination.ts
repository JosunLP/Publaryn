const DEFAULT_PAGE_SIZE = 100;
const DEFAULT_MAX_PAGES = 1000;

export interface CollectPaginatedItemsOptions {
  perPage?: number;
  maxPages?: number;
}

export async function collectPaginatedItems<TItem>(
  fetchPage: (page: number, perPage: number) => Promise<TItem[]>,
  options: CollectPaginatedItemsOptions = {}
): Promise<TItem[]> {
  const perPage = Math.max(1, Math.trunc(options.perPage ?? DEFAULT_PAGE_SIZE));
  const maxPages = Math.max(1, Math.trunc(options.maxPages ?? DEFAULT_MAX_PAGES));
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
