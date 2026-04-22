import { defineConfig } from 'vitepress';

const repositoryName = 'Publaryn';
const base =
  process.env.GITHUB_ACTIONS === 'true' ? `/${repositoryName}/` : '/';

export default defineConfig({
  title: 'Publaryn',
  description:
    'Security-first, self-hostable multi-ecosystem package registry built in Rust.',
  lang: 'en-US',
  base,
  cleanUrls: true,
  lastUpdated: true,
  themeConfig: {
    siteTitle: 'Publaryn Docs',
    nav: [
      { text: 'Start', link: '/' },
      { text: 'Product', link: '/product/README' },
      { text: 'Architecture', link: '/architecture/README' },
      { text: 'Operations', link: '/operations/README' },
      { text: '1.0 Contract', link: '/1.0' },
      { text: 'API Routes', link: '/api-routes' },
      { text: 'Reference', link: '/reference/README' },
      { text: 'ADRs', link: '/adr/README' },
      { text: 'Releases', link: '/releases/README' },
    ],
    sidebar: {
      '/': [
        {
          text: 'Start here',
          items: [
            { text: 'Documentation home', link: '/' },
            { text: '1.0 release contract', link: '/1.0' },
            { text: 'Release checklist', link: '/release-checklist' },
            { text: 'API and adapter routes', link: '/api-routes' },
          ],
        },
        {
          text: 'Guides',
          items: [
            { text: 'Product guide', link: '/product/README' },
            { text: 'Architecture overview', link: '/architecture/README' },
            { text: 'Operations guide', link: '/operations/README' },
            { text: 'Reference hub', link: '/reference/README' },
          ],
        },
        {
          text: 'Product and architecture',
          items: [
            { text: 'Product concept', link: '/concept' },
            { text: 'ADR index', link: '/adr/README' },
          ],
        },
        {
          text: 'Operations and releases',
          items: [
            { text: 'Release notes', link: '/releases/' },
            {
              text: 'Operator job queue recovery',
              link: '/operator/job-queue-recovery',
            },
          ],
        },
      ],
      '/product/': [
        {
          text: 'Product guide',
          items: [
            { text: 'Product overview', link: '/product/README' },
            { text: '1.0 release contract', link: '/1.0' },
            { text: 'Product concept', link: '/concept' },
          ],
        },
      ],
      '/architecture/': [
        {
          text: 'Architecture',
          items: [
            { text: 'Architecture overview', link: '/architecture/README' },
            { text: 'ADR index', link: '/adr/README' },
            { text: 'Product concept', link: '/concept' },
          ],
        },
      ],
      '/operations/': [
        {
          text: 'Operations',
          items: [
            { text: 'Operations guide', link: '/operations/README' },
            { text: 'Release checklist', link: '/release-checklist' },
            {
              text: 'Operator job queue recovery',
              link: '/operator/job-queue-recovery',
            },
            { text: 'Release notes', link: '/releases/README' },
          ],
        },
      ],
      '/reference/': [
        {
          text: 'Reference',
          items: [
            { text: 'Reference hub', link: '/reference/README' },
            { text: 'API and adapter routes', link: '/api-routes' },
            { text: '1.0 release contract', link: '/1.0' },
            { text: 'ADR index', link: '/adr/README' },
          ],
        },
      ],
      '/adr/': [
        {
          text: 'Architecture decision records',
          items: [{ text: 'ADR index', link: '/adr/README' }],
        },
      ],
      '/releases/': [
        {
          text: 'Release notes',
          items: [
            { text: 'Release notes index', link: '/releases/README' },
            { text: 'Publaryn 1.0.0', link: '/releases/1.0.0' },
            { text: 'Release checklist', link: '/release-checklist' },
          ],
        },
      ],
      '/operator/': [
        {
          text: 'Operator runbooks',
          items: [
            {
              text: 'Job queue recovery',
              link: '/operator/job-queue-recovery',
            },
          ],
        },
      ],
    },
    search: {
      provider: 'local',
    },
    socialLinks: [
      { icon: 'github', link: 'https://github.com/JosunLP/Publaryn' },
    ],
    editLink: {
      pattern: 'https://github.com/JosunLP/Publaryn/edit/main/docs/:path',
      text: 'Edit this page on GitHub',
    },
    footer: {
      message: 'Dual-licensed under Apache-2.0 and MIT.',
      copyright: 'Copyright © Publaryn contributors',
    },
  },
});
