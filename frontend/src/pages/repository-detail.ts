import { ApiError } from '../api/client';
import type {
  RepositoryDetail,
  RepositoryPackageSummary,
} from '../api/repositories';
import { getRepository, listRepositoryPackages } from '../api/repositories';
import type { RouteContext } from '../router';
import { ecosystemIcon, ecosystemLabel } from '../utils/ecosystem';
import { escapeHtml, formatDate, formatNumber } from '../utils/format';
import {
  formatRepositoryKindLabel,
  formatRepositoryPackageCoverageLabel,
  formatRepositoryVisibilityLabel,
  resolveRepositoryOwnerSummary,
} from '../utils/repositories';

const MAX_VISIBLE_PACKAGES = 100;

export function repositoryDetailPage(
  { params }: RouteContext,
  container: HTMLElement
): void {
  const slug = params.slug ?? '';

  container.innerHTML = `<div class="loading"><span class="spinner"></span> Loading…</div>`;
  void render(container, slug);
}

async function render(container: HTMLElement, slug: string): Promise<void> {
  let repository: RepositoryDetail;

  try {
    repository = await getRepository(slug);
  } catch (caughtError: unknown) {
    if (caughtError instanceof ApiError && caughtError.status === 404) {
      container.innerHTML = `
        <div class="empty-state mt-6">
          <h2>Repository not found</h2>
          <p>@${escapeHtml(slug)} does not exist or is not visible to you.</p>
          <a href="/search" class="btn btn-primary mt-4">Search packages</a>
        </div>`;
      return;
    }

    const message =
      caughtError instanceof Error
        ? caughtError.message
        : 'Failed to load repository.';

    container.innerHTML = `<div class="alert alert-error mt-6">Failed to load repository: ${escapeHtml(message)}</div>`;
    return;
  }

  let packages: RepositoryPackageSummary[] = [];
  let packageError: string | null = null;

  try {
    const packageData = await listRepositoryPackages(slug, {
      perPage: MAX_VISIBLE_PACKAGES,
    });
    packages = packageData.packages || [];
    packageError = packageData.load_error || null;
  } catch (caughtError: unknown) {
    packageError =
      caughtError instanceof Error
        ? caughtError.message
        : 'Failed to load repository packages.';
  }

  const repositorySlug = repository.slug?.trim() || slug;
  const repositoryName =
    repository.name?.trim() || repositorySlug || 'Repository';
  const ownerSummary = resolveRepositoryOwnerSummary({
    ownerOrgName: repository.owner_org_name,
    ownerOrgSlug: repository.owner_org_slug,
    ownerUsername: repository.owner_username,
  });

  container.innerHTML = `
    <div class="mt-6">
      <nav style="font-size:0.875rem; margin-bottom:16px;">
        ${renderBreadcrumb(ownerSummary, repositoryName)}
      </nav>

      <div class="pkg-header">
        <h1 class="pkg-header__name">${escapeHtml(repositoryName)}</h1>
        <span class="badge badge-ecosystem">Repository</span>
        <span class="badge badge-ecosystem">${escapeHtml(formatRepositoryKindLabel(repository.kind))}</span>
        <span class="badge badge-ecosystem">${escapeHtml(formatRepositoryVisibilityLabel(repository.visibility))}</span>
      </div>

      <p class="text-muted mt-4" style="font-size:1.05rem;">@${escapeHtml(repositorySlug)}</p>
      ${
        repository.description
          ? `<p class="text-muted mt-4" style="font-size:1.05rem;">${escapeHtml(repository.description)}</p>`
          : ''
      }

      <div class="pkg-detail">
        <div class="pkg-detail__main">
          <div class="card mb-4" style="padding:0;">
            <div style="padding:16px 20px 8px;">
              <h3 style="font-size:0.875rem; font-weight:600;">Visible packages</h3>
              <p class="settings-copy">${escapeHtml(renderVisiblePackageSummary(packages.length))}</p>
            </div>
            ${
              packageError
                ? `<div class="alert alert-error" style="margin:0 20px 20px;">${escapeHtml(packageError)}</div>`
                : renderPackageList(packages)
            }
          </div>
        </div>

        <div class="pkg-detail__sidebar">
          ${renderRepositorySidebar(repository, ownerSummary, packages.length)}
        </div>
      </div>
    </div>
  `;
}

