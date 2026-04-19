export type EcosystemId =
  | 'npm'
  | 'pypi'
  | 'cargo'
  | 'nuget'
  | 'rubygems'
  | 'maven'
  | 'composer'
  | 'oci';

export interface EcosystemDefinition {
  id: EcosystemId;
  label: string;
  icon: string;
}

const OCI_DIGEST_PATTERN = /^[A-Za-z][A-Za-z0-9_+.-]*:[A-Fa-f0-9]{32,}$/;

export function isDigestReference(value: string | null | undefined): boolean {
  const trimmed = value?.trim();
  return Boolean(trimmed && OCI_DIGEST_PATTERN.test(trimmed));
}

export function formatVersionLabel(
  ecosystem: string | null | undefined,
  version: string | null | undefined
): string {
  const trimmed = version?.trim();

  if (!trimmed) {
    return '';
  }

  if (ecosystem?.toLowerCase() === 'oci' && isDigestReference(trimmed)) {
    return trimmed;
  }

  return `v${trimmed}`;
}

/**
 * Generate ecosystem-specific install instructions.
 */
export function installCommand(
  ecosystem: string | null | undefined,
  name: string,
  version?: string | null
): string {
  const suffix = version ? `@${version}` : '';

  switch (ecosystem?.toLowerCase()) {
    case 'npm':
    case 'bun':
      return `npm install ${name}${suffix}`;
    case 'pypi':
      return version
        ? `pip install ${name}==${version}`
        : `pip install ${name}`;
    case 'cargo':
      return version ? `cargo add ${name}@${version}` : `cargo add ${name}`;
    case 'nuget':
      return version
        ? `dotnet add package ${name} --version ${version}`
        : `dotnet add package ${name}`;
    case 'rubygems':
      return version
        ? `gem install ${name} -v ${version}`
        : `gem install ${name}`;
    case 'composer':
      return version
        ? `composer require ${name}:${version}`
        : `composer require ${name}`;
    case 'maven': {
      const [groupId = name, artifactId = name] = name.split(':');
      return `<dependency>\n  <groupId>${groupId}</groupId>\n  <artifactId>${artifactId}</artifactId>${version ? `\n  <version>${version}</version>` : ''}\n</dependency>`;
    }
    case 'oci': {
      const reference = version
        ? isDigestReference(version)
          ? `@${version}`
          : `:${version}`
        : '';
      return `docker pull ${name}${reference}`;
    }
    default:
      return name;
  }
}

/**
 * User-friendly ecosystem display names.
 */
export const ECOSYSTEMS: EcosystemDefinition[] = [
  { id: 'npm', label: 'npm', icon: '📦' },
  { id: 'pypi', label: 'PyPI', icon: '🐍' },
  { id: 'cargo', label: 'Cargo', icon: '🦀' },
  { id: 'nuget', label: 'NuGet', icon: '🔷' },
  { id: 'rubygems', label: 'RubyGems', icon: '💎' },
  { id: 'maven', label: 'Maven', icon: '☕' },
  { id: 'composer', label: 'Composer', icon: '🎵' },
  { id: 'oci', label: 'OCI / Docker', icon: '🐳' },
];

export function ecosystemLabel(id: string | null | undefined): string {
  return (
    ECOSYSTEMS.find((ecosystem) => ecosystem.id === id?.toLowerCase())?.label ||
    id ||
    ''
  );
}

export function ecosystemIcon(id: string | null | undefined): string {
  return (
    ECOSYSTEMS.find((ecosystem) => ecosystem.id === id?.toLowerCase())?.icon ||
    '📦'
  );
}
