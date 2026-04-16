/**
 * Compatibility router for the current page modules.
 *
 * Keeps the existing `route()`, `notFound()`, `navigate()`, and `resolve()`
 * surface while delegating browser history and route state to bQuery.
 */

import {
  currentRoute as bqueryCurrentRoute,
  createRouter,
  isNavigating as bqueryIsNavigating,
} from '@bquery/bquery/router';

export type PageCleanup = void | (() => void);

export interface RouteContext {
  params: Record<string, string>;
  query: URLSearchParams;
}

export interface NotFoundContext {
  path: string;
}

export type RouteHandler = (ctx: RouteContext) => PageCleanup;
export type NotFoundHandler = (ctx: NotFoundContext) => PageCleanup;

interface RouteRecord {
  pattern: string;
  handler: RouteHandler;
}

interface PageRouteMeta {
  kind: 'page';
  handler: RouteHandler;
}

interface NotFoundRouteMeta {
  kind: 'not-found';
}

type MatchedRouteMeta = PageRouteMeta | NotFoundRouteMeta;

const routes: RouteRecord[] = [];
const API_AND_PROTOCOL_PATH_PATTERN =
  /^\/(v1|npm|pypi|composer|rubygems|maven|cargo|nuget|health|readiness|_\/|swagger-ui)/;

let notFoundHandler: NotFoundHandler | null = null;
let currentCleanup: (() => void) | null = null;
let router: ReturnType<typeof createRouter> | null = null;
let routerDirty = false;

export const currentRoute = bqueryCurrentRoute;
export const isNavigating = bqueryIsNavigating;

function cleanupCurrentPage(): void {
  if (typeof currentCleanup === 'function') {
    currentCleanup();
  }

  currentCleanup = null;
}

function safeDecode(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

function decodeParams(
  params: Record<string, unknown> | null | undefined
): Record<string, string> {
  return Object.fromEntries(
    Object.entries(params ?? {}).map(([key, value]) => [
      key,
      typeof value === 'string' ? safeDecode(value) : String(value ?? ''),
    ])
  );
}

function buildRouteDefinitions() {
  return [
    ...routes.map(({ pattern, handler }) => ({
      path: pattern,
      component: () => null,
      meta: {
        kind: 'page' as const,
        handler,
      },
    })),
    {
      path: '*',
      component: () => null,
      meta: {
        kind: 'not-found' as const,
      },
    },
  ];
}

function renderCurrentRoute(): void {
  cleanupCurrentPage();

  const activeRoute = bqueryCurrentRoute.value;
  const routeMeta = activeRoute?.matched
    ?.meta as unknown as MatchedRouteMeta | null;

  if (routeMeta?.kind === 'page') {
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

function ensureRouter(): ReturnType<typeof createRouter> {
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

  router.afterEach(() => {
    renderCurrentRoute();
  });

  routerDirty = false;
  return router;
}

export function route(pattern: string, handler: RouteHandler): void {
  routes.push({ pattern, handler });
  routerDirty = true;
}

export function notFound(handler: NotFoundHandler): void {
  notFoundHandler = handler;
}

export function navigate(
  path: string,
  { replace = false }: { replace?: boolean } = {}
): void {
  const activeRouter = ensureRouter();
  const navigateMethod = replace
    ? activeRouter.replace.bind(activeRouter)
    : activeRouter.push.bind(activeRouter);

  void navigateMethod(path).catch((error: unknown) => {
    console.error('Publaryn router navigation failed:', error);
  });
}

export function resolve(): void {
  ensureRouter();
  renderCurrentRoute();
}

document.addEventListener('click', (event: MouseEvent) => {
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
