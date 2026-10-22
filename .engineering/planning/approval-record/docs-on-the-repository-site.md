---
format: aep.planning-md/3
id: approval-record:docs-on-the-repository-site
kind: approval-record
status: draft
title: Timo approved moving the harness docs to the repository site
tags:
- b10x-chef-orchestrator
relations:
- decides: story:docs-on-the-repository-site
revision: 2
---
## The stop

Moving the harness documentation off the unified site (`https://beyond10x.github.io/docs/harness/`)
to the repository's own site at `https://beyond10x.github.io/harness/`, following the workspace
`docs` skill's stages A–D, with the unified-site files deleted last, modeled on mandate and
conductor. Once the `workflow` removal has landed, the pages do not mention it.

## Who decided

Timo, 2026-10-22 ~04:4x, relayed by session `b10x-chef-orchestrator` under its grant. Verbatim:
"https://beyond10x.github.io/docs/harness/ <- this is using the old format, I need this outside
this ugly website ... just like mandate, conductor".

## What it authorises

The repository's own site and its workflows, the organization-side steps of the docs skill's
Stage C (Website, root caller, Atlas catalog) for harness only, the deletion of harness's
unified-site files, and the GitHub website field. On 2026-10-22 conductor serves its own site;
`https://beyond10x.github.io/mandate/` still answers "Documentation moved".

## Follow-up approval: docs-system #31

2026-10-22, session `b10x-chef-orchestrator` under its grant: "once those are green, merge #31
through the bot route and then finish Stage C". docs-system PR #31
(`https://github.com/beyond10x/docs-system/pull/31`) drops `harness/docs` from the surfaces of
`changes/docs-system-0.3.3.yaml`, because `atlas docs snapshot` refused it with
`docs-system/passive-docusaurus-markers-0.3.3 affects non-public surface harness/docs`.
