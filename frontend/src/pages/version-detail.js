import { getRelease, listArtifacts } from '../api/packages.js';
import {
  ecosystemIcon,
  ecosystemLabel,
  installCommand,
} from '../utils/ecosystem.js';
import { copyToClipboard, escapeHtml, formatDate } from '../utils/format.js';

export function versionDetailPage({ params }, container) {
  const { ecosystem, name, version } = params;
  container.innerHTML = `<div class="loading"><span class="spinner"></span> Loading…</div>`;
  render(container, ecosystem, name, version);
}

async function render(container, ecosystem, name, version) {
  let release;
  try {
    release = await getRelease(ecosystem, name, version);
  } catch (err) {
    if (err.status === 404) {
      container.innerHTML = `
        <div class="empty-state mt-6">
          <h2>Version not found</h2>
          <p>${escapeHtml(ecosystem)}/${escapeHtml(name)}@${escapeHtml(version)} does not exist.</p>
          <a href="/packages/${encodeURIComponent(ecosystem)}/${encodeURIComponent(name)}" class="btn btn-primary mt-4">Back to package</a>
        </div>`;
      return;
    }
    container.innerHTML = `<div class="alert alert-error mt-6">Failed to load version: ${escapeHtml(err.message)}</div>`;
    return;
  }

  let artifacts = [];
  try {
    artifacts = await listArtifacts(ecosystem, name, version);
    if (!Array.isArray(artifacts)) artifacts = [];
  } catch {
    // Non-critical — page still renders without artifacts
  }

  const install = installCommand(ecosystem, name, version);

  container.innerHTML = `
    <div class="mt-6">
      <nav style="font-size:0.875rem; margin-bottom:16px;">
        <a href="/packages/${encodeURIComponent(ecosystem)}/${encodeURIComponent(name)}">${ecosystemIcon(ecosystem)} ${escapeHtml(name)}</a>
        &rsaquo; <span style="color:var(--color-text-secondary);">v${escapeHtml(version)}</span>
      </nav>

      <div class="pkg-header">
        <h1 class="pkg-header__name">${escapeHtml(name)}</h1>
        <span class="badge badge-ecosystem">${ecosystemIcon(ecosystem)} ${ecosystemLabel(ecosystem)}</span>
        <span class="pkg-header__version">v${escapeHtml(version)}</span>
        ${release.is_yanked ? '<span class="badge badge-yanked">yanked</span>' : ''}
        ${release.status === 'deprecated' ? '<span class="badge badge-deprecated">deprecated</span>' : ''}
      </div>

      <!-- Install -->
      <div class="card mt-4 mb-4">
        <h3 style="font-size:0.8125rem; font-weight:600; color:var(--color-text-muted); text-transform:uppercase; letter-spacing:0.05em; margin-bottom:8px;">Install</h3>
        <div class="code-block">
          <code id="install-cmd">${escapeHtml(install)}</code>
          <button class="copy-btn" id="copy-install-btn">Copy</button>
        </div>
      </div>

      <div class="pkg-detail">
        <div class="pkg-detail__main">
          ${release.description ? `<div class="card mb-4"><h3 style="margin-bottom:8px;">Description</h3><p>${escapeHtml(release.description)}</p></div>` : ''}
          ${release.changelog ? `<div class="card mb-4"><h3 style="margin-bottom:8px;">Changelog</h3><pre style="white-space:pre-wrap;">${escapeHtml(release.changelog)}</pre></div>` : ''}

          ${renderArtifactsSection(artifacts, ecosystem, name, version)}
        </div>

        <div class="pkg-detail__sidebar">
          ${renderVersionSidebar(release)}
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
}

function renderArtifactsSection(artifacts, ecosystem, name, version) {
  if (artifacts.length === 0) {
    return '<div class="card"><div class="empty-state"><p>No artifacts available.</p></div></div>';
  }

  const rows = artifacts
    .map(
      (a) => `
    <div class="release-row">
      <div>
        <a href="/v1/packages/${encodeURIComponent(ecosystem)}/${encodeURIComponent(name)}/releases/${encodeURIComponent(version)}/artifacts/${encodeURIComponent(a.filename)}"
           target="_blank" rel="noopener noreferrer"
           class="release-row__version">
          ${escapeHtml(a.filename)}
        </a>
        <span class="text-muted" style="font-size:0.8125rem; margin-left:8px;">${escapeHtml(a.content_type || '')}</span>
      </div>
      <div class="release-row__meta">
        ${a.size_bytes != null ? formatFileSize(a.size_bytes) : ''}
      </div>
    </div>`
    )
    .join('');

  return `<div class="card" style="padding:0;"><div style="padding:16px 20px 8px;"><h3 style="font-size:0.875rem; font-weight:600;">Artifacts</h3></div>${rows}</div>`;
}

function renderVersionSidebar(release) {
  const meta = [];
  if (release.status) meta.push(row('Status', escapeHtml(release.status)));
  if (release.published_at)
    meta.push(row('Published', formatDate(release.published_at)));
  if (release.created_at)
    meta.push(row('Created', formatDate(release.created_at)));
  if (release.is_prerelease) meta.push(row('Pre-release', 'Yes'));

  if (release.sha256)
    meta.push(
      row(
        'SHA-256',
        `<code style="font-size:0.75rem; word-break:break-all;">${escapeHtml(release.sha256.substring(0, 16))}…</code>`
      )
    );

  return meta.length > 0
    ? `<div class="card"><div class="sidebar-section"><h3>Version Info</h3>${meta.join('')}</div></div>`
    : '';
}

function row(label, value) {
  return `<div class="sidebar-row"><span class="sidebar-row__label">${label}</span><span class="sidebar-row__value">${value}</span></div>`;
}

function formatFileSize(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
