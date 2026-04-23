import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { semverPattern } from './semver.mjs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = join(__dirname, '..', '..');

const packageJsonFiles = ['frontend/package.json', 'docs/package.json'];

function usage() {
  console.error(
    'Usage: bun scripts/release/manifest-version.mjs <check|set> --version <semver>'
  );
}

function parseArgs() {
  const [mode, ...rest] = process.argv.slice(2);
  const versionFlagIndex = rest.indexOf('--version');

  if (!mode || !['check', 'set'].includes(mode) || versionFlagIndex === -1) {
    usage();
    process.exit(1);
  }

  const version = rest[versionFlagIndex + 1];

  if (!version || !semverPattern.test(version)) {
    console.error(`Invalid version: ${version ?? '<missing>'}`);
    process.exit(1);
  }

  return { mode, version };
}

function collectCargoTomls(directory) {
  const entries = readdirSync(directory, { withFileTypes: true });
  const files = [];

  for (const entry of entries) {
    const absolutePath = join(directory, entry.name);

    if (entry.isDirectory()) {
      files.push(...collectCargoTomls(absolutePath));
      continue;
    }

    if (entry.isFile() && entry.name === 'Cargo.toml') {
      files.push(absolutePath);
    }
  }

  return files.sort();
}

function cargoVersion(filePath) {
  const content = readFileSync(filePath, 'utf8');
  const match = content.match(/^version\s*=\s*"([^"]+)"\s*$/m);

  if (!match) {
    throw new Error(
      `No crate version found in ${relative(repoRoot, filePath)}`
    );
  }

  return match[1];
}

function setCargoVersion(filePath, version) {
  const content = readFileSync(filePath, 'utf8');
  const updated = content.replace(
    /^version\s*=\s*"([^"]+)"\s*$/m,
    `version = "${version}"`
  );

  if (updated === content) {
    throw new Error(
      `Failed to update crate version in ${relative(repoRoot, filePath)}`
    );
  }

  writeFileSync(filePath, updated, 'utf8');
}

function packageVersion(filePath) {
  return JSON.parse(readFileSync(filePath, 'utf8')).version;
}

function setPackageVersion(filePath, version) {
  const packageJson = JSON.parse(readFileSync(filePath, 'utf8'));
  packageJson.version = version;
  writeFileSync(filePath, `${JSON.stringify(packageJson, null, 2)}\n`, 'utf8');
}

function main() {
  const { mode, version } = parseArgs();
  const cargoFiles = collectCargoTomls(join(repoRoot, 'crates'));
  const packageFiles = packageJsonFiles.map((file) => join(repoRoot, file));
  const mismatches = [];

  for (const filePath of cargoFiles) {
    const currentVersion = cargoVersion(filePath);

    if (mode === 'check') {
      if (currentVersion !== version) {
        mismatches.push(`${relative(repoRoot, filePath)} -> ${currentVersion}`);
      }
    } else {
      setCargoVersion(filePath, version);
    }
  }

  for (const filePath of packageFiles) {
    const currentVersion = packageVersion(filePath);

    if (mode === 'check') {
      if (currentVersion !== version) {
        mismatches.push(`${relative(repoRoot, filePath)} -> ${currentVersion}`);
      }
    } else {
      setPackageVersion(filePath, version);
    }
  }

  if (mode === 'check' && mismatches.length > 0) {
    console.error(`Manifest versions do not match ${version}:`);
    for (const mismatch of mismatches) {
      console.error(`- ${mismatch}`);
    }
    process.exit(1);
  }

  console.log(
    mode === 'check'
      ? `All manifest versions match ${version}.`
      : `Updated manifest versions to ${version}.`
  );
}

main();
