import { getStats } from '../api/packages.js';
import { navigate } from '../router.js';
import { ECOSYSTEMS } from '../utils/ecosystem.js';
import { escapeHtml, formatNumber } from '../utils/format.js';

export function landingPage({ params, query }, container) {
  container.innerHTML = `<div class="loading"><span class="spinner"></span> Loading…</div>`;

  render(container);
}

async function render(container) {
  let stats = { packages: 0, releases: 0, organizations: 0 };
  try {
    stats = await getStats();
  } catch {
    // Stats are non-critical; render page without them
  }

  container.innerHTML = `
    <section class="hero">
      <h1>Publaryn</h1>
      <p>A secure, multi-ecosystem package registry for modern software teams.</p>
      <div class="search-bar">
        <form id="hero-search-form">
          <input
            type="search"
            name="q"
            class="search-input"
            placeholder="Search packages across all ecosystems…"
            aria-label="Search packages"
            autocomplete="off"
          />
        </form>
      </div>
    </section>

    <section class="stats-bar">
      <div class="stat">
        <div class="stat__value">${formatNumber(stats.packages)}</div>
        <div class="stat__label">Packages</div>
      </div>
      <div class="stat">
        <div class="stat__value">${formatNumber(stats.releases)}</div>
        <div class="stat__label">Releases</div>
      </div>
      <div class="stat">
        <div class="stat__value">${formatNumber(stats.organizations)}</div>
        <div class="stat__label">Organizations</div>
      </div>
    </section>

    <section>
      <h2 style="text-align:center; margin-bottom:16px;">Supported Ecosystems</h2>
      <div class="ecosystem-grid">
        ${ECOSYSTEMS.map(
          (e) => `
          <a href="/search?ecosystem=${e.id}" class="ecosystem-tile">
            <span style="font-size:1.5rem">${e.icon}</span>
            <span>${escapeHtml(e.label)}</span>
          </a>`
        ).join('')}
      </div>
    </section>
  `;

  const form = container.querySelector('#hero-search-form');
  form.addEventListener('submit', (e) => {
    e.preventDefault();
    const q = form.querySelector('input').value.trim();
    if (q) {
      navigate(`/search?q=${encodeURIComponent(q)}`);
    }
  });
}
