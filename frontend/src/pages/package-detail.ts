import { ApiError } from '../api/client';
import type { PackageDetail, Release, Tag } from '../api/packages';
import { getPackage, listReleases, listTags } from '../api/packages';
import type { RouteContext } from '../router';
import {
  ecosystemIcon,
  ecosystemLabel,
  installCommand,
} from '../utils/ecosystem';
import {
  copyToClipboard,
  escapeHtml,
  formatDate,
  formatNumber,
} from '../utils/format';
import { renderMarkdown } from '../utils/markdown';

export function packageDetailPage(
  { params }: RouteContext,
  container: HTMLElement
): void {
  const ecosystem = params.ecosystem ?? '';
  const name = params.name ?? '';

  container.innerHTML = `<div class="loading"><span class="spinner"></span> Loading…</div>`;
  void render(container, ecosystem, name);
}

async function render(
  container: HTMLElement,
  ecosystem: string,
  name: string
): Promise<void> {
  let pkg: PackageDetail;

  try {
    pkg = await getPackage(ecosystem, name);
  } catch (caughtError: unknown) {
    if (caughtError instanceof ApiError && caughtError.status === 404) {
      container.innerHTML = `
        <div class="empty-state mt-6">
          <h2>Package not found</h2>
          <p>${escapeHtml(ecosystem)}/${escapeHtml(name)} does not exist or is not public.</p>
          <a href="/search" class="btn btn-primary mt-4">Search packages</a>
        </div>`;
      return;
    }

    const message =
      caughtError instanceof Error
        ? caughtError.message
        : 'Failed to load package.';

    container.innerHTML = `<div class="alert alert-error mt-6">Failed to load package: ${escapeHtml(message)}</div>`;
    return;
  }

  const [releases, tags] = await Promise.all([
    listReleases(ecosystem, name, { perPage: 20 }).catch(() => [] as Release[]),
    listTags(ecosystem, name).catch(() => [] as Tag[]),
  ]);

  const latestVersion =
    pkg.latest_version ??
    (releases.length > 0 ? (releases[0]?.version ?? null) : null);
  const install = latestVersion
    ? installCommand(ecosystem, pkg.name, latestVersion)
    : installCommand(ecosystem, pkg.name);
  const readmeHtml = renderMarkdown(pkg.readme);

  container.innerHTML = `
    <div class="mt-6">
      <div class="pkg-header">
        <h1 class="pkg-header__name">${escapeHtml(pkg.display_name || pkg.name)}</h1>
        <span class="badge badge-ecosystem">${ecosystemIcon(ecosystem)} ${ecosystemLabel(ecosystem)}</span>
        ${latestVersion ? `<span class="pkg-header__version">v${escapeHtml(latestVersion)}</span>` : ''}
        ${pkg.is_deprecated ? '<span class="badge badge-deprecated">deprecated</span>' : ''}
        ${pkg.is_archived ? '<span class="badge badge-yanked">archived</span>' : ''}
      </div>

      ${
        pkg.description
          ? `<p class="text-muted mt-4" style="font-size:1.05rem;">${escapeHtml(pkg.description)}</p>`
          : ''
      }

      <div class="pkg-detail">
        <div class="pkg-detail__main">
          <div class="card mb-4">
            <h3 style="font-size:0.8125rem; font-weight:600; color:var(--color-text-muted); text-transform:uppercase; letter-spacing:0.05em; margin-bottom:8px;">Install</h3>
            <div class="code-block">
              <code id="install-cmd">${escapeHtml(install)}</code>
              <button class="copy-btn" id="copy-install-btn">Copy</button>
            </div>
          </div>

          <div class="tabs">
            <div class="tab active" data-tab="readme">Readme</div>
            <div class="tab" data-tab="versions">Versions (${releases.length})</div>
          </div>

          <div id="tab-readme">
            ${
              readmeHtml
                ? `<div class="readme-content">${readmeHtml}</div>`
                : `<div class="empty-state"><p>No README available for this package.</p></div>`
            }
          </div>

          <div id="tab-versions" style="display:none;">
            ${renderReleasesTable(releases, ecosystem, name)}
          </div>
        </div>

        <div class="pkg-detail__sidebar">
          ${renderSidebar(pkg, tags)}
        </div>
      </div>
    </div>
  `;

  const copyButton =
    container.querySelector<HTMLButtonElement>('#copy-install-btn');
  copyButton?.addEventListener('click', async () => {
    const copied = await copyToClipboard(install);
    copyButton.textContent = copied ? 'Copied!' : 'Failed';
    window.setTimeout(() => {
      copyButton.textContent = 'Copy';
    }, 2000);
  });

  container.querySelectorAll<HTMLElement>('.tab').forEach((tab) => {
    tab.addEventListener('click', () => {
      container.querySelectorAll<HTMLElement>('.tab').forEach((item) => {
        item.classList.remove('active');
      });

      tab.classList.add('active');

      const target = tab.dataset.tab;
      const readmePanel = container.querySelector<HTMLElement>('#tab-readme');
      const versionsPanel =
        container.querySelector<HTMLElement>('#tab-versions');

      if (!readmePanel || !versionsPanel) {
        return;
      }

      readmePanel.style.display = target === 'readme' ? '' : 'none';
      versionsPanel.style.display = target === 'versions' ? '' : 'none';
    });
  });
}

