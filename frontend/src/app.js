/**
 * Publaryn frontend application entry point.
 *
 * Registers all routes, initializes the layout, and starts the router.
 */

import { onUnauthorized } from './api/client.js';
import { renderLayout } from './layouts/layout.js';
import { landingPage } from './pages/landing.js';
import { loginPage } from './pages/login.js';
import { notFoundPage } from './pages/not-found.js';
import { packageDetailPage } from './pages/package-detail.js';
import { registerPage } from './pages/register.js';
import { searchPage } from './pages/search.js';
import { versionDetailPage } from './pages/version-detail.js';
import { navigate, notFound, resolve, route } from './router.js';
import './styles/main.css';

const root = document.getElementById('app');

// Auth redirect on 401
onUnauthorized(() => navigate('/login', { replace: true }));

// Helper: render layout then call page handler
function page(handler) {
  return (ctx) => {
    const main = renderLayout(root);
    return handler(ctx, main);
  };
}

// ── Route definitions ────────────────────────────────────
route('/', page(landingPage));
route('/search', page(searchPage));
route('/packages/:ecosystem/:name', page(packageDetailPage));
route('/packages/:ecosystem/:name/versions/:version', page(versionDetailPage));
route('/login', page(loginPage));
route('/register', page(registerPage));
notFound(page(notFoundPage));

// ── Start ────────────────────────────────────────────────
resolve();
