import { clearAuthToken, getAuthToken } from '../api/client.js';
import { navigate } from '../router.js';
import { getThemeMode, toggleThemeMode } from '../theme.js';

/**
 * Render the shared page layout (header + footer) and return a reference
 * to the main content container.
 */
export function renderLayout(rootEl) {
  const isLoggedIn = !!getAuthToken();
  const themeMode = getThemeMode();

  rootEl.innerHTML = `
    <header class="site-header border-b border-slate-200/80 bg-white/90 backdrop-blur supports-[backdrop-filter]:bg-white/80 dark:border-slate-800/80 dark:bg-slate-950/85">
      <div class="container">
        <a href="/" class="logo">
          <svg viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg"><rect width="32" height="32" rx="6" fill="#2563eb"/><text x="5" y="23" font-family="monospace" font-weight="bold" font-size="18" fill="white">P</text></svg>
          <span>Publaryn</span>
          <bq-badge variant="success">Preview</bq-badge>
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
        <nav class="flex flex-wrap justify-end">
          <button
            id="theme-toggle-btn"
            class="btn btn-secondary btn-sm"
            type="button"
            aria-pressed="${themeMode === 'dark'}"
          >
            ${themeMode === 'dark' ? 'Light mode' : 'Dark mode'}
          </button>
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

  const themeToggle = rootEl.querySelector('#theme-toggle-btn');
  themeToggle?.addEventListener('click', () => {
    const nextMode = toggleThemeMode();
    themeToggle.setAttribute('aria-pressed', String(nextMode === 'dark'));
    themeToggle.textContent = nextMode === 'dark' ? 'Light mode' : 'Dark mode';
  });

  rootEl.querySelector('#logout-btn')?.addEventListener('click', () => {
    clearAuthToken();
    navigate('/login', { replace: true });
  });

  return rootEl.querySelector('#main-content');
}