function renderReleasesTable(
  releases: Release[],
  ecosystem: string,
  name: string
): string {
  if (releases.length === 0) {
    return '<div class="empty-state"><p>No releases yet.</p></div>';
  }

  return releases
    .map(
      (release) => `
    <div class="release-row">
      <div>
        <a href="/packages/${encodeURIComponent(ecosystem)}/${encodeURIComponent(name)}/versions/${encodeURIComponent(release.version)}" class="release-row__version">${escapeHtml(release.version)}</a>
        ${release.is_yanked ? '<span class="badge badge-yanked">yanked</span>' : ''}
        ${release.status === 'deprecated' ? '<span class="badge badge-deprecated">deprecated</span>' : ''}
      </div>
      <div class="release-row__meta">
        ${release.published_at ? formatDate(release.published_at) : formatDate(release.created_at)}
      </div>
    </div>`
    )
    .join('');
}

function renderSidebar(pkg: PackageDetail, tags: Tag[]): string {
  const sections: string[] = [];
  const metadata: string[] = [];

  if (pkg.license) {
    metadata.push(row('License', escapeHtml(pkg.license)));
  }

  if (pkg.visibility) {
    metadata.push(row('Visibility', escapeHtml(pkg.visibility)));
  }

  if (pkg.download_count != null) {
    metadata.push(row('Downloads', formatNumber(pkg.download_count)));
  }

  if (pkg.created_at) {
    metadata.push(row('Created', formatDate(pkg.created_at)));
  }

  if (pkg.updated_at) {
    metadata.push(row('Updated', formatDate(pkg.updated_at)));
  }

  if (metadata.length > 0) {
    sections.push(
      `<div class="card"><div class="sidebar-section"><h3>Package Info</h3>${metadata.join('')}</div></div>`
    );
  }

  if (pkg.owner_username || pkg.owner_org_slug) {
    const ownerName = pkg.owner_username || pkg.owner_org_slug || '';
    const ownerLink = pkg.owner_org_slug
      ? `/orgs/${encodeURIComponent(pkg.owner_org_slug)}`
      : `/search?q=${encodeURIComponent(ownerName)}`;

    sections.push(
      `<div class="card"><div class="sidebar-section"><h3>Owner</h3><a href="${ownerLink}">${escapeHtml(ownerName)}</a></div></div>`
    );
  }

  const links: string[] = [];

  if (pkg.homepage) {
    links.push(
      `<a href="${escapeHtml(pkg.homepage)}" target="_blank" rel="noopener noreferrer">Homepage</a>`
    );
  }

  if (pkg.repository_url) {
    links.push(
      `<a href="${escapeHtml(pkg.repository_url)}" target="_blank" rel="noopener noreferrer">Repository</a>`
    );
  }

  if (links.length > 0) {
    sections.push(
      `<div class="card"><div class="sidebar-section"><h3>Links</h3>${links.map((link) => `<div style="margin-bottom:4px;">${link}</div>`).join('')}</div></div>`
    );
  }

  if (pkg.keywords && pkg.keywords.length > 0) {
    const keywords = pkg.keywords
      .map(
        (keyword) =>
          `<a href="/search?q=${encodeURIComponent(keyword)}" class="badge badge-ecosystem" style="margin:2px;">${escapeHtml(keyword)}</a>`
      )
      .join(' ');

    sections.push(
      `<div class="card"><div class="sidebar-section"><h3>Keywords</h3><div>${keywords}</div></div></div>`
    );
  }

  if (tags.length > 0) {
    const tagList = tags
      .map((tag) =>
        row(escapeHtml(tag.tag || tag.name || ''), escapeHtml(tag.version))
      )
      .join('');

    sections.push(
      `<div class="card"><div class="sidebar-section"><h3>Tags</h3>${tagList}</div></div>`
    );
  }

  return sections.join('');
}

function row(label: string, value: string): string {
  return `<div class="sidebar-row"><span class="sidebar-row__label">${label}</span><span class="sidebar-row__value">${value}</span></div>`;
}
