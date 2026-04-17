import { ApiError, getAuthToken } from '../api/client';
import type { OrganizationMembership } from '../api/orgs';
import { listMyOrganizations } from '../api/orgs';
import type {
  PackageDetail,
  Release,
  SecurityFinding,
  Tag,
} from '../api/packages';
import {
  getPackage,
  listReleases,
  listSecurityFindings,
  listTags,
  severityLevel,
  transferPackageOwnership,
} from '../api/packages';
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
import { selectPackageTransferTargets } from '../utils/package-transfer';

interface RenderOptions {
  transferNotice?: string | null;
  transferError?: string | null;
}

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
  name: string,
  { transferNotice = null, transferError = null }: RenderOptions = {}
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

  const [releases, tags, findings] = await Promise.all([
    listReleases(ecosystem, name, { perPage: 20 }).catch(() => [] as Release[]),
    listTags(ecosystem, name).catch(() => [] as Tag[]),
    listSecurityFindings(ecosystem, name).catch(() => [] as SecurityFinding[]),
  ]);
  const transferState = await loadTransferState(pkg);

  const openFindings = findings.filter((f) => !f.is_resolved);
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
            <div class="tab" data-tab="security">Security${openFindings.length > 0 ? ` <span class="badge badge-severity-${worstSeverity(openFindings)}" style="margin-left:4px;">${openFindings.length}</span>` : ''}</div>
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

          <div id="tab-security" style="display:none;">
            ${renderFindingsPanel(findings)}
          </div>
        </div>

        <div class="pkg-detail__sidebar">
          ${renderSidebar(
            pkg,
            tags,
            renderSecuritySummary(openFindings),
            renderTransferCard(pkg, transferState, {
              notice: transferNotice,
              error: transferError,
            })
          )}
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
      const securityPanel =
        container.querySelector<HTMLElement>('#tab-security');

      if (!readmePanel || !versionsPanel || !securityPanel) {
        return;
      }

      readmePanel.style.display = target === 'readme' ? '' : 'none';
      versionsPanel.style.display = target === 'versions' ? '' : 'none';
      securityPanel.style.display = target === 'security' ? '' : 'none';
    });
  });

  const resolvedToggle = container.querySelector<HTMLInputElement>(
    '#findings-show-resolved'
  );
  resolvedToggle?.addEventListener('change', async () => {
    const includeResolved = resolvedToggle.checked;
    const refreshedFindings = await listSecurityFindings(ecosystem, name, {
      includeResolved,
    }).catch(() => [] as SecurityFinding[]);
    const findingsContainer =
      container.querySelector<HTMLElement>('#findings-list');
    if (findingsContainer) {
      findingsContainer.innerHTML = renderFindingsList(refreshedFindings);
    }
  });

  const transferForm = container.querySelector<HTMLFormElement>(
    '#package-transfer-form'
  );
  transferForm?.addEventListener('submit', async (event) => {
    event.preventDefault();

    const formData = new FormData(transferForm);
    const targetOrgSlug =
      formData.get('target_org_slug')?.toString().trim() || '';
    const confirmBox = transferForm.querySelector<HTMLInputElement>(
      '#package-transfer-confirm'
    );

    if (!targetOrgSlug) {
      await render(container, ecosystem, name, {
        transferError: 'Select an organization to receive this package.',
      });
      return;
    }

    if (!confirmBox?.checked) {
      await render(container, ecosystem, name, {
        transferError:
          'Please confirm that you understand this transfer is immediate and revokes existing team grants.',
      });
      return;
    }

    const submitButton = getSubmitButton(transferForm);
    if (!submitButton) {
      return;
    }

    submitButton.disabled = true;
    submitButton.textContent = 'Transferring…';

    try {
      const result = await transferPackageOwnership(ecosystem, name, {
        targetOrgSlug,
      });
      const targetLabel =
        result.owner?.name || result.owner?.slug || targetOrgSlug;

      await render(container, ecosystem, name, {
        transferNotice: `Package ownership transferred to ${targetLabel}.`,
      });
    } catch (caughtError: unknown) {
      await render(container, ecosystem, name, {
        transferError: toErrorMessage(
          caughtError,
          'Failed to transfer package ownership.'
        ),
      });
    }
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

function renderSidebar(
  pkg: PackageDetail,
  tags: Tag[],
  securityCard: string,
  transferCard: string
): string {
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

  if (securityCard) {
    sections.push(securityCard);
  }

  if (transferCard) {
    sections.push(transferCard);
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

async function loadTransferState(pkg: PackageDetail): Promise<{
  showTransfer: boolean;
  organizations: OrganizationMembership[];
  loadError: string | null;
}> {
  if (!getAuthToken() || pkg.can_transfer !== true) {
    return {
      showTransfer: false,
      organizations: [],
      loadError: null,
    };
  }

  try {
    const response = await listMyOrganizations();
    const organizations = selectPackageTransferTargets(
      response.organizations || [],
      pkg.owner_org_slug
    );

    return {
      showTransfer: true,
      organizations,
      loadError: null,
    };
  } catch (caughtError: unknown) {
    return {
      showTransfer: true,
      organizations: [],
      loadError: toErrorMessage(
        caughtError,
        'Failed to load your organizations for package transfer.'
      ),
    };
  }
}

function renderTransferCard(
  pkg: PackageDetail,
  transferState: {
    showTransfer: boolean;
    organizations: OrganizationMembership[];
    loadError: string | null;
  },
  {
    notice,
    error,
  }: {
    notice: string | null;
    error: string | null;
  }
): string {
  if (!transferState.showTransfer) {
    return '';
  }

  const ownerName =
    pkg.owner_org_slug || pkg.owner_username || 'the current owner';
  const organizationOptions = transferState.organizations
    .map(
      (organization) => `
        <option value="${escapeHtml(organization.slug || '')}">
          ${escapeHtml(organization.name || organization.slug || 'Unnamed organization')}
        </option>
      `
    )
    .join('');

  return `
    <div class="card">
      <div class="sidebar-section">
        <h3>Transfer ownership</h3>
        <div class="alert alert-warning" style="margin-bottom:12px;">
          This transfer is immediate and revokes existing team grants on the package.
        </div>
        ${notice ? `<div class="alert alert-success" style="margin-bottom:12px;">${escapeHtml(notice)}</div>` : ''}
        ${error ? `<div class="alert alert-error" style="margin-bottom:12px;">${escapeHtml(error)}</div>` : ''}
        ${
          transferState.loadError
            ? `<div class="alert alert-error" style="margin-bottom:12px;">${escapeHtml(transferState.loadError)}</div>`
            : ''
        }
        <p class="settings-copy" style="margin-bottom:12px;">
          Move this package away from ${escapeHtml(ownerName)} into an organization you already administer.
        </p>
        ${
          transferState.organizations.length === 0
            ? `<p class="settings-copy" style="margin-bottom:0;">You can transfer this package, but you do not currently administer another organization that can receive it.</p>`
            : `
                <form id="package-transfer-form">
                  <div class="form-group" style="margin-bottom:12px;">
                    <label for="package-transfer-target">Target organization</label>
                    <select id="package-transfer-target" name="target_org_slug" class="form-input" required>
                      <option value="">Select an organization</option>
                      ${organizationOptions}
                    </select>
                  </div>
                  <div class="form-group" style="margin-bottom:12px;">
                    <label class="flex items-start gap-2">
                      <input type="checkbox" id="package-transfer-confirm" name="confirm" required />
                      <span>I understand this package transfer is immediate and existing team grants will be removed.</span>
                    </label>
                  </div>
                  <button type="submit" class="btn btn-danger" style="width:100%; justify-content:center;">Transfer package</button>
                </form>
              `
        }
      </div>
    </div>
  `;
}

function getSubmitButton(form: HTMLFormElement): HTMLButtonElement | null {
  return form.querySelector<HTMLButtonElement>('button[type="submit"]');
}

function toErrorMessage(caughtError: unknown, fallback: string): string {
  if (caughtError instanceof ApiError) {
    return caughtError.message;
  }

  if (caughtError instanceof Error && caughtError.message.trim()) {
    return caughtError.message;
  }

  return fallback;
}

/* ── Security findings helpers ─────────────────────────── */

function worstSeverity(findings: SecurityFinding[]): string {
  if (findings.length === 0) {
    return 'info';
  }

  let worst = 'info';
  let worstLevel = -1;

  for (const f of findings) {
    const level = severityLevel(f.severity);
    if (level > worstLevel) {
      worstLevel = level;
      worst = f.severity.toLowerCase();
    }
  }

  return worst;
}

function renderFindingsPanel(findings: SecurityFinding[]): string {
  return `
    <div class="findings-toggle">
      <label>
        <input type="checkbox" id="findings-show-resolved" />
        Show resolved findings
      </label>
    </div>
    <div id="findings-list">
      ${renderFindingsList(findings)}
    </div>
  `;
}

function renderFindingsList(findings: SecurityFinding[]): string {
  if (findings.length === 0) {
    return '<div class="empty-state"><p>No security findings.</p></div>';
  }

  const sorted = [...findings].sort(
    (a, b) => severityLevel(b.severity) - severityLevel(a.severity)
  );

  return sorted.map((f) => renderFindingRow(f)).join('');
}

function renderFindingRow(f: SecurityFinding): string {
  const sev = f.severity?.toLowerCase() || 'info';
  const resolvedClass = f.is_resolved ? ' finding-resolved' : '';
  const kindLabel = formatFindingKind(f.kind);
  const advisoryLink =
    f.advisory_id && f.advisory_id.startsWith('CVE-')
      ? `<a href="https://nvd.nist.gov/vuln/detail/${encodeURIComponent(f.advisory_id)}" target="_blank" rel="noopener noreferrer">${escapeHtml(f.advisory_id)}</a>`
      : f.advisory_id
        ? escapeHtml(f.advisory_id)
        : '';

  const meta: string[] = [];
  if (f.release_version) {
    meta.push(`v${escapeHtml(f.release_version)}`);
  }
  if (f.artifact_filename) {
    meta.push(escapeHtml(f.artifact_filename));
  }
  if (f.detected_at) {
    meta.push(formatDate(f.detected_at));
  }
  if (f.is_resolved) {
    meta.push(
      `Resolved${f.resolved_at ? ` ${formatDate(f.resolved_at)}` : ''}`
    );
  }

  return `
    <div class="finding-row${resolvedClass}">
      <div class="finding-row__header">
        <span class="badge badge-severity-${sev}">${escapeHtml(sev)}</span>
        <span class="badge badge-ecosystem">${escapeHtml(kindLabel)}</span>
        <span class="finding-row__title">${escapeHtml(f.title)}</span>
        ${advisoryLink ? `<span>${advisoryLink}</span>` : ''}
      </div>
      <div class="finding-row__meta">${meta.join(' · ')}</div>
      ${f.description ? `<div class="finding-row__description">${escapeHtml(f.description)}</div>` : ''}
    </div>
  `;
}

function formatFindingKind(kind: string): string {
  return kind.replace(/_/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());
}

function renderSecuritySummary(openFindings: SecurityFinding[]): string {
  if (openFindings.length === 0) {
    return '';
  }

  const counts: Record<string, number> = {};
  for (const f of openFindings) {
    const sev = f.severity?.toLowerCase() || 'info';
    counts[sev] = (counts[sev] || 0) + 1;
  }

  const breakdown = ['critical', 'high', 'medium', 'low', 'info']
    .filter((s) => counts[s])
    .map(
      (s) =>
        `<span class="badge badge-severity-${s}" style="margin-right:4px;">${counts[s]} ${s}</span>`
    )
    .join('');

  return `
    <div class="card">
      <div class="sidebar-section">
        <h3>Security</h3>
        <p style="margin-bottom:8px; font-size:0.875rem;">${openFindings.length} open finding${openFindings.length === 1 ? '' : 's'}</p>
        <div>${breakdown}</div>
      </div>
    </div>
  `;
}
