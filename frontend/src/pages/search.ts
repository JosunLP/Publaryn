import type { SearchPackagesResponse } from '../api/packages';
import { searchPackages } from '../api/packages';
import type { RouteContext } from '../router';
import { navigate } from '../router';
import { ECOSYSTEMS, ecosystemIcon, ecosystemLabel } from '../utils/ecosystem';
import { escapeHtml, formatDate, formatNumber } from '../utils/format';

export function searchPage(
  { query }: RouteContext,
  container: HTMLElement
): void {
  const q = query.get('q') ?? '';
  const ecosystem = query.get('ecosystem') ?? '';
  const parsedPage = Number.parseInt(query.get('page') ?? '1', 10);
  const page = Number.isFinite(parsedPage) && parsedPage > 0 ? parsedPage : 1;

  container.innerHTML = `<div class="loading"><span class="spinner"></span> Searching…</div>`;
  void render(container, { q, ecosystem, page });
}

async function render(
  container: HTMLElement,
  { q, ecosystem, page }: { q: string; ecosystem: string; page: number }
): Promise<void> {
  const perPage = 20;
  let results: SearchPackagesResponse = {
    total: 0,
    packages: [],
    page: 1,
    per_page: perPage,
  };

  try {
    results = await searchPackages({
      q: q || undefined,
      ecosystem: ecosystem || undefined,
      page,
      perPage,
    });
  } catch (caughtError: unknown) {
    const message =
      caughtError instanceof Error ? caughtError.message : 'Search failed.';

    container.innerHTML = `<div class="alert alert-error">Search failed: ${escapeHtml(message)}</div>`;
    return;
  }

  const totalPages = Math.max(1, Math.ceil(results.total / perPage));

  container.innerHTML = `
    <div class="mt-6">
      <form id="search-form" style="display:flex; gap:12px; margin-bottom:20px; flex-wrap:wrap;">
        <input
          type="search"
          name="q"
          class="search-input"
          value="${escapeHtml(q)}"
          placeholder="Search packages…"
          aria-label="Search packages"
          style="flex:1; min-width:200px;"
        />
        <select name="ecosystem" class="form-input" style="width:auto; min-width:140px;">
          <option value="">All ecosystems</option>
          ${ECOSYSTEMS.map(
            (option) =>
              `<option value="${option.id}" ${option.id === ecosystem ? 'selected' : ''}>${option.label}</option>`
          ).join('')}
        </select>
        <button type="submit" class="btn btn-primary">Search</button>
      </form>

      <div class="text-muted mb-4" style="font-size:0.875rem;">
        ${results.total} result${results.total !== 1 ? 's' : ''} found
      </div>

      <div class="card" style="padding:0;">
        ${
          results.packages.length === 0
            ? `<div class="empty-state">
                <h3>No packages found</h3>
                <p>Try a different search term or browse by ecosystem.</p>
              </div>`
            : results.packages
                .map(
                  (pkg) => `
            <a href="/packages/${encodeURIComponent(pkg.ecosystem || 'unknown')}/${encodeURIComponent(pkg.name)}" class="package-card">
              <div class="package-card__header">
                <span class="package-card__name">${escapeHtml(pkg.display_name || pkg.name)}</span>
                <span class="badge badge-ecosystem">${ecosystemIcon(pkg.ecosystem)} ${ecosystemLabel(pkg.ecosystem)}</span>
                ${pkg.latest_version ? `<span class="package-card__version">v${escapeHtml(pkg.latest_version)}</span>` : ''}
                ${pkg.is_deprecated ? `<span class="badge badge-deprecated">deprecated</span>` : ''}
              </div>
              <div class="package-card__description">${escapeHtml(pkg.description || '')}</div>
              <div class="package-card__meta">
                ${pkg.owner_name ? `<span>by ${escapeHtml(pkg.owner_name)}</span>` : ''}
                ${pkg.download_count != null ? `<span>${formatNumber(pkg.download_count)} downloads</span>` : ''}
                ${pkg.updated_at ? `<span>updated ${formatDate(pkg.updated_at)}</span>` : ''}
              </div>
            </a>`
                )
                .join('')
        }
      </div>

      ${
        totalPages > 1
          ? `<div class="pagination">
              ${page > 1 ? `<button class="btn btn-secondary btn-sm" data-page="${page - 1}">← Prev</button>` : ''}
              <span class="current">Page ${page} of ${totalPages}</span>
              ${page < totalPages ? `<button class="btn btn-secondary btn-sm" data-page="${page + 1}">Next →</button>` : ''}
            </div>`
          : ''
      }
    </div>
  `;

  const form = container.querySelector<HTMLFormElement>('#search-form');
  form?.addEventListener('submit', (event) => {
    event.preventDefault();

    const formData = new FormData(form);
    const params = new URLSearchParams();
    const queryValue = formData.get('q')?.toString().trim() ?? '';
    const ecosystemValue = formData.get('ecosystem')?.toString().trim() ?? '';

    if (queryValue) {
      params.set('q', queryValue);
    }

    if (ecosystemValue) {
      params.set('ecosystem', ecosystemValue);
    }

    navigate(`/search?${params.toString()}`);
  });

  container
    .querySelectorAll<HTMLButtonElement>('.pagination button[data-page]')
    .forEach((button) => {
      button.addEventListener('click', () => {
        const nextPage = button.dataset.page;
        if (!nextPage) {
          return;
        }

        const params = new URLSearchParams();

        if (q) {
          params.set('q', q);
        }

        if (ecosystem) {
          params.set('ecosystem', ecosystem);
        }

        params.set('page', nextPage);
        navigate(`/search?${params.toString()}`);
      });
    });
}
