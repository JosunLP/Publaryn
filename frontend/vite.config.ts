import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [sveltekit()],
  server: {
    port: 5173,
    proxy: {
      '/v1': 'http://localhost:3000',
      '/npm': 'http://localhost:3000',
      '/pypi': 'http://localhost:3000',
      '/cargo': 'http://localhost:3000',
      '/nuget': 'http://localhost:3000',
      '/health': 'http://localhost:3000',
      '/readiness': 'http://localhost:3000',
      '/_/': 'http://localhost:3000',
      '/swagger-ui': 'http://localhost:3000',
    },
  },
});
