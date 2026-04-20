const SEARCH_QUERY_KEYS = {
  q: 'q',
  ecosystem: 'ecosystem',
  org: 'org',
  page: 'page',
} as const;

export const SEARCH_ECOSYSTEM_VALUES = [
  'npm',
  'pypi',
  'cargo',
  'nuget',
  'rubygems',
  'maven',
  'composer',
  'oci',
] as const;

const SEARCH_ECOSYSTEM_SET = new Set<string>(SEARCH_ECOSYSTEM_VALUES);

export interface SearchView {
  q: string;
  ecosystem: string;
  org: string;
  page: number;
}

export function normalizeSearchQuery(value: string | null | undefined): string {
  return typeof value === 'string' ? value.trim() : '';
}

export function normalizeSearchEcosystem(value: string | null | undefined): string {
  const normalizedValue = normalizeSearchQuery(value).toLowerCase();
  if (!normalizedValue) {
    return '';
  }

  if (normalizedValue === 'bun') {
    return 'npm';
  }

  return SEARCH_ECOSYSTEM_SET.has(normalizedValue) ? normalizedValue : '';
}

export function normalizeSearchOrg(value: string | null | undefined): string {
  return normalizeSearchQuery(value);
}

export function normalizeSearchPage(value: string | number | null | undefined): number {
  const parsedValue =
    typeof value === 'number' ? value : Number.parseInt(normalizeSearchQuery(value), 10);
  return Number.isFinite(parsedValue) && parsedValue > 0 ? parsedValue : 1;
}

export function getSearchViewFromQuery(query: URLSearchParams): SearchView {
  return {
    q: normalizeSearchQuery(query.get(SEARCH_QUERY_KEYS.q)),
    ecosystem: normalizeSearchEcosystem(query.get(SEARCH_QUERY_KEYS.ecosystem)),
    org: normalizeSearchOrg(query.get(SEARCH_QUERY_KEYS.org)),
    page: normalizeSearchPage(query.get(SEARCH_QUERY_KEYS.page)),
  };
}

export function buildSearchPath(
  {
    q,
    ecosystem,
    org,
    page,
  }: {
    q?: string | null | undefined;
    ecosystem?: string | null | undefined;
    org?: string | null | undefined;
    page?: string | number | null | undefined;
  },
  currentSearch: string | URLSearchParams = ''
): string {
  const params =
    currentSearch instanceof URLSearchParams
      ? new URLSearchParams(currentSearch)
      : new URLSearchParams(currentSearch);
  const normalizedQuery = normalizeSearchQuery(q);
  const normalizedEcosystem = normalizeSearchEcosystem(ecosystem);
  const normalizedOrg = normalizeSearchOrg(org);
  const normalizedPage = normalizeSearchPage(page);

  if (normalizedQuery) {
    params.set(SEARCH_QUERY_KEYS.q, normalizedQuery);
  } else {
    params.delete(SEARCH_QUERY_KEYS.q);
  }

  if (normalizedEcosystem) {
    params.set(SEARCH_QUERY_KEYS.ecosystem, normalizedEcosystem);
  } else {
    params.delete(SEARCH_QUERY_KEYS.ecosystem);
  }

  if (normalizedOrg) {
    params.set(SEARCH_QUERY_KEYS.org, normalizedOrg);
  } else {
    params.delete(SEARCH_QUERY_KEYS.org);
  }

  if (normalizedPage > 1) {
    params.set(SEARCH_QUERY_KEYS.page, String(normalizedPage));
  } else {
    params.delete(SEARCH_QUERY_KEYS.page);
  }

  const queryString = params.toString();
  return queryString ? `/search?${queryString}` : '/search';
}