function renderBreadcrumb(
  ownerSummary: ReturnType<typeof resolveRepositoryOwnerSummary>,
  repositoryName: string
): string {
  const segments: string[] = [];

  if (ownerSummary.href) {
    segments.push(
      `<a href="${escapeHtml(ownerSummary.href)}">${escapeHtml(ownerSummary.label)}</a>`
    );
  }

  segments.push(
    `<span style="color:var(--color-text-secondary);">${escapeHtml(repositoryName)}</span>`
  );

  return segments.join(' &rsaquo; ');
}

function renderVisiblePackageSummary(packageCount: number): string {
  if (packageCount >= MAX_VISIBLE_PACKAGES) {
    return `Showing the first ${MAX_VISIBLE_PACKAGES} visible packages.`;
  }

  return formatRepositoryPackageCoverageLabel(packageCount, packageCount);
}

function renderPackageList(packages: RepositoryPackageSummary[]): string {
  if (packages.length === 0) {
    return `
      <div class="empty-state" style="margin:0 20px 20px;">
        <p>No visible packages belong to this repository yet.</p>
      </div>`;
  }

  const rows = packages
    .map((pkg) => {
      const ecosystem = pkg.ecosystem || '';
      const name = pkg.name || 'Unnamed package';
      const metaParts: string[] = [];

      if (typeof pkg.download_count === 'number') {
        metaParts.push(
          `${escapeHtml(formatNumber(pkg.download_count))} downloads`
        );
      }

      if (pkg.created_at) {
        metaParts.push(`created ${escapeHtml(formatDate(pkg.created_at))}`);
      }

      return `
        <div class="release-row">
          <div>
            <a href="/packages/${encodeURIComponent(ecosystemOrFallback(ecosystem))}/${encodeURIComponent(name)}" class="release-row__version">
              ${ecosystemIcon(ecosystem)} ${escapeHtml(name)}
            </a>
            <span class="text-muted" style="font-size:0.8125rem; margin-left:8px;">
              ${escapeHtml(ecosystemLabel(ecosystem))}
            </span>
            ${
              pkg.visibility
                ? `<span class="badge badge-ecosystem" style="margin-left:8px;">${escapeHtml(formatRepositoryVisibilityLabel(pkg.visibility))}</span>`
                : ''
            }
            ${
              pkg.description
                ? `<div class="settings-copy" style="margin-top:6px;">${escapeHtml(pkg.description)}</div>`
                : ''
            }
          </div>
          <div class="release-row__meta">${metaParts.join(' · ')}</div>
        </div>`;
    })
    .join('');

  return rows;
}

function renderRepositorySidebar(
  repository: RepositoryDetail,
  ownerSummary: ReturnType<typeof resolveRepositoryOwnerSummary>,
  packageCount: number
): string {
  const metadata: string[] = [];
  const repositorySlug = repository.slug?.trim() || 'unknown';

  metadata.push(row('Slug', `@${escapeHtml(repositorySlug)}`));
  metadata.push(
    row('Owner', renderOptionalLink(ownerSummary.href, ownerSummary.label))
  );
  metadata.push(
    row('Kind', escapeHtml(formatRepositoryKindLabel(repository.kind)))
  );
  metadata.push(
    row(
      'Visibility',
      escapeHtml(formatRepositoryVisibilityLabel(repository.visibility))
    )
  );
  metadata.push(
    row('Visible packages', escapeHtml(formatNumber(packageCount)))
  );

  if (repository.created_at) {
    metadata.push(
      row('Created', escapeHtml(formatDate(repository.created_at)))
    );
  }

  if (repository.updated_at) {
    metadata.push(
      row('Updated', escapeHtml(formatDate(repository.updated_at)))
    );
  }

  const sections = [
    `<div class="card"><div class="sidebar-section"><h3>Repository info</h3>${metadata.join('')}</div></div>`,
  ];

  if (repository.upstream_url) {
    sections.push(`
      <div class="card">
        <div class="sidebar-section">
          <h3>Upstream</h3>
          <a href="${escapeHtml(repository.upstream_url)}" target="_blank" rel="noopener noreferrer">
            ${escapeHtml(repository.upstream_url)}
          </a>
        </div>
      </div>
    `);
  }

  return sections.join('');
}

function renderOptionalLink(href: string | null, label: string): string {
  if (!href) {
    return escapeHtml(label);
  }

  return `<a href="${escapeHtml(href)}">${escapeHtml(label)}</a>`;
}

function row(label: string, value: string): string {
  return `<div class="sidebar-row"><span class="sidebar-row__label">${escapeHtml(label)}</span><span class="sidebar-row__value">${value}</span></div>`;
}

function ecosystemOrFallback(ecosystem: string): string {
  return ecosystem.trim() || 'unknown';
}
