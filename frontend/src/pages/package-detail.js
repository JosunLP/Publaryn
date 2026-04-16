import { getPackage, listReleases, listTags } from '../api/packages.js';
import {
  ecosystemIcon,
  ecosystemLabel,
  installCommand,
} from '../utils/ecosystem.js';
import {
  copyToClipboard,
  escapeHtml,
  formatDate,
  formatNumber,
} from '../utils/format.js';
import { renderMarkdown } from '../utils/markdown.js';

export function packageDetailPage({ params, query }, container) {
  const { ecosystem, name } = params;
  container.innerHTML = `<div class="loading"><span class="spinner"></span> Loading…</div>`;
  render(container, ecosystem, name);
}

async function render(container, ecosystem, name) {
  let pkg;
  try {
    pkg = await getPackage(ecosystem, name);
  } catch (err) {
    if (err.status === 404) {
      container.innerHTML = `
        <div class="empty-state mt-6">
          <h2>Package not found</h2>
          <p>${escapeHtml(ecosystem)}/${escapeHtml(name)} does not exist or is not public.</p>
          <a href="/search" class="btn btn-primary mt-4">Search packages</a>
        </div>`;
      return;
    }
    container.innerHTML = `<div class="alert alert-error mt-6">Failed to load package: ${escapeHtml(err.message)}</div>`;
    return;
  }

  // Load supplementary data in parallel
  const [releases, tags] = await Promise.all([
    listReleases(ecosystem, name, { perPage: 20 }).catch(() => []),
    listTags(ecosystem, name).catch(() => []),
  ]);

  const latestVersion =
    pkg.latest_version || (releases.length > 0 ? releases[0].version : null);
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
          <!-- Install command -->
          <div class="card mb-4">
            <h3 style="font-size:0.8125rem; font-weight:600; color:var(--color-text-muted); text-transform:uppercase; letter-spacing:0.05em; margin-bottom:8px;">Install</h3>
            <div class="code-block">
              <code id="install-cmd">${escapeHtml(install)}</code>
              <button class="copy-btn" id="copy-install-btn">Copy</button>
            </div>
          </div>

          <!-- Tabs: Readme / Versions -->
          <div class="tabs">
            <div class="tab active" data-tab="readme">Readme</div>
            <div class="tab" data-tab="versions">Versions (${Array.isArray(releases) ? releases.length : 0})</div>
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

  // Copy button
  const copyBtn = container.querySelector('#copy-install-btn');
  if (copyBtn) {
    copyBtn.addEventListener('click', async () => {
      const ok = await copyToClipboard(install);
      copyBtn.textContent = ok ? 'Copied!' : 'Failed';
      setTimeout(() => {
        copyBtn.textContent = 'Copy';
      }, 2000);
    });
  }

  // Tab switching
  container.querySelectorAll('.tab').forEach((tab) => {
    tab.addEventListener('click', () => {
      container
        .querySelectorAll('.tab')
        .forEach((t) => t.classList.remove('active'));
      tab.classList.add('active');
      const target = tab.dataset.tab;
      container.querySelector('#tab-readme').style.display =
        target === 'readme' ? '' : 'none';
      container.querySelector('#tab-versions').style.display =
        target === 'versions' ? '' : 'none';
    });
  });
}

function renderReleasesTable(releases, ecosystem, name) {
  if (!Array.isArray(releases) || releases.length === 0) {
    return '<div class="empty-state"><p>No releases yet.</p></div>';
  }
  return releases
    .map(
      (r) => `
    <div class="release-row">
      <div>
        <a href="/packages/${encodeURIComponent(ecosystem)}/${encodeURIComponent(name)}/versions/${encodeURIComponent(r.version)}" class="release-row__version">${escapeHtml(r.version)}</a>
        ${r.is_yanked ? '<span class="badge badge-yanked">yanked</span>' : ''}
        ${r.status === 'deprecated' ? '<span class="badge badge-deprecated">deprecated</span>' : ''}
      </div>
      <div class="release-row__meta">
        ${r.published_at ? formatDate(r.published_at) : formatDate(r.created_at)}
      </div>
    </div>`
    )
    .join('');
}

function renderSidebar(pkg, tags) {
  const sections = [];

  // Metadata
  const meta = [];
  if (pkg.license) meta.push(row('License', escapeHtml(pkg.license)));
  if (pkg.visibility) meta.push(row('Visibility', escapeHtml(pkg.visibility)));
  if (pkg.download_count != null)
    meta.push(row('Downloads', formatNumber(pkg.download_count)));
  if (pkg.created_at) meta.push(row('Created', formatDate(pkg.created_at)));
  if (pkg.updated_at) meta.push(row('Updated', formatDate(pkg.updated_at)));
  if (meta.length > 0) {
    sections.push(
      `<div class="card"><div class="sidebar-section"><h3>Package Info</h3>${meta.join('')}</div></div>`
    );
  }

  // Owner
  if (pkg.owner_username || pkg.owner_org_slug) {
    const ownerName = pkg.owner_username || pkg.owner_org_slug;
    const ownerLink = pkg.owner_org_slug
      ? `/orgs/${encodeURIComponent(pkg.owner_org_slug)}`
      : `/search?q=${encodeURIComponent(ownerName)}`;
    sections.push(
      `<div class="card"><div class="sidebar-section"><h3>Owner</h3><a href="${ownerLink}">${escapeHtml(ownerName)}</a></div></div>`
    );
  }

  // Links
  const links = [];
  if (pkg.homepage)
    links.push(
      `<a href="${escapeHtml(pkg.homepage)}" target="_blank" rel="noopener noreferrer">Homepage</a>`
    );
  if (pkg.repository_url)
    links.push(
      `<a href="${escapeHtml(pkg.repository_url)}" target="_blank" rel="noopener noreferrer">Repository</a>`
    );
  if (links.length > 0) {
    sections.push(
      `<div class="card"><div class="sidebar-section"><h3>Links</h3>${links.map((l) => `<div style="margin-bottom:4px;">${l}</div>`).join('')}</div></div>`
    );
  }

  // Keywords
  if (pkg.keywords && pkg.keywords.length > 0) {
    const kw = pkg.keywords
      .map(
        (k) =>
          `<a href="/search?q=${encodeURIComponent(k)}" class="badge badge-ecosystem" style="margin:2px;">${escapeHtml(k)}</a>`
      )
      .join(' ');
    sections.push(
      `<div class="card"><div class="sidebar-section"><h3>Keywords</h3><div>${kw}</div></div></div>`
    );
  }

  // Tags / dist-tags
  if (Array.isArray(tags) && tags.length > 0) {
    const tagList = tags
      .map((t) => row(escapeHtml(t.tag || t.name), escapeHtml(t.version)))
      .join('');
    sections.push(
      `<div class="card"><div class="sidebar-section"><h3>Tags</h3>${tagList}</div></div>`
    );
  }

  return sections.join('');
}

function row(label, value) {
  return `<div class="sidebar-row"><span class="sidebar-row__label">${label}</span><span class="sidebar-row__value">${value}</span></div>`;
}
