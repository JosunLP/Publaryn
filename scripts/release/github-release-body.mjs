import { existsSync, readFileSync } from 'node:fs';
import path, { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { semverPattern } from './semver.mjs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = join(__dirname, '..', '..');
const docsRoot = join(repoRoot, 'docs');
const defaultDocsBaseUrl = 'https://josunlp.github.io/Publaryn';

function usage() {
  console.error(
    'Usage: bun scripts/release/github-release-body.mjs --version <semver> [--docs-base-url <url>]'
  );
}

function parseArgs() {
  const args = process.argv.slice(2);
  const versionFlagIndex = args.indexOf('--version');
  const docsBaseUrlFlagIndex = args.indexOf('--docs-base-url');

  if (versionFlagIndex === -1) {
    usage();
    process.exit(1);
  }

  const version = args[versionFlagIndex + 1];
  if (!version || !semverPattern.test(version)) {
    console.error(`Invalid version: ${version ?? '<missing>'}`);
    process.exit(1);
  }

  const docsBaseUrl =
    docsBaseUrlFlagIndex === -1
      ? defaultDocsBaseUrl
      : args[docsBaseUrlFlagIndex + 1];

  if (!docsBaseUrl) {
    console.error('Missing docs base URL.');
    process.exit(1);
  }

  return {
    version,
    docsBaseUrl: docsBaseUrl.replace(/\/+$/, ''),
  };
}

function docsPathToSitePath(relativeDocPath) {
  let normalized = relativeDocPath.replace(/\\/g, '/').replace(/\.md$/, '');

  if (normalized === 'index') {
    return '/';
  }

  if (normalized.endsWith('/README')) {
    normalized = normalized.slice(0, -'/README'.length);
  }

  return normalized ? `/${normalized}` : '/';
}

function absolutizeDocsLink(target, sourceDocPath, docsBaseUrl) {
  if (/^(?:[a-z]+:|#)/i.test(target)) {
    return target;
  }

  const [withoutHash, hash = ''] = target.split('#', 2);
  const [pathname, query = ''] = withoutHash.split('?', 2);

  let resolvedDocPath;

  if (pathname.startsWith('/')) {
    resolvedDocPath = pathname.slice(1);
  } else {
    resolvedDocPath = path.posix.normalize(
      path.posix.join(path.posix.dirname(sourceDocPath), pathname)
    );
  }

  const sitePath = docsPathToSitePath(resolvedDocPath);
  const suffix = `${query ? `?${query}` : ''}${hash ? `#${hash}` : ''}`;
  return `${docsBaseUrl}${sitePath}${suffix}`;
}

function rewriteMarkdownLinks(markdown, sourceDocPath, docsBaseUrl) {
  return markdown.replace(
    /(!?\[[^\]]*]\()([^)]+)(\))/g,
    (_, prefix, target, suffix) =>
      `${prefix}${absolutizeDocsLink(target, sourceDocPath, docsBaseUrl)}${suffix}`
  );
}

function main() {
  const { version, docsBaseUrl } = parseArgs();
  const sourceDocPath = `releases/${version}.md`;
  const releaseNotesPath = join(docsRoot, sourceDocPath);

  if (!existsSync(releaseNotesPath)) {
    throw new Error(`Missing release notes file: docs/${sourceDocPath}`);
  }

  const releaseNotes = readFileSync(releaseNotesPath, 'utf8');
  process.stdout.write(
    rewriteMarkdownLinks(releaseNotes, sourceDocPath, docsBaseUrl)
  );
}

main();
