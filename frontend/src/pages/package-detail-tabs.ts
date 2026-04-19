export const PACKAGE_DETAIL_TAB_VALUES = [
  'readme',
  'versions',
  'security',
] as const;

export type PackageDetailTab = (typeof PACKAGE_DETAIL_TAB_VALUES)[number];

const PACKAGE_DETAIL_TAB_SET = new Set<string>(PACKAGE_DETAIL_TAB_VALUES);

export function normalizePackageDetailTab(
  value: string | null | undefined
): PackageDetailTab {
  if (typeof value !== 'string') {
    return 'readme';
  }

  const trimmed = value.trim().toLowerCase();
  return PACKAGE_DETAIL_TAB_SET.has(trimmed) ? (trimmed as PackageDetailTab) : 'readme';
}

export function getPackageDetailTabFromQuery(
  query: URLSearchParams
): PackageDetailTab {
  return normalizePackageDetailTab(query.get('tab'));
}

export function buildPackageDetailPath(
  ecosystem: string,
  name: string,
  {
    tab,
  }: {
    tab?: string | null | undefined;
  } = {},
  currentSearch: string | URLSearchParams = ''
): string {
  const params =
    currentSearch instanceof URLSearchParams
      ? new URLSearchParams(currentSearch)
      : new URLSearchParams(currentSearch);
  const normalizedTab = normalizePackageDetailTab(tab);

  if (normalizedTab === 'readme') {
    params.delete('tab');
  } else {
    params.set('tab', normalizedTab);
  }

  const queryString = params.toString();
  const encodedEcosystem = encodeURIComponent(ecosystem);
  const encodedName = encodeURIComponent(name);

  return queryString
    ? `/packages/${encodedEcosystem}/${encodedName}?${queryString}`
    : `/packages/${encodedEcosystem}/${encodedName}`;
}
