/// <reference path="./bun-test.d.ts" />

import { afterEach } from 'bun:test';
import { readFileSync } from 'node:fs';
import { existsSync, mkdirSync, symlinkSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, resolve as resolvePath } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { Window } from 'happy-dom';
import { compile } from 'svelte/compiler';

const window = new Window({
  url: 'https://example.test/',
});
Object.defineProperty(window, 'SyntaxError', {
  configurable: true,
  writable: true,
  value: SyntaxError,
});

installDomGlobals(window);
document.body.innerHTML = '';

const componentModuleCache = new Map<string, Promise<any>>();
const compiledComponentUrlCache = new Map<string, Promise<string>>();

afterEach(() => {
  document.body.innerHTML = '';
});

export async function renderSvelte(
  component: any,
  props: Record<string, unknown> = {}
) {
  const runtime = await getClientRuntime();
  const componentModule = await loadComponentModule(component);
  const target = document.createElement('div');
  document.body.appendChild(target);
  const instance = runtime.mount(componentModule.default, {
    target,
    props,
  });
  runtime.flushSync();

  return {
    target,
    instance,
    unmount() {
      runtime.unmount(instance);
      runtime.flushSync();
      target.remove();
    },
  };
}

export function changeValue(
  element: HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement,
  value: string
): void {
  element.value = value;
  element.dispatchEvent(new Event('input', { bubbles: true }));
  element.dispatchEvent(new Event('change', { bubbles: true }));
}

export function setChecked(element: HTMLInputElement, checked: boolean): void {
  element.checked = checked;
  element.dispatchEvent(new Event('input', { bubbles: true }));
  element.dispatchEvent(new Event('change', { bubbles: true }));
}

export function submitForm(form: HTMLFormElement): void {
  form.dispatchEvent(
    new SubmitEvent('submit', {
      bubbles: true,
      cancelable: true,
    })
  );
}

export function click(element: HTMLElement): void {
  element.click();
}

async function getClientRuntime(): Promise<{
  flushSync: (fn?: (() => void) | undefined) => void;
  mount: (component: any, options: { target: Element; props?: Record<string, unknown> }) => any;
  unmount: (component: any) => void;
}> {
  const renderModuleUrl = new URL(
    '../node_modules/svelte/src/internal/client/render.js',
    import.meta.url
  ).href;
  const batchModuleUrl = new URL(
    '../node_modules/svelte/src/internal/client/reactivity/batch.js',
    import.meta.url
  ).href;

  const renderModule = (await import(renderModuleUrl)) as {
    mount: (component: any, options: { target: Element; props?: Record<string, unknown> }) => any;
    unmount: (component: any) => void;
  };
  const batchModule = (await import(batchModuleUrl)) as {
    flushSync: (fn?: (() => void) | undefined) => void;
  };

  return {
    flushSync: batchModule.flushSync,
    mount: renderModule.mount,
    unmount: renderModule.unmount,
  };
}

async function loadComponentModule(component: any): Promise<any> {
  const componentPath =
    typeof component === 'string' ? component : component?.default || component;

  if (typeof componentPath !== 'string') {
    return component;
  }

  const cached = componentModuleCache.get(componentPath);
  if (cached) {
    return cached;
  }

  const outputUrl = await ensureCompiledComponentUrl(componentPath);
  const modulePromise = import(outputUrl);
  componentModuleCache.set(componentPath, modulePromise);
  return modulePromise;
}

async function ensureCompiledComponentUrl(componentPath: string): Promise<string> {
  const cached = compiledComponentUrlCache.get(componentPath);
  if (cached) {
    return cached;
  }

  const outputUrlPromise = compileComponentModule(componentPath);
  compiledComponentUrlCache.set(componentPath, outputUrlPromise);
  return outputUrlPromise;
}

async function compileComponentModule(componentPath: string): Promise<string> {
  const source = readFileSync(componentPath, 'utf8');
  const compiled = compile(source, {
    filename: componentPath,
    generate: 'client',
    dev: true,
  });
  const outputDir = '/tmp/publaryn-svelte-test-modules';
  const outputPath = `${outputDir}/${createHash('sha256').update(componentPath).digest('hex')}.mjs`;
  mkdirSync(outputDir, { recursive: true });
  const nodeModulesLink = `${outputDir}/node_modules`;
  const localNodeModulesPath = fileURLToPath(
    new URL('../node_modules', import.meta.url)
  );
  if (!existsSync(nodeModulesLink)) {
    symlinkSync(localNodeModulesPath, nodeModulesLink, 'dir');
  }
  const rewrittenCode = await rewriteRelativeImports(compiled.js.code, componentPath);
  writeFileSync(outputPath, rewrittenCode, 'utf8');

  return pathToFileURL(outputPath).href;
}

async function rewriteRelativeImports(
  code: string,
  componentPath: string
): Promise<string> {
  const importPattern =
    /(from\s+['"]|import\s*\(\s*['"]|import\s+['"])(\.{1,2}\/[^'"]+)(['"]\s*\)?)/g;
  const replacements = new Map<string, string>();
  let match: RegExpExecArray | null;

  while ((match = importPattern.exec(code)) !== null) {
    const specifier = match[2];
    if (replacements.has(specifier)) {
      continue;
    }

    const resolvedPath = resolveRelativeImport(componentPath, specifier);
    const rewrittenSpecifier = resolvedPath.endsWith('.svelte')
      ? await ensureCompiledComponentUrl(resolvedPath)
      : pathToFileURL(resolvedPath).href;
    replacements.set(specifier, rewrittenSpecifier);
  }

  let rewrittenCode = code;
  for (const [specifier, rewrittenSpecifier] of replacements) {
    rewrittenCode = rewrittenCode.replaceAll(specifier, rewrittenSpecifier);
  }

  return rewrittenCode;
}

function resolveRelativeImport(componentPath: string, specifier: string): string {
  const basePath = resolvePath(dirname(componentPath), specifier);
  const candidates = [
    basePath,
    `${basePath}.ts`,
    `${basePath}.js`,
    `${basePath}.svelte`,
    resolvePath(basePath, 'index.ts'),
    resolvePath(basePath, 'index.js'),
    resolvePath(basePath, 'index.svelte'),
  ];

  const resolved = candidates.find((candidate) => existsSync(candidate));
  if (!resolved) {
    throw new Error(
      `Unable to resolve relative import '${specifier}' from ${componentPath}`
    );
  }

  return resolved;
}

function installDomGlobals(window: Window): void {
  const globals = [
    'window',
    'document',
    'navigator',
    'HTMLElement',
    'HTMLInputElement',
    'HTMLSelectElement',
    'HTMLTextAreaElement',
    'HTMLFormElement',
    'HTMLButtonElement',
    'HTMLMediaElement',
    'HTMLTemplateElement',
    'Node',
    'Element',
    'Text',
    'Comment',
    'DocumentFragment',
    'Event',
    'SubmitEvent',
    'CustomEvent',
    'MouseEvent',
    'FormData',
    'Blob',
    'File',
    'MutationObserver',
    'getComputedStyle',
    'requestAnimationFrame',
    'cancelAnimationFrame',
  ] as const;

  for (const key of globals) {
    Object.defineProperty(globalThis, key, {
      configurable: true,
      writable: true,
      value: (window as any)[key],
    });
  }
}
