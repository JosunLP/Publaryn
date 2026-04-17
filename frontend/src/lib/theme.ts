import { browser } from '$app/environment';
import { writable } from 'svelte/store';

export type ThemeMode = 'light' | 'dark';

const STORAGE_KEY = 'publaryn.theme';
const themeStore = writable<ThemeMode>('light');
let initialized = false;

export const themeMode = {
  subscribe: themeStore.subscribe,
};

export function initializeTheme(): void {
  if (!browser || initialized) {
    return;
  }

  initialized = true;
  const resolvedMode = resolveInitialTheme();
  themeStore.set(resolvedMode);
  applyThemeMode(resolvedMode);
}

export function toggleThemeMode(): ThemeMode {
  let nextMode: ThemeMode = 'light';

  themeStore.update((currentMode) => {
    nextMode = currentMode === 'dark' ? 'light' : 'dark';
    applyThemeMode(nextMode);
    return nextMode;
  });

  return nextMode;
}

function resolveInitialTheme(): ThemeMode {
  if (!browser) {
    return 'light';
  }

  const storedValue = window.localStorage.getItem(STORAGE_KEY);
  if (storedValue === 'dark' || storedValue === 'light') {
    return storedValue;
  }

  return window.matchMedia('(prefers-color-scheme: dark)').matches
    ? 'dark'
    : 'light';
}

function applyThemeMode(mode: ThemeMode): void {
  if (!browser) {
    return;
  }

  window.localStorage.setItem(STORAGE_KEY, mode);
  document.documentElement.dataset.theme = mode;
  document.documentElement.classList.toggle('dark', mode === 'dark');
  document.documentElement.style.colorScheme = mode;
}
