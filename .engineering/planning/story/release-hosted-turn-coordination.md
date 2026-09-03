---
format: aep.planning-md/1
id: story:release-hosted-turn-coordination
kind: story
status: implemented
title: Release hosted turn coordination
summary: Publish the hosted context and durable approval APIs as a pinned Harness release.
relations:
- derived_from: epic:embedded-by-a-consumer
- serves: vision:b10x-owns-its-loop
scope:
- confidence: inferred
  path: CHANGELOG.md
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: Cargo.toml
- confidence: inferred
  path: README.md
- confidence: inferred
  path: website/docs/index.md
- confidence: inferred
  path: website/docs/status.md
revision: 5
---
## Outcome

Harness 0.11.1 publishes the already-landed per-turn environment and durable approval checkpoint APIs so AgentIDE can depend on a released revision.

## Scope

- Version and release documentation only; the implementation is already on `main`.
- Verify the complete repository gate before the bot-authored release commit and tag.

## Acceptance

- The workspace and public status surfaces name 0.11.1.
- `cargo xtask gate` passes from the exact release tree.
- The annotated 0.11.1 tag points at the gated bot-authored main commit.
