import type { Release } from '../api/packages';

export const ARTIFACT_KIND_OPTIONS = [
  { value: 'tarball', label: 'Tarball' },
  { value: 'wheel', label: 'Wheel' },
  { value: 'sdist', label: 'Source distribution' },
  { value: 'crate', label: 'Crate' },
  { value: 'nupkg', label: 'NuGet package' },
  { value: 'snupkg', label: 'NuGet symbols package' },
  { value: 'jar', label: 'JAR' },
  { value: 'pom', label: 'POM' },
  { value: 'gem', label: 'Gem' },
  { value: 'composer_zip', label: 'Composer zip' },
  { value: 'oci_manifest', label: 'OCI manifest' },
  { value: 'oci_layer', label: 'OCI layer' },
  { value: 'checksum', label: 'Checksum' },
  { value: 'signature', label: 'Signature' },
  { value: 'sbom', label: 'SBOM' },
  { value: 'source_zip', label: 'Source zip' },
] as const;

type ReleaseLifecycleState = Pick<
  Release,
  'status' | 'is_yanked' | 'is_deprecated'
>;

export interface ReleaseActionAvailability {
  canUploadArtifact: boolean;
  canPublish: boolean;
  canYank: boolean;
  canRestore: boolean;
  canDeprecate: boolean;
  canUndeprecate: boolean;
}

export interface ReleaseReadiness {
  tone: 'success' | 'warning' | 'info';
  message: string;
}

export function getDefaultArtifactKindForEcosystem(ecosystem: string): string {
  switch (ecosystem.toLowerCase()) {
    case 'pypi':
      return 'wheel';
    case 'cargo':
      return 'crate';
    case 'nuget':
      return 'nupkg';
    case 'maven':
      return 'jar';
    case 'rubygems':
      return 'gem';
    case 'composer':
      return 'composer_zip';
    case 'oci':
      return 'oci_manifest';
    default:
      return 'tarball';
  }
}

export function getReleaseActionAvailability(
  release: ReleaseLifecycleState,
  artifactCount: number
): ReleaseActionAvailability {
  const status = normalizeStatus(release.status);
  const isFinalized = !matchesStatus(status, [
    'quarantine',
    'scanning',
    'deleted',
  ]);

  return {
    canUploadArtifact: matchesStatus(status, ['quarantine', 'scanning']),
    canPublish: matchesStatus(status, ['quarantine']) && artifactCount > 0,
    canYank: !release.is_yanked && isFinalized,
    canRestore: release.is_yanked === true,
    canDeprecate: !release.is_deprecated && isFinalized,
    canUndeprecate: release.is_deprecated === true,
  };
}

export function describeReleaseReadiness(
  release: Pick<Release, 'status' | 'is_yanked' | 'is_deprecated'>,
  artifactCount: number
): ReleaseReadiness {
  const status = normalizeStatus(release.status);

  if (matchesStatus(status, ['quarantine'])) {
    if (artifactCount === 0) {
      return {
        tone: 'warning',
        message: 'Upload at least one artifact before publishing this release.',
      };
    }

    return {
      tone: 'success',
      message:
        'This release is ready to publish once you confirm the uploaded artifacts.',
    };
  }

  if (matchesStatus(status, ['scanning'])) {
    return {
      tone: 'info',
      message:
        'This release is being scanned and will become readable after the checks finish.',
    };
  }

  if (release.is_yanked) {
    return {
      tone: 'warning',
      message:
        'This release is yanked. Restore it to return it to the default package history.',
    };
  }

  if (release.is_deprecated) {
    return {
      tone: 'info',
      message:
        'This release is deprecated and still readable so consumers can migrate deliberately.',
    };
  }

  return {
    tone: 'success',
    message: 'This release is published and available to readers.',
  };
}

export function getReleaseTimestampLabel(status?: string | null): string {
  return matchesStatus(normalizeStatus(status), ['quarantine', 'scanning'])
    ? 'Created'
    : 'Published';
}

export function getRestoreReleaseLabel(
  release: Pick<Release, 'is_deprecated'>
): string {
  return release.is_deprecated ? 'Restore to deprecated' : 'Restore release';
}

export function formatArtifactKindLabel(kind?: string | null): string {
  const normalized = normalizeStatus(kind);
  const known = ARTIFACT_KIND_OPTIONS.find(
    (option) => option.value === normalized
  );
  if (known) {
    return known.label;
  }

  return normalized
    .split(/[_-]+/)
    .filter(Boolean)
    .map((part) => {
      if (part === 'oci') {
        return 'OCI';
      }

      if (part === 'sbom') {
        return 'SBOM';
      }

      return part.charAt(0).toUpperCase() + part.slice(1);
    })
    .join(' ');
}

export function formatReleaseStatusLabel(status?: string | null): string {
  const normalized = normalizeStatus(status);

  if (!normalized) {
    return 'Unknown';
  }

  return normalized
    .split('_')
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(' ');
}

function matchesStatus(status: string, values: string[]): boolean {
  return values.includes(status);
}

function normalizeStatus(value?: string | null): string {
  return (value || '').trim().toLowerCase();
}
