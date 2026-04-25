import type { PackageDetail, UpdatePackageInput } from '../api/packages';

export interface PackageMetadataFormValues {
  description: string;
  readme: string;
  homepage: string;
  repositoryUrl: string;
  license: string;
  keywords: string;
  visibility: string;
}

interface NormalizedPackageMetadataValues {
  description: string | null;
  readme: string | null;
  homepage: string | null;
  repositoryUrl: string | null;
  license: string | null;
  keywords: string[] | null;
  visibility: string | null;
}

const PACKAGE_VISIBILITY_VALUES = new Set([
  'public',
  'private',
  'internal_org',
  'unlisted',
  'quarantined',
]);

export function createPackageMetadataFormValues(
  pkg: PackageDetail | null | undefined
): PackageMetadataFormValues {
  return {
    description: pkg?.description ?? '',
    readme: pkg?.readme ?? '',
    homepage: pkg?.homepage ?? '',
    repositoryUrl: pkg?.repository_url ?? '',
    license: pkg?.license ?? '',
    keywords: Array.isArray(pkg?.keywords) ? pkg.keywords.join(', ') : '',
    visibility: normalizePackageVisibility(pkg?.visibility) ?? '',
  };
}

export function normalizePackageMetadataInput(
  values: PackageMetadataFormValues
): NormalizedPackageMetadataValues {
  return {
    description: normalizePackageMetadataText(values.description),
    readme: normalizePackageMetadataReadme(values.readme),
    homepage: normalizePackageMetadataText(values.homepage),
    repositoryUrl: normalizePackageMetadataText(values.repositoryUrl),
    license: normalizePackageMetadataText(values.license),
    keywords: normalizePackageMetadataKeywords(values.keywords),
    visibility: normalizePackageVisibility(values.visibility),
  };
}

export function buildPackageMetadataUpdateInput(
  pkg: PackageDetail | null | undefined,
  values: PackageMetadataFormValues
): UpdatePackageInput {
  const current = normalizeCurrentPackageMetadata(pkg);
  const next = normalizePackageMetadataInput(values);
  const input: UpdatePackageInput = {};

  if (current.description !== next.description) {
    input.description = next.description;
  }

  if (current.readme !== next.readme) {
    input.readme = next.readme;
  }

  if (current.homepage !== next.homepage) {
    input.homepage = next.homepage;
  }

  if (current.repositoryUrl !== next.repositoryUrl) {
    input.repositoryUrl = next.repositoryUrl;
  }

  if (current.license !== next.license) {
    input.license = next.license;
  }

  if (!sameKeywords(current.keywords, next.keywords)) {
    input.keywords = next.keywords;
  }

  if (current.visibility !== next.visibility) {
    input.visibility = next.visibility ?? null;
  }

  return input;
}

export function packageMetadataHasChanges(
  pkg: PackageDetail | null | undefined,
  values: PackageMetadataFormValues
): boolean {
  return Object.keys(buildPackageMetadataUpdateInput(pkg, values)).length > 0;
}

export function normalizePackageMetadataKeywords(
  value: string | null | undefined
): string[] | null {
  if (typeof value !== 'string') {
    return null;
  }

  const normalized: string[] = [];
  for (const part of value.split(/[\n,]+/)) {
    const trimmed = part.trim();
    if (!trimmed) {
      continue;
    }

    if (
      normalized.some(
        (existingKeyword) =>
          existingKeyword.toLowerCase() === trimmed.toLowerCase()
      )
    ) {
      continue;
    }

    normalized.push(trimmed);
  }

  return normalized.length > 0 ? normalized : null;
}

function normalizeCurrentPackageMetadata(
  pkg: PackageDetail | null | undefined
): NormalizedPackageMetadataValues {
  return {
    description: normalizePackageMetadataText(pkg?.description),
    readme:
      typeof pkg?.readme === 'string' && pkg.readme.trim().length > 0
        ? pkg.readme
        : null,
    homepage: normalizePackageMetadataText(pkg?.homepage),
    repositoryUrl: normalizePackageMetadataText(pkg?.repository_url),
    license: normalizePackageMetadataText(pkg?.license),
    keywords: normalizeKeywordList(pkg?.keywords),
    visibility: normalizePackageVisibility(pkg?.visibility),
  };
}

function normalizePackageVisibility(
  value: string | null | undefined
): string | null {
  if (typeof value !== 'string') {
    return null;
  }

  const normalized = value.trim().toLowerCase().replace(/-/g, '_');
  return PACKAGE_VISIBILITY_VALUES.has(normalized) ? normalized : null;
}

function normalizePackageMetadataText(
  value: string | null | undefined
): string | null {
  if (typeof value !== 'string') {
    return null;
  }

  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function normalizePackageMetadataReadme(
  value: string | null | undefined
): string | null {
  if (typeof value !== 'string') {
    return null;
  }

  return value.trim().length > 0 ? value : null;
}

function normalizeKeywordList(
  keywords: string[] | null | undefined
): string[] | null {
  if (!Array.isArray(keywords)) {
    return null;
  }

  const normalized: string[] = [];
  for (const keyword of keywords) {
    if (typeof keyword !== 'string') {
      continue;
    }

    const trimmed = keyword.trim();
    if (!trimmed) {
      continue;
    }

    if (
      normalized.some(
        (existingKeyword) =>
          existingKeyword.toLowerCase() === trimmed.toLowerCase()
      )
    ) {
      continue;
    }

    normalized.push(trimmed);
  }

  return normalized.length > 0 ? normalized : null;
}

function sameKeywords(left: string[] | null, right: string[] | null): boolean {
  if (left === right) {
    return true;
  }

  if (!left || !right || left.length !== right.length) {
    return false;
  }

  return left.every((keyword, index) => keyword === right[index]);
}
