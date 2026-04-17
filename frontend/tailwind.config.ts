import type { Config } from 'tailwindcss';

export default {
  darkMode: ['class', '[data-theme="dark"]'],
  content: ['./src/**/*.{html,js,svelte,ts}'],
  theme: {
    extend: {
      colors: {
        brand: {
          50: '#eff6ff',
          500: '#3b82f6',
          600: '#2563eb',
          700: '#1d4ed8',
        },
      },
      boxShadow: {
        focus: '0 0 0 3px rgba(37, 99, 235, 0.18)',
      },
      borderRadius: {
        xl: '0.875rem',
      },
    },
  },
} satisfies Config;
