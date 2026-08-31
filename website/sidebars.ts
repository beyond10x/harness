import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  docsSidebar: [
    {type: 'doc', id: 'index', label: 'What is Harness?'},
    {
      type: 'category',
      label: 'Tutorials',
      collapsed: false,
      items: [
        {type: 'doc', id: 'getting-started', label: 'First read-only run'},
        {type: 'doc', id: 'tutorials/confined-change', label: 'First confined change'},
      ],
    },
    {
      type: 'category',
      label: 'How-to guides',
      collapsed: false,
      items: [
        {type: 'doc', id: 'guides/profiles', label: 'Configure providers and profiles'},
        {type: 'doc', id: 'guides/confinement', label: 'Inspect and confine tools'},
        {type: 'doc', id: 'guides/sessions-and-events', label: 'Resume and consume events'},
        {type: 'doc', id: 'guides/structured-runs', label: 'Structure and extend a run'},
        {type: 'doc', id: 'guides/workflows', label: 'Run a workflow'},
      ],
    },
    {
      type: 'category',
      label: 'Concepts',
      collapsed: false,
      items: [
        {type: 'doc', id: 'concepts/agent-loop', label: 'The agent loop'},
        {type: 'doc', id: 'concepts/tools-and-approvals', label: 'Tools and approvals'},
        {type: 'doc', id: 'concepts/security-boundary', label: 'Security boundary'},
      ],
    },
    {
      type: 'category',
      label: 'Reference',
      collapsed: false,
      items: [
        {type: 'doc', id: 'reference/cli', label: 'Command line'},
        {type: 'doc', id: 'reference/toolchains', label: 'Toolchains'},
        {type: 'doc', id: 'reference/wires', label: 'Provider wires'},
        {type: 'doc', id: 'reference/configuration', label: 'Provider and profile configuration'},
        {type: 'doc', id: 'reference/workflows', label: 'Workflow format and events'},
        {type: 'doc', id: 'status', label: 'Status and limitations'},
      ],
    },
  ],
};

export default sidebars;
