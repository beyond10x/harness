---
format: aep.planning-md/3
id: story:ess-pin-0-75
kind: story
status: active
title: Harness runs its specification on ESS 0.75.0, the newest release
summary: spec/ess-inputs.yaml and the conformance crate's ess-* pins move from 0.74.0 to 0.75.0
tags:
- d142
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
revision: 10
transitions:
- {from: "draft", to: "proposed", at: "2026-10-28T17:45:32Z", actor: "human:timo", revision: 9}
- {from: "proposed", to: "active", at: "2026-10-28T17:45:32Z", actor: "human:timo", revision: 10}
---
## Context

`b10x-chef-orchestrator`, 2026-10-28: "ESS 0.75.0 is released (tag on ess main b593d356,
verify-release exit 0). Pin it in harness in your open pin PR". Operator rule 2026-10-05: always
the newest ESS. Verified: ESS release `0.75.0` (published 2026-10-28T14:48:46Z), annotated tag
object `c3a4bd84` on commit `b593d3567cd03070d16d67b2a936f0b8c22c276f`. Its API note
(`Note::CrossCallerUnswapped` gains `mixed`, `ChangedRowCause` gains `SetMoveOnly`) touches nothing
harness names: `git grep` finds neither in `crates/`.

## Acceptance

- `spec/ess-inputs.yaml` requires `ess 0.75.0`; the four `ess-*` dependencies of
  `b10x-harness-conformance` are at rev `b593d356`; `Cargo.lock` changes only `ess-*` packages.
- The full gate is green: the native ESS suite 29/29 passed, the inert audit 0/29, and
  `conformance/model.json` and `suite.json` regenerate byte-identical.
