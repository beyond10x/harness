---
format: aep.planning-md/3
id: story:docs-on-the-repository-site
kind: story
status: active
title: Harness documentation is served from its own site
summary: The docs leave the unified site for https://beyond10x.github.io/harness/, by the workspace docs skill stages A-D.
relations:
- serves: vision:b10x-owns-its-loop
revision: 3
transitions:
- {from: "draft", to: "proposed", at: "2026-10-22T02:18:25Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-22T02:18:25Z", actor: "human:timo", revision: 3}
---
## Context

Timo, 2026-10-22, relayed by `b10x-chef-orchestrator`: "https://beyond10x.github.io/docs/harness/
<- this is using the old format, I need this outside this ugly website ... just like mandate,
conductor". On 2026-10-22 `https://beyond10x.github.io/harness/` answers "Documentation moved" and
`/docs/harness/` serves the unified site; conductor serves its own Docusaurus site from
`website/` with `pages.yml` and `b10x-docs-site.yml`.

## Outcome

The harness documentation is served by the repository's own site at
`https://beyond10x.github.io/harness/`, and the former unified routes redirect to it.

## Acceptance

- `curl -sL https://beyond10x.github.io/harness/` returns a page titled for Harness, not
  "Documentation moved".
- A former `/docs/harness/…` route redirects to its page under `/harness/`.
- `Documentation validation` and `Documentation site` runs are green on the landing commit.
- `b10x.docs.yaml`, `b10x-docs-bundle.yml`, `b10x-docs-pages.yml` and the unified-site docs check
  are deleted, after the organization side has landed.
- The repository's GitHub website field and README name the new site.
- `cargo xtask gate` exits 0; the CLI reference stays generated from clap.

## Scope

- `website/`, `.github/workflows/pages.yml`, `.github/workflows/b10x-docs-site.yml`
- `.github/workflows/b10x-docs-bundle.yml`, `.github/workflows/b10x-docs-pages.yml`,
  `.github/workflows/b10x-docs-check.yml`, `b10x.docs.yaml` (deleted last)
- `crates/harness-xtask/src/website_contract.rs`, `README.md`, `AGENTS.md`
- Organization side: Website, beyond10x.github.io and Atlas, harness entries only

## Out of scope

- Any other repository's documentation.
