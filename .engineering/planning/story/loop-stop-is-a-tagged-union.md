---
format: aep.planning-md/3
id: story:loop-stop-is-a-tagged-union
kind: story
status: active
title: The loop stop is specified as a tagged union
summary: Declare harness.loop.Stop as an ESS union tagged by kind, matching LoopStop serde shape
relations:
- informed_by: executable-system-specification:harness-runtime
- serves: vision:b10x-owns-its-loop
scope:
- confidence: inferred
  path: conformance/model.json
- confidence: inferred
  path: conformance/suite.json
- confidence: inferred
  path: spec/COVERAGE.md
- confidence: inferred
  path: spec/domains/loop.yaml
revision: 9
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T20:48:00Z", actor: "human:timo", revision: 8}
- {from: "proposed", to: "active", at: "2026-10-29T20:48:01Z", actor: "human:timo", revision: 9}
---
## Outcome

`harness.loop.Stop` in `spec/domains/loop.yaml` is declared as an ESS tagged union
(`kind: union`, `tag: kind`, one variant per `LoopStop` variant with exactly that variant's
fields), matching the Rust type's own serde shape
(`crates/harness-loop/src/lib.rs:126`: `#[serde(tag = "kind", rename_all = "kebab-case",
deny_unknown_fields)]`). Today it is a struct with a `StopKind` enum and twelve optional fields
(`spec/domains/loop.yaml:24-38`), so the specification admits combinations the type cannot
produce (a `completed` stop carrying `limit_ms`, a `max-cost` stop without `spent_micro_usd`).

## Acceptance

- `spec/domains/loop.yaml` declares `harness.loop.Stop` as a union tagged by `kind`, with one
  variant per `LoopStop` variant (`crates/harness-loop/src/lib.rs:127-197`, `Failed` included),
  each carrying exactly that variant's fields with their types and optionality; the
  `harness.loop.StopKind` enum and the optional-field struct are gone, and the header comment at
  `spec/domains/loop.yaml:7-10` no longer says payload correlations are left to scenarios.
- Every scenario under `spec/scenarios/` still validates and executes against the real libraries:
  `cargo xtask specification` exits 0 with the same scenario count as before the change, and
  `conformance/model.json` and `conformance/suite.json` are regenerated (no `StopKind` left).
- A test in `crates/harness-conformance/tests/` compiles the specification with one scenario whose
  expected `completed` stop carries `limit_ms`, and asserts that ESS refuses it with a named
  diagnostic. If ESS 0.77.0 admits it, the test is not weakened: that is reported as an ESS gap
  and the story stays open.
- `spec/COVERAGE.md:13-16` states that the stop is a tagged union and no longer names `StopKind`
  or "a struct with optional variant fields".

## Source

ESS supports tagged unions: `ess crates/specify/ess-domain/src/types.rs:1178-1180` refuses an
untagged one, example `crates/generate/ess-synth/tests/json_web.rs:39-43` (relayed by the
orchestrator from the ess session, 2026-10-22). Pinned ESS was 0.67.0 when this was filed; wave d221
(2026-10-29) moves it to 0.77.0 (`spec/ess-inputs.yaml`) first. Since harness 0.23.0 `LoopStop`
also has the variant `Failed { cause, message, after_compaction }` (`crates/harness-loop/src/lib.rs:192`).

Revised 2026-10-29 from the acceptance critic's round (wave d221): the variant-exact clause moved
into the acceptance, the negative check is one named test, the COVERAGE citation is corrected.
