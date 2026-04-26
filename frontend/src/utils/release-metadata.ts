import type { JsonValue, ReleaseEcosystemMetadata } from '../api/packages';

export interface ReleaseDependencyGroupSummary {
  label: string;
  count: number;
  names: string[];
}

export interface ReleaseDependencyOverview {
  ecosystem: string;
  total: number;
  groups: ReleaseDependencyGroupSummary[];
}

type JsonRecord = Record<string, JsonValue | undefined>;

function isRecord(value: unknown): value is JsonRecord {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

function stringValue(value: unknown): string | null {
  return typeof value === 'string' && value.trim().length > 0
    ? value.trim()
    : null;
}

function dependencyName(value: unknown): string | null {
  if (typeof value === 'string') {
    return stringValue(value);
  }

  if (!isRecord(value)) {
    return null;
  }

  const directName =
    stringValue(value.name) ||
    stringValue(value.id) ||
    stringValue(value.package) ||
    stringValue(value.crate_name) ||
    stringValue(value.artifact_id);
  const groupId = stringValue(value.group_id);

  return groupId && directName ? `${groupId}:${directName}` : directName;
}

function uniqueNames(values: unknown[], limit = 6): string[] {
  const names: string[] = [];
  const seen = new Set<string>();

  for (const value of values) {
    const name = dependencyName(value);

    if (!name || seen.has(name)) {
      continue;
    }

    seen.add(name);
    names.push(name);

    if (names.length >= limit) {
      break;
    }
  }

  return names;
}

function groupSummary(
  label: string,
  dependencies: unknown[]
): ReleaseDependencyGroupSummary | null {
  if (dependencies.length === 0) {
    return null;
  }

  return {
    label,
    count: dependencies.length,
    names: uniqueNames(dependencies),
  };
}

function compactGroups(
  ecosystem: string,
  groups: Array<ReleaseDependencyGroupSummary | null>
): ReleaseDependencyOverview | null {
  const filteredGroups = groups.filter(
    (group): group is ReleaseDependencyGroupSummary => Boolean(group)
  );
  const total = filteredGroups.reduce((sum, group) => sum + group.count, 0);

  return total > 0
    ? {
        ecosystem,
        total,
        groups: filteredGroups,
      }
    : null;
}

function dependencyObjectEntries(value: unknown): unknown[] {
  if (!isRecord(value)) {
    return [];
  }

  return Object.keys(value)
    .filter((name) => name.trim().length > 0)
    .map((name) => ({ name }));
}

function cargoDependencyOverview(
  details: Extract<ReleaseEcosystemMetadata, { kind: 'cargo' }>['details']
): ReleaseDependencyOverview | null {
  const dependencies = Array.isArray(details.dependencies)
    ? details.dependencies
    : [];
  const runtime = dependencies.filter(
    (dependency) =>
      !isRecord(dependency) ||
      !stringValue(dependency.kind) ||
      stringValue(dependency.kind) === 'normal'
  );
  const build = dependencies.filter(
    (dependency) =>
      isRecord(dependency) && stringValue(dependency.kind) === 'build'
  );
  const development = dependencies.filter(
    (dependency) =>
      isRecord(dependency) && stringValue(dependency.kind) === 'dev'
  );

  return compactGroups('cargo', [
    groupSummary('Runtime dependencies', runtime),
    groupSummary('Build dependencies', build),
    groupSummary('Development dependencies', development),
  ]);
}

function nugetDependencyOverview(
  details: Extract<ReleaseEcosystemMetadata, { kind: 'nuget' }>['details']
): ReleaseDependencyOverview | null {
  const groups = Array.isArray(details.dependency_groups)
    ? details.dependency_groups
    : [];

  return compactGroups(
    'nuget',
    groups.map((group) => {
      if (!isRecord(group)) {
        return null;
      }

      const dependencies = Array.isArray(group.dependencies)
        ? group.dependencies
        : [];
      const label =
        stringValue(group.target_framework) ||
        stringValue(group.targetFramework) ||
        'Any framework';

      return groupSummary(label, dependencies);
    })
  );
}

function rubygemsDependencyOverview(
  details: Extract<ReleaseEcosystemMetadata, { kind: 'rubygems' }>['details']
): ReleaseDependencyOverview | null {
  return compactGroups('rubygems', [
    groupSummary(
      'Runtime dependencies',
      Array.isArray(details.runtime_dependencies)
        ? details.runtime_dependencies
        : []
    ),
    groupSummary(
      'Development dependencies',
      Array.isArray(details.development_dependencies)
        ? details.development_dependencies
        : []
    ),
  ]);
}

function composerDependencyOverview(
  details: Record<string, JsonValue>
): ReleaseDependencyOverview | null {
  return compactGroups('composer', [
    groupSummary('Runtime require', dependencyObjectEntries(details.require)),
    groupSummary(
      'Development require',
      dependencyObjectEntries(details['require-dev'])
    ),
  ]);
}

function mavenDependencyOverview(
  details: Record<string, JsonValue>
): ReleaseDependencyOverview | null {
  const dependencies = Array.isArray(details.dependencies)
    ? details.dependencies
    : [];
  const grouped = new Map<string, unknown[]>();

  for (const dependency of dependencies) {
    const rawScope = isRecord(dependency) ? stringValue(dependency.scope) : null;
    const scope = rawScope ?? 'compile';
    const label = `${scope.charAt(0).toUpperCase()}${scope.slice(1)} dependencies`;
    grouped.set(label, [...(grouped.get(label) || []), dependency]);
  }

  return compactGroups(
    'maven',
    [...grouped.entries()].map(([label, dependencies]) =>
      groupSummary(label, dependencies)
    )
  );
}

export function buildReleaseDependencyOverview(
  metadata: ReleaseEcosystemMetadata | null | undefined
): ReleaseDependencyOverview | null {
  if (!metadata) {
    return null;
  }

  switch (metadata.kind) {
    case 'cargo':
      return cargoDependencyOverview(metadata.details);
    case 'nuget':
      return nugetDependencyOverview(metadata.details);
    case 'rubygems':
      return rubygemsDependencyOverview(metadata.details);
    case 'composer':
      return composerDependencyOverview(metadata.details);
    case 'maven':
      return mavenDependencyOverview(metadata.details);
    default:
      return null;
  }
}
