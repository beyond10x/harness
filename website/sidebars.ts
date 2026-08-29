import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  docsSidebar: [
    {type: 'doc', id: 'index', label: 'What is Harness?'},
    {type: 'doc', id: 'getting-started', label: 'Getting started'},
    {
      type: 'category',
      label: 'Understand',
      collapsed: false,
      items: [
        {type: 'doc', id: 'concepts/agent-loop', label: 'The agent loop'},
        {type: 'doc', id: 'concepts/tools-and-approvals', label: 'Tools and approvals'},
        {type: 'doc', id: 'concepts/security-boundary', label: 'Security boundary'},
      ],
    },
    {
      type: 'category',
      label: 'Operate',
      collapsed: false,
      items: [
        {type: 'doc', id: 'guides/sessions-and-events', label: 'Sessions and events'},
        {type: 'doc', id: 'guides/confinement', label: 'Confined workspaces'},
        {type: 'doc', id: 'guides/structured-runs', label: 'Structured runs, delegates and hooks'},
        {type: 'doc', id: 'guides/workflows', label: 'Workflows'},
      ],
    },
    {
      type: 'category',
      label: 'Reference',
      collapsed: false,
      items: [
        {type: 'doc', id: 'reference/cli', label: 'Command line'},
        {type: 'doc', id: 'reference/wires', label: 'Provider wires'},
      ],
    },
    {type: 'doc', id: 'status', label: 'Status and limitations'},
  ],
};

export default sidebars;
