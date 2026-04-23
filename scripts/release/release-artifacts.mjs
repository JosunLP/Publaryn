import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = join(__dirname, '..', '..');
const semverPattern =
  /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

function usage() {
  console.error(
    'Usage: bun scripts/release/release-artifacts.mjs --version <semver>'
  );
}

function parseArgs() {
  const args = process.argv.slice(2);
  const versionFlagIndex = args.indexOf('--version');

  if (versionFlagIndex === -1) {
    usage();
    process.exit(1);
  }

  const version = args[versionFlagIndex + 1];

  if (!version || !semverPattern.test(version)) {
    console.error(`Invalid version: ${version ?? '<missing>'}`);
    process.exit(1);
  }

  return version;
}

function requireContains(content, fragment, label) {
  if (!content.includes(fragment)) {
    throw new Error(`Missing ${label}: ${fragment}`);
  }
}

function main() {
  const version = parseArgs();
  const releaseNotesPath = join(repoRoot, 'docs', 'releases', `${version}.md`);
  const changelogPath = join(repoRoot, 'CHANGELOG.md');

  if (!existsSync(releaseNotesPath)) {
    throw new Error(`Missing release notes file: docs/releases/${version}.md`);
  }

  const releaseNotes = readFileSync(releaseNotesPath, 'utf8');
  const changelog = readFileSync(changelogPath, 'utf8');

  requireContains(changelog, `## [${version}]`, 'changelog section');
  requireContains(
    releaseNotes,
    '## Highlights',
    'release notes highlights section'
  );
  requireContains(
    releaseNotes,
    '## Supported in',
    'release notes supported section'
  );
  requireContains(
    releaseNotes,
    '## Explicitly not part of',
    'release notes unsupported section'
  );

  if (releaseNotes.includes('(Draft)')) {
    throw new Error(`Release notes still marked as draft: docs/releases/${version}.md`);
  }

  const plannedChangelogMarker = `## [${version}] - Planned`;
  if (changelog.includes(plannedChangelogMarker)) {
    throw new Error(
      `Changelog entry still marked as planned: CHANGELOG.md contains "${plannedChangelogMarker}".`
    );
  }

  console.log(`Release artifacts for ${version} are present.`);
}

main();
