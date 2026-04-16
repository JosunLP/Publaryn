import { getAuthToken } from '../api/client.js';
import { navigate } from '../router.js';

/**
 * Render the shared page layout (header + footer) and return a reference
 * to the main content container.
 */
export function renderLayout(rootEl) {
  const isLoggedIn = !!getAuthToken();

  rootEl.innerHTML = `
    <header class="site-header">
      <div class="container">
        <a href="/" class="logo">
          <svg viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg"><rect width="32" height="32" rx="6" fill="#2563eb"/><text x="5" y="23" font-family="monospace" font-weight="bold" font-size="18" fill="white">P</text></svg>
          Publaryn
        </a>
        <div class="search-bar">
          <form id="header-search-form" action="/search">
            <input
              type="search"
              name="q"
              class="search-input"
              placeholder="Search packages…"
              aria-label="Search packages"
              autocomplete="off"
            />
          </form>
        </div>
        <nav>
          ${
            isLoggedIn
              ? `<a href="/settings" class="btn btn-secondary btn-sm">Settings</a>
               <button id="logout-btn" class="btn btn-secondary btn-sm">Logout</button>`
              : `<a href="/login" class="btn btn-secondary btn-sm">Sign in</a>
               <a href="/register" class="btn btn-primary btn-sm">Sign up</a>`
          }
        </nav>
      </div>
    </header>
    <main id="main-content" class="container"></main>
    <footer class="site-footer">
      <div class="container">
        Publaryn &mdash; Secure multi-ecosystem package registry
      </div>
    </footer>
  `;

  // Handle header search form submission
  const searchForm = rootEl.querySelector('#header-search-form');
  searchForm.addEventListener('submit', (e) => {
    e.preventDefault();
    const q = searchForm.querySelector('input').value.trim();
    if (q) {
      navigate(`/search?q=${encodeURIComponent(q)}`);
    }
  });

  return rootEl.querySelector('#main-content');
}
