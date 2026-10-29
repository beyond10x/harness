---
format: aep.planning-md/3
id: story:ess-pin-0-76
kind: story
status: implemented
title: Harness pins ESS 0.76.0
summary: spec/ess-inputs.yaml and the ess-* crates of b10x-harness-conformance move to the 0.76.0 release.
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: AGENTS.md
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: crates/harness-conformance/Cargo.toml
- confidence: cited
  path: crates/harness-conformance/src/main.rs
- confidence: cited
  path: spec/README.md
- confidence: cited
  path: spec/ess-inputs.yaml
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T05:09:16Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-10-29T05:09:16Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-10-29T05:09:16Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1}}}
---
## Outcome

Harness specifies itself with ESS 0.76.0: `spec/ess-inputs.yaml` requires `ess 0.76.0` and the
`ess-*` crates of `b10x-harness-conformance` sit at the 0.76.0 release commit `d5c00e9c`.

## Evidence

- Requested by b10x-chef-orchestrator on 2026-10-29 under the operator's rule of 2026-10-05
  ("ALWAYS upgrade to most recent ESS"), to ride along in the async-loop wave's pull request.
- ESS's own replay ran harness at origin/main against 0.76.0: no exit code moved, generated output
  identical (orchestrator's message).
- The 0.76.0 library change (`TypeBinding::declared` returns `Option<&DeclaredTypeRef>`; ess-domain
  `Case` and `FieldCase` gain presence fields) touches nothing `b10x-harness-conformance` names: it
  builds and passes clippy unchanged.

## Acceptance

- `cargo xtask specification` passes on 0.76.0 with 29 of 29 scenarios and leaves `conformance/`
  byte-identical (`git status` shows no change under it).
- `cargo test -p b10x-harness-conformance --locked` and its clippy pass.
- `CHANGELOG.md` `[Unreleased]` names the new pin under Changed.

## Scope

Recorded as `scope` entries.
