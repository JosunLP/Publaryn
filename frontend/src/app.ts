import { defineBqueryConfig } from '@bquery/bquery/platform';
import { effect } from '@bquery/bquery/reactive';
import '@bquery/ui';

import { onUnauthorized } from './api/client.js';
import { renderLayout } from './layouts/layout.js';
import { landingPage } from './pages/landing.js';
import { loginPage } from './pages/login.js';
import { notFoundPage } from './pages/not-found.js';
import { orgDetailPage } from './pages/org-detail.js';
import { packageDetailPage } from './pages/package-detail.js';
import { registerPage } from './pages/register.js';
import { searchPage } from './pages/search.js';
import { settingsPage } from './pages/settings.js';
import { versionDetailPage } from './pages/version-detail.js';
import {
  currentRoute,
  isNavigating,
  navigate,
  notFound,
  resolve,
  route,
} from './router.js';
import './styles/main.css';
import { initializeTheme } from './theme.js';

type PageHandler = (ctx: any, container: HTMLElement) => void | (() => void);

const root = document.getElementById('app');

if (!(root instanceof HTMLElement)) {
  throw new Error('Publaryn frontend root element was not found.');
}

defineBqueryConfig({
  transitions: {
    skipOnReducedMotion: true,
    classes: ['page-transition'],
  },
});

initializeTheme();

onUnauthorized(() => navigate('/login', { replace: true }));

function titleForPath(path: string, isNotFound: boolean) {
  if (isNotFound) {
    return 'Page not found — Publaryn';
  }

  if (path === '/') {
    return 'Publaryn — Secure multi-ecosystem package registry';
  }

  if (path === '/search') {
    return 'Search — Publaryn';
  }

  if (path === '/login') {
    return 'Sign in — Publaryn';
  }

  if (path === '/register') {
    return 'Sign up — Publaryn';
  }

  if (path === '/settings') {
    return 'Settings — Publaryn';
  }

  if (path.startsWith('/orgs/')) {
    return 'Organization — Publaryn';
  }

  if (path.startsWith('/packages/') && path.includes('/versions/')) {
    return 'Package version — Publaryn';
  }

  if (path.startsWith('/packages/')) {
    return 'Package details — Publaryn';
  }

  return 'Publaryn';
}

effect(() => {
  const path = currentRoute.value.path || '/';
  const isNotFound = currentRoute.value.matched?.meta?.kind === 'not-found';

  document.body.dataset.routePath = path;
  document.body.dataset.routeState = isNavigating.value ? 'loading' : 'idle';
  document.title = titleForPath(path, isNotFound);
});

function page(handler: PageHandler) {
  return (ctx: any) => {
    const main = renderLayout(root);
    return handler(ctx, main);
  };
}

route('/', page(landingPage));
route('/search', page(searchPage));
route('/orgs/:slug', page(orgDetailPage));
route('/packages/:ecosystem/:name', page(packageDetailPage));
route('/packages/:ecosystem/:name/versions/:version', page(versionDetailPage));
route('/login', page(loginPage));
route('/register', page(registerPage));
route('/settings', page(settingsPage));
notFound(page(notFoundPage));

resolve();
