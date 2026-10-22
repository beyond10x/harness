import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';
import {withProductSite} from '@beyond10x/docs-system/product-site';

const config: Config = {
  title: 'Harness',
  tagline: 'The beyond10x agent loop: one loop between a model API and an explicitly bounded set of tools.',
  favicon: 'img/favicon.svg',

  future: {
    v4: true,
  },

  url: 'https://beyond10x.github.io',
  baseUrl: '/harness/',

  organizationName: 'beyond10x',
  projectName: 'harness',
  deploymentBranch: 'gh-pages',
  trailingSlash: false,

  onBrokenLinks: 'throw',
  onBrokenAnchors: 'throw',

  markdown: {
    format: 'detect',
    mermaid: true,
    hooks: {
      onBrokenMarkdownLinks: 'throw',
    },
  },

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  themes: ['@docusaurus/theme-mermaid'],

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          routeBasePath: 'docs',
          editUrl: 'https://github.com/beyond10x/harness/tree/main/website/',
        },
        blog: false,
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    navbar: {
      title: 'Harness',
      items: [
        {to: '/docs/', label: 'Docs', position: 'left'},
        {to: '/docs/getting-started', label: 'Get started', position: 'left'},
        {to: '/docs/guides/command-line', label: 'Guides', position: 'left'},
        {to: '/docs/reference/cli', label: 'CLI', position: 'left'},
        {to: '/docs/status', label: 'Status', position: 'left'},
        {href: 'https://github.com/beyond10x/harness', label: 'GitHub', position: 'right'},
      ],
    },
    footer: {
      links: [
        {
          title: 'Docs',
          items: [
            {label: 'Overview', to: '/docs/'},
            {label: 'First read-only run', to: '/docs/getting-started'},
            {label: 'Make a confined change', to: '/docs/tutorials/confined-change'},
            {label: 'Status', to: '/docs/status'},
            {label: 'Limitations', to: '/docs/limitations'},
          ],
        },
        {
          title: 'Concepts',
          items: [
            {label: 'The agent loop', to: '/docs/concepts/agent-loop'},
            {label: 'Tools and approvals', to: '/docs/concepts/tools-and-approvals'},
            {label: 'Security boundary', to: '/docs/concepts/security-boundary'},
          ],
        },
        {
          title: 'Guides',
          items: [
            {label: 'Use the command line', to: '/docs/guides/command-line'},
            {label: 'Providers and profiles', to: '/docs/guides/profiles'},
            {label: 'Confined workspaces', to: '/docs/guides/confinement'},
            {label: 'Sessions and events', to: '/docs/guides/sessions-and-events'},
          ],
        },
        {
          title: 'Reference',
          items: [
            {label: 'b10x-harness CLI', to: '/docs/reference/cli'},
            {label: 'Configuration', to: '/docs/reference/configuration'},
            {label: 'Toolchains', to: '/docs/reference/toolchains'},
            {label: 'Crates', to: '/docs/reference/crates'},
          ],
        },
        {
          title: 'Project',
          items: [
            {label: 'Source', href: 'https://github.com/beyond10x/harness'},
            {label: 'Changelog', href: 'https://github.com/beyond10x/harness/blob/main/CHANGELOG.md'},
            {label: 'Security policy', href: 'https://github.com/beyond10x/harness/security/policy'},
          ],
        },
      ],
      copyright: 'A beyond10x project. Harness · LicenseRef-B10x-Proprietary.',
    },
  } satisfies Preset.ThemeConfig,
};

export default withProductSite(config, {landing: './product.json', mark: 'Hs'});
