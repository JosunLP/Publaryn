/**
 * Minimal client-side router.
 *
 * Supports path patterns like /packages/:ecosystem/:name
 * Uses History API for clean URLs.
 */

const routes = [];
let notFoundHandler = null;
let currentCleanup = null;

export function route(pattern, handler) {
  const paramNames = [];
  const regexStr = pattern.replace(/:([^/]+)/g, (_, name) => {
    paramNames.push(name);
    return '([^/]+)';
  });
  routes.push({
    regex: new RegExp(`^${regexStr}$`),
    paramNames,
    handler,
  });
}

export function notFound(handler) {
  notFoundHandler = handler;
}

export function navigate(path, { replace = false } = {}) {
  if (replace) {
    history.replaceState(null, '', path);
  } else {
    history.pushState(null, '', path);
  }
  resolve();
}

export function resolve() {
  const path = location.pathname;
  const search = new URLSearchParams(location.search);

  // Cleanup previous page
  if (currentCleanup) {
    currentCleanup();
    currentCleanup = null;
  }

  for (const { regex, paramNames, handler } of routes) {
    const match = path.match(regex);
    if (match) {
      const params = {};
      paramNames.forEach((name, i) => {
        params[name] = decodeURIComponent(match[i + 1]);
      });
      const cleanup = handler({ params, query: search });
      if (typeof cleanup === 'function') {
        currentCleanup = cleanup;
      }
      return;
    }
  }

  if (notFoundHandler) {
    notFoundHandler({ path });
  }
}

// Handle browser back/forward
window.addEventListener('popstate', () => resolve());

// Intercept link clicks for SPA navigation
document.addEventListener('click', (e) => {
  const link = e.target.closest('a[href]');
  if (!link) return;
  const href = link.getAttribute('href');
  // Only handle internal navigation links
  if (
    !href ||
    href.startsWith('http') ||
    href.startsWith('//') ||
    href.startsWith('#') ||
    link.target === '_blank'
  ) {
    return;
  }
  // Skip API and protocol adapter paths
  if (
    /^\/(v1|npm|pypi|composer|rubygems|maven|cargo|nuget|health|readiness|_\/|swagger-ui)/.test(
      href
    )
  ) {
    return;
  }
  e.preventDefault();
  navigate(href);
});
