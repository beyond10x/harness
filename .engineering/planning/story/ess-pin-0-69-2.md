---
format: aep.planning-md/3
id: story:ess-pin-0-69-2
kind: story
status: active
title: The specification runs on ESS 0.69.2
summary: spec/ess-inputs.yaml and the ess-* crates move from 0.67.0 to 0.69.2; generated projections regenerated
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: AGENTS.md
- confidence: cited
  path: Cargo.lock
- confidence: cited
  path: conformance
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
- {from: "draft", to: "proposed", at: "2026-10-26T10:45:03Z", actor: "human:timo", revision: 9}
- {from: "proposed", to: "active", at: "2026-10-26T10:45:04Z", actor: "human:timo", revision: 10}
---
## Context

The workspace rule (2026-10-05, "ALWAYS upgrade to most recent ESS") moves every repository to the
newest ESS release. Harness pins ESS 0.67.0 (`spec/ess-inputs.yaml:2`, the four `ess-*` crates by
rev `4579d804` in `crates/harness-conformance/Cargo.toml`). The newest release on 2026-10-26 is
0.69.2 (tag on `a3bc2df3`).

## Outcome

The specification is validated, compiled and synthesized by ESS 0.69.2, and the native
conformance suite runs on the 0.69.2 libraries.

## Acceptance

`cargo xtask specification` exits 0 with `spec/ess-inputs.yaml` requiring `ess 0.69.2` and the
`ess-*` crates pinned by rev `a3bc2df3df986e8d340553145e8615a00f982331`; `conformance/model.json`
and `conformance/suite.json` are what the 0.69.2 release binary writes
(`ess specify compile`, `ess verify conform synthesize --suite-format 5`).
