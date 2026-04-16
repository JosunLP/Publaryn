/**
 * Generate ecosystem-specific install instructions.
 */
export function installCommand(ecosystem, name, version) {
  const v = version ? `@${version}` : '';
  switch (ecosystem?.toLowerCase()) {
    case 'npm':
    case 'bun':
      return `npm install ${name}${v}`;
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
    case 'maven':
      return `<dependency>\n  <groupId>${name.split(':')[0] || name}</groupId>\n  <artifactId>${name.split(':')[1] || name}</artifactId>${version ? `\n  <version>${version}</version>` : ''}\n</dependency>`;
    case 'oci':
      return `docker pull ${name}${version ? `:${version}` : ''}`;
    default:
      return name;
  }
}

/**
 * User-friendly ecosystem display names.
 */
export const ECOSYSTEMS = [
  { id: 'npm', label: 'npm', icon: '📦' },
  { id: 'pypi', label: 'PyPI', icon: '🐍' },
  { id: 'cargo', label: 'Cargo', icon: '🦀' },
  { id: 'nuget', label: 'NuGet', icon: '🔷' },
  { id: 'rubygems', label: 'RubyGems', icon: '💎' },
  { id: 'maven', label: 'Maven', icon: '☕' },
  { id: 'composer', label: 'Composer', icon: '🎵' },
  { id: 'oci', label: 'OCI / Docker', icon: '🐳' },
];

export function ecosystemLabel(id) {
  return ECOSYSTEMS.find((e) => e.id === id?.toLowerCase())?.label || id;
}

export function ecosystemIcon(id) {
  return ECOSYSTEMS.find((e) => e.id === id?.toLowerCase())?.icon || '📦';
}
