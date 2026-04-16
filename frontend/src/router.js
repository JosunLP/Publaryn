/**
 * Compatibility router for the current page modules.
 *
 * Keeps the existing `route()`, `notFound()`, `navigate()`, and `resolve()`
 * surface while delegating browser history and route state to bQuery.
 */

import {
  currentRoute as bqueryCurrentRoute,
  isNavigating as bqueryIsNavigating,
  createRouter,
} from '@bquery/bquery/router';

const routes = [];
const API_AND_PROTOCOL_PATH_PATTERN =
  /^\/(v1|npm|pypi|composer|rubygems|maven|cargo|nuget|health|readiness|_\/|swagger-ui)/;

let notFoundHandler = null;
let currentCleanup = null;
let router = null;
let routerDirty = false;

export const currentRoute = bqueryCurrentRoute;
export const isNavigating = bqueryIsNavigating;

function cleanupCurrentPage() {
  if (typeof currentCleanup === 'function') {
    currentCleanup();
  }
  currentCleanup = null;
}

function safeDecode(value) {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function decodeParams(params) {
  return Object.fromEntries(
    Object.entries(params ?? {}).map(([key, value]) => [
      key,
      typeof value === 'string' ? safeDecode(value) : value,
    ])
  );
}

function buildRouteDefinitions() {
  return [
    ...routes.map(({ pattern, handler }) => ({
      path: pattern,
      component: () => null,
      meta: {
        kind: 'page',
        handler,
      },
    })),
    {
      path: '*',
      component: () => null,
      meta: {
        kind: 'not-found',
      },
    },
  ];
}

function renderCurrentRoute() {
  cleanupCurrentPage();

  const activeRoute = bqueryCurrentRoute.value;
  const routeMeta = activeRoute?.matched?.meta ?? null;

  if (routeMeta?.kind === 'page' && typeof routeMeta.handler === 'function') {
    const cleanup = routeMeta.handler({
      params: decodeParams(activeRoute.params),
      query: new URLSearchParams(window.location.search),
    });

    if (typeof cleanup === 'function') {
      currentCleanup = cleanup;
    }

    return;
  }

  if (
    routeMeta?.kind === 'not-found' &&
    typeof notFoundHandler === 'function'
  ) {
    const cleanup = notFoundHandler({
      path: activeRoute?.path || window.location.pathname,
    });

    if (typeof cleanup === 'function') {
      currentCleanup = cleanup;
    }
  }
}

function ensureRouter() {
  if (router && !routerDirty) {
    return router;
  }

  if (router) {
    cleanupCurrentPage();
    router.destroy();
    router = null;
  }

  router = createRouter({
    routes: buildRouteDefinitions(),
  });
  router.afterEach(() => renderCurrentRoute());
  routerDirty = false;

  return router;
}

export function route(pattern, handler) {
  routes.push({ pattern, handler });
  routerDirty = true;
}

export function notFound(handler) {
  notFoundHandler = handler;
}

export function navigate(path, { replace = false } = {}) {
  const activeRouter = ensureRouter();

  void activeRouter[replace ? 'replace' : 'push'](path).catch((error) => {
    console.error('Publaryn router navigation failed:', error);
  });
}

export function resolve() {
  ensureRouter();
  renderCurrentRoute();
}

document.addEventListener('click', (event) => {
  if (!(event.target instanceof Element)) {
    return;
  }

  const link = event.target.closest('a[href]');
  if (!(link instanceof HTMLAnchorElement)) {
    return;
  }

  const href = link.getAttribute('href');
  if (
    !href ||
    href.startsWith('//') ||
    href.startsWith('#') ||
    link.target === '_blank' ||
    link.hasAttribute('download')
  ) {
    return;
  }

  if (link.origin !== window.location.origin) {
    return;
  }

  const targetPath = `${link.pathname}${link.search}${link.hash}`;
  if (API_AND_PROTOCOL_PATH_PATTERN.test(link.pathname)) {
    return;
  }

  event.preventDefault();
  navigate(targetPath);
});
