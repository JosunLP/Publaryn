import { effect, persistedSignal } from '@bquery/bquery/reactive';
import { applyThemeTokens } from '@bquery/ui/theme';

export type ThemeMode = 'light' | 'dark';

const themeMode = persistedSignal<ThemeMode>('publaryn.theme', 'light');
let initialized = false;

export function initializeTheme(): void {
  if (initialized || typeof document === 'undefined') {
    return;
  }

  initialized = true;

  applyThemeTokens({
    '--bq-color-primary-600': '#2563eb',
    '--bq-color-primary-700': '#1d4ed8',
    '--bq-color-success-600': '#16a34a',
    '--bq-color-warning-600': '#d97706',
    '--bq-color-danger-600': '#dc2626',
    '--bq-radius-lg': '0.75rem',
    '--bq-font-family-sans':
      'Inter, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
  });

  effect(() => {
    document.documentElement.dataset.theme = themeMode.value;
    document.documentElement.style.colorScheme = themeMode.value;
  });
}

export function getThemeMode(): ThemeMode {
  return themeMode.value;
}

export function toggleThemeMode(): ThemeMode {
  themeMode.value = themeMode.value === 'dark' ? 'light' : 'dark';
  return themeMode.value;
}
