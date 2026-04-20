/// <reference path="./bun-test.d.ts" />

import { afterEach } from 'bun:test';
import { JSDOM } from 'jsdom';
import { flushSync, mount, unmount } from 'svelte';

const dom = new JSDOM('<!doctype html><html><body></body></html>', {
  url: 'https://example.test/',
});

installDomGlobals(dom.window);

afterEach(() => {
  document.body.innerHTML = '';
});

export function renderSvelte(
  component: Parameters<typeof mount>[0],
  props: Record<string, unknown> = {}
) {
  const target = document.createElement('div');
  document.body.appendChild(target);
  const instance = mount(component, {
    target,
    props,
  });
  flushSync();

  return {
    target,
    instance,
    unmount() {
      unmount(instance);
      flushSync();
      target.remove();
    },
  };
}

export function changeValue(
  element: HTMLInputElement | HTMLSelectElement,
  value: string
): void {
  element.value = value;
  element.dispatchEvent(new Event('input', { bubbles: true }));
  element.dispatchEvent(new Event('change', { bubbles: true }));
  flushSync();
}

export function setChecked(element: HTMLInputElement, checked: boolean): void {
  element.checked = checked;
  element.dispatchEvent(new Event('input', { bubbles: true }));
  element.dispatchEvent(new Event('change', { bubbles: true }));
  flushSync();
}

export function submitForm(form: HTMLFormElement): void {
  form.dispatchEvent(
    new SubmitEvent('submit', {
      bubbles: true,
      cancelable: true,
    })
  );
  flushSync();
}

function installDomGlobals(window: Window): void {
  const globals = [
    'window',
    'document',
    'navigator',
    'HTMLElement',
    'HTMLInputElement',
    'HTMLSelectElement',
    'HTMLFormElement',
    'Node',
    'Element',
    'Text',
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
      value: window[key],
    });
  }
}
