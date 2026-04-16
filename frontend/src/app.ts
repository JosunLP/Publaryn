import { defineBqueryConfig } from '@bquery/bquery/platform';
import { effect } from '@bquery/bquery/reactive';
import '@bquery/ui';

import { onUnauthorized } from './api/client';
import { renderLayout } from './layouts/layout';
import { landingPage } from './pages/landing';
import { loginPage } from './pages/login';
import { notFoundPage } from './pages/not-found';
import { orgDetailPage } from './pages/org-detail';
import { packageDetailPage } from './pages/package-detail';
import { registerPage } from './pages/register';
import { searchPage } from './pages/search';
import { settingsPage } from './pages/settings';
import { versionDetailPage } from './pages/version-detail';
import {
  currentRoute,
  isNavigating,
  navigate,
  notFound,
  resolve,
  route,
} from './router';
import type { PageCleanup } from './router';
import './styles/main.css';
import { initializeTheme } from './theme';

type PageHandler<TContext> = (
  ctx: TContext,
  container: HTMLElement
) => PageCleanup;

const root = document.getElementById('app');

if (!(root instanceof HTMLElement)) {
  throw new Error('Publaryn frontend root element was not found.');
}

const appRoot = root;

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
  const routePath = currentRoute.value.path;
  const path = typeof routePath === 'string' && routePath ? routePath : '/';
  const routeMeta = currentRoute.value.matched?.meta as
    | { kind?: string }
    | undefined;
  const isNotFound = routeMeta?.kind === 'not-found';

  document.body.dataset.routePath = path;
  document.body.dataset.routeState = isNavigating.value ? 'loading' : 'idle';
  document.title = titleForPath(path, isNotFound);
});

function page<TContext>(handler: PageHandler<TContext>) {
  return (ctx: TContext): PageCleanup => {
    const main = renderLayout(appRoot);
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
