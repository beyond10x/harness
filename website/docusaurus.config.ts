import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'Harness',
  tagline: 'A provider-neutral agent loop with explicit effects',
  favicon: 'img/favicon.svg',

  future: {
    v4: true,
  },

  url: 'https://beyond10x.github.io',
  baseUrl: '/harness/',
  organizationName: 'beyond10x',
  projectName: 'harness',
  trailingSlash: false,

  onBrokenLinks: 'throw',
  onBrokenAnchors: 'throw',
  markdown: {
    format: 'detect',
    hooks: {
      onBrokenMarkdownLinks: 'throw',
    },
  },

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      {
        docs: {
          path: './docs',
          routeBasePath: 'docs',
          sidebarPath: './sidebars.ts',
          editUrl: 'https://github.com/beyond10x/harness/edit/main/website/docs/',
          showLastUpdateTime: true,
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/social-card.svg',
    metadata: [
      {
        name: 'keywords',
        content:
          'AI agent harness, agent loop, tool calling, approvals, budgets, sessions, OpenAI Responses, Anthropic Messages, Rust',
      },
    ],
    colorMode: {
      defaultMode: 'dark',
      respectPrefersColorScheme: true,
    },
    navbar: {
      title: 'Harness',
      hideOnScroll: true,
      logo: {
        alt: 'Harness mark',
        src: 'img/mark.svg',
      },
      items: [
        {to: '/docs/getting-started', label: 'Get started', position: 'left'},
        {to: '/docs/guides/profiles', label: 'How-to', position: 'left'},
        {to: '/docs/concepts/agent-loop', label: 'Concepts', position: 'left'},
        {to: '/docs/reference/cli', label: 'CLI', position: 'left'},
        {to: '/docs/status', label: 'Status', position: 'left'},
        {
          href: 'https://github.com/beyond10x/harness',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Learn',
          items: [
            {label: 'Start here', to: '/docs/'},
            {label: 'First read-only run', to: '/docs/getting-started'},
            {label: 'First confined change', to: '/docs/tutorials/confined-change'},
            {label: 'Configure providers', to: '/docs/guides/profiles'},
          ],
        },
        {
          title: 'Operate',
          items: [
            {label: 'Sessions and events', to: '/docs/guides/sessions-and-events'},
            {label: 'Confined workspaces', to: '/docs/guides/confinement'},
            {label: 'Structured runs', to: '/docs/guides/structured-runs'},
            {label: 'Workflows', to: '/docs/guides/workflows'},
            {label: 'The agent loop', to: '/docs/concepts/agent-loop'},
            {label: 'Security boundary', to: '/docs/concepts/security-boundary'},
          ],
        },
        {
          title: 'Reference',
          items: [
            {label: 'Command line', to: '/docs/reference/cli'},
            {label: 'Toolchains', to: '/docs/reference/toolchains'},
            {label: 'Provider wires', to: '/docs/reference/wires'},
            {label: 'Configuration', to: '/docs/reference/configuration'},
            {label: 'Workflow format', to: '/docs/reference/workflows'},
            {label: 'Project status', to: '/docs/status'},
            {label: 'Security policy', href: 'https://github.com/beyond10x/harness/security/policy'},
            {label: 'GitHub repository', href: 'https://github.com/beyond10x/harness'},
          ],
        },
      ],
      copyright: `© ${new Date().getFullYear()} beyond10x · Publicly readable, proprietary source · An effect is either gated or it did not happen.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ['rust', 'json', 'bash'],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
