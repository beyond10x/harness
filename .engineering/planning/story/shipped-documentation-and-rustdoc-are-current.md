---
format: aep.planning-md/3
id: story:shipped-documentation-and-rustdoc-are-current
kind: story
status: implemented
title: README, STATUS, website, and rustdoc describe the shipped tree
summary: Release, workspace, and API documentation agree and strict documentation builds.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T01:56:32Z", actor: "human:timo", revision: 2, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T01:56:32Z", actor: "human:timo", revision: 3, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T03:29:37Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":2}}, imported: true}
---
## Defect

Reader-facing pages still advertise older releases and a workspace-name restriction the code dropped. Strict rustdoc fails on broken private intra-doc links, so the gate does not currently prove all public reasoning remains connected.

## Acceptance

README, STATUS, ROADMAP where affected, and website state the same current behaviour without hand-written test counts. Broken links are fixed or rendered as non-links, and RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps is part of the Rust gate and passes. Website typecheck and build pass.
