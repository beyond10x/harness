---
format: aep.planning-md/3
id: story:pages-publishes-the-merged-site
kind: story
status: implemented
title: Pages publishes the merged site
summary: Enable and verify the Actions-built public site at the advertised URL.
tags:
- docs
relations:
- derived_from: epic:public-site-is-accurate-live-and-governed
- serves: vision:b10x-owns-its-loop
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T06:30:40Z", actor: "human:timo", revision: 2, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T06:30:40Z", actor: "human:timo", revision: 3, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T06:53:26Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
# Story: Pages publishes the merged site

## Outcome

Anyone can reach the Harness landing page and public documentation at the advertised GitHub Pages URL.

## Context

Every existing Pages run fails before build because the repository has no Pages site configured.

## Acceptance

- Pages uses GitHub Actions as its source and enforced HTTPS.
- The workflow builds the merged commit with the locked Node graph, TypeScript, and Docusaurus.
- The landing page, getting-started, CLI reference, and status routes return HTTP 200.
- The repository homepage names the deployed URL.

## Out of Scope

No custom domain or Atlas publication.

## Open Questions

None.
