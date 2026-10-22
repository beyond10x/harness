---
format: aep.planning-md/3
id: story:loop-stop-is-a-tagged-union
kind: story
status: draft
title: The loop stop is specified as a tagged union
summary: Declare harness.loop.Stop as an ESS union tagged by kind, matching LoopStop serde shape
relations:
- informed_by: executable-system-specification:harness-runtime
revision: 1
---
## Outcome

`harness.loop.Stop` in `spec/domains/loop.yaml` is declared as an ESS tagged union
(`kind: union`, `tag: kind`, one variant per `LoopStop` variant with exactly that variant's
fields), matching the Rust type's own serde shape
(`crates/harness-loop/src/lib.rs:121-123`: `#[serde(tag = "kind", rename_all = "kebab-case",
deny_unknown_fields)]`). Today it is a struct with a `StopKind` enum and ten optional fields, so
the specification admits combinations the type cannot produce (a `completed` stop carrying
`limit_ms`, a `max-cost` stop without `spent_micro_usd`).

## Acceptance

- `spec/domains/loop.yaml` declares the stop as a union tagged by `kind`; the `StopKind` enum and
  the optional-field struct are gone, and every scenario under `spec/scenarios/` still validates
  and executes against the real libraries (`cargo xtask specification` exit 0).
- A scenario whose expected stop carries a field its variant does not have fails validation (shown
  by a negative fixture or the inert-target run).
- `spec/COVERAGE.md:15-19` no longer says the stop envelope is a struct with optional variant
  fields.

## Source

ESS supports tagged unions: `ess crates/specify/ess-domain/src/types.rs:1178-1180` refuses an
untagged one, example `crates/generate/ess-synth/tests/json_web.rs:39-43` (relayed by the
orchestrator from the ess session, 2026-10-22). Pinned ESS is 0.67.0 (`spec/ess-inputs.yaml`).
