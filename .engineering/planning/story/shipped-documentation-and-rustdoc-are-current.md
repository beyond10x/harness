---
format: aep.planning-md/1
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
---
## Defect

Reader-facing pages still advertise older releases and a workspace-name restriction the code dropped. Strict rustdoc fails on broken private intra-doc links, so the gate does not currently prove all public reasoning remains connected.

## Acceptance

README, STATUS, ROADMAP where affected, and website state the same current behaviour without hand-written test counts. Broken links are fixed or rendered as non-links, and RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps is part of the Rust gate and passes. Website typecheck and build pass.
