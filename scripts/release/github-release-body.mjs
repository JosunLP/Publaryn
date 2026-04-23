import { existsSync, readFileSync } from 'node:fs';
import path, { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { semverPattern } from './semver.mjs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = join(__dirname, '..', '..');
const docsRoot = join(repoRoot, 'docs');
const fallbackDocsBaseUrl = 'https://josunlp.github.io/Publaryn';

function defaultDocsBaseUrl() {
  const repository = process.env.GITHUB_REPOSITORY;

  if (!repository) {
    return fallbackDocsBaseUrl;
  }

  const separatorIndex = repository.indexOf('/');

  if (separatorIndex === -1) {
    return fallbackDocsBaseUrl;
  }

  const owner = repository.slice(0, separatorIndex);
  const repo = repository.slice(separatorIndex + 1);

  if (!owner || !repo) {
    return fallbackDocsBaseUrl;
  }

  return `https://${owner.toLowerCase()}.github.io/${repo}`;
}

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
      ? defaultDocsBaseUrl()
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

function isWhitespace(char) {
  return /\s/.test(char);
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

function findClosingBracket(markdown, openBracketIndex) {
  let escape = false;
  let depth = 0;

  for (let index = openBracketIndex; index < markdown.length; index += 1) {
    const char = markdown[index];

    if (escape) {
      escape = false;
      continue;
    }

    if (char === '\\') {
      escape = true;
      continue;
    }

    if (char === '[') {
      depth += 1;
      continue;
    }

    if (char === ']') {
      depth -= 1;
      if (depth === 0) {
        return index;
      }
    }
  }

  return -1;
}

function findClosingParen(markdown, openParenIndex) {
  let escape = false;
  let depth = 1;
  let inAngle = false;
  let quoteChar = null;

  for (let index = openParenIndex + 1; index < markdown.length; index += 1) {
    const char = markdown[index];

    if (escape) {
      escape = false;
      continue;
    }

    if (char === '\\') {
      escape = true;
      continue;
    }

    if (quoteChar) {
      if (char === quoteChar) {
        quoteChar = null;
      }
      continue;
    }

    if (inAngle) {
      if (char === '>') {
        inAngle = false;
      }
      continue;
    }

    if (char === '<') {
      inAngle = true;
      continue;
    }

    if (char === '"' || char === "'") {
      quoteChar = char;
      continue;
    }

    if (char === '(') {
      depth += 1;
      continue;
    }

    if (char === ')') {
      depth -= 1;
      if (depth === 0) {
        return index;
      }
    }
  }

  return -1;
}

function splitMarkdownLinkTarget(content) {
  let index = 0;

  while (index < content.length && isWhitespace(content[index])) {
    index += 1;
  }

  if (index >= content.length) {
    return null;
  }

  if (content[index] === '<') {
    let escape = false;

    for (let end = index + 1; end < content.length; end += 1) {
      const char = content[end];

      if (escape) {
        escape = false;
        continue;
      }

      if (char === '\\') {
        escape = true;
        continue;
      }

      if (char === '>') {
        return {
          leadingWhitespace: content.slice(0, index),
          target: content.slice(index + 1, end),
          suffix: content.slice(end + 1),
          wrapInAngles: true,
        };
      }
    }

    return null;
  }

  let escape = false;
  let depth = 0;
  let end = index;

  for (; end < content.length; end += 1) {
    const char = content[end];

    if (escape) {
      escape = false;
      continue;
    }

    if (char === '\\') {
      escape = true;
      continue;
    }

    if (isWhitespace(char) && depth === 0) {
      break;
    }

    if (char === '(') {
      depth += 1;
      continue;
    }

    if (char === ')') {
      if (depth === 0) {
        break;
      }
      depth -= 1;
    }
  }

  return {
    leadingWhitespace: content.slice(0, index),
    target: content.slice(index, end),
    suffix: content.slice(end),
    wrapInAngles: false,
  };
}

function rewriteMarkdownLinkContent(content, sourceDocPath, docsBaseUrl) {
  const parsed = splitMarkdownLinkTarget(content);

  if (!parsed) {
    return content;
  }

  const rewrittenTarget = absolutizeDocsLink(
    parsed.target,
    sourceDocPath,
    docsBaseUrl
  );
  const renderedTarget = parsed.wrapInAngles
    ? `<${rewrittenTarget}>`
    : rewrittenTarget;
  return `${parsed.leadingWhitespace}${renderedTarget}${parsed.suffix}`;
}

function rewriteMarkdownLinks(markdown, sourceDocPath, docsBaseUrl) {
  let rewritten = '';
  let index = 0;

  while (index < markdown.length) {
    const isImageLink = markdown[index] === '!' && markdown[index + 1] === '[';
    const isLink = markdown[index] === '[';

    if (!isImageLink && !isLink) {
      rewritten += markdown[index];
      index += 1;
      continue;
    }

    const openBracketIndex = isImageLink ? index + 1 : index;
    const closeBracketIndex = findClosingBracket(markdown, openBracketIndex);

    if (
      closeBracketIndex === -1 ||
      markdown[closeBracketIndex + 1] !== '('
    ) {
      rewritten += markdown[index];
      index += 1;
      continue;
    }

    const closeParenIndex = findClosingParen(markdown, closeBracketIndex + 1);

    if (closeParenIndex === -1) {
      rewritten += markdown[index];
      index += 1;
      continue;
    }

    const prefix = markdown.slice(index, closeBracketIndex + 2);
    const content = markdown.slice(closeBracketIndex + 2, closeParenIndex);
    rewritten += `${prefix}${rewriteMarkdownLinkContent(content, sourceDocPath, docsBaseUrl)})`;
    index = closeParenIndex + 1;
  }

  return rewritten;
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
