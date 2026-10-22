---
format: aep.planning-md/3
id: story:harness-events-is-retired
kind: story
status: active
title: harness events is retired; metaharness reads the --json record
summary: The events subcommand that rewrites a --json record as metaharness.event/1 is removed (Atlas ADR 0096 decision 7); metaharness project --from harness-json replaces it.
relations:
- serves: vision:b10x-owns-its-loop
revision: 3
transitions:
- {from: "draft", to: "proposed", at: "2026-10-22T02:18:25Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-22T02:18:25Z", actor: "human:timo", revision: 3}
---
## Context

metaharness owns the normalization of harness transcripts into `trace-ir/1` (atlas ADR 0096
decision 7, lines 71-73), and metaharness `story:retire-trace-formats-beside-trace-ir` keeps the
harness `--json` record as a source and retires the rewrite. metaharness 0.13.0 (`a34551e`) reads
the record itself: `metaharness project --from harness-json` (`crates/metaharness-cli/src/lib.rs:139`,
`tests/adversary_project_from.rs:126`). Relayed by `b10x-chef-orchestrator` (2026-10-22) from the
metaharness wave's handover.

`b10x-harness events` (`crates/harness-cli/src/lib.rs:455-485`, `:3919`, `:3981`;
converter `crates/harness-cli/src/metaharness.rs`, 1284 lines, at harness `0ee5655b`) rewrites a
`--json` record as `metaharness.event/1`. metaharness's own reader keeps the property its doc comment
argues for: `metaharness-b10x` runs in `DecisionMode::Observe` only and emits `Seam::None`
(metaharness `AGENTS.md` invariant 9).

## Consumers

Search of `~/beyond10x` on 2026-10-22 for `harness events`, `b10x-harness … events`, an `events`
argv element next to the harness binary, and `metaharness.event/1` (excluding `target/`,
`node_modules/`, `.git/`, `*.jsonl`):

| where | what | moves with this story |
|---|---|---|
| `crates/harness-cli/src/lib.rs`, `src/metaharness.rs` | the subcommand and converter | removed |
| `crates/harness-conformance/src/main.rs:92` | lists `events` among subcommands | yes |
| `crates/harness-cli/tests/started_cwd_adversary.rs:420` | runs `events` | yes |
| `crates/harness-llm/tests/adversary_public_surface.rs:372` | a `sink.events()` label, not the subcommand | check only |
| `docs/design/0003-workflow-runner.md:206`, `website/docs/guides/sessions-and-events.md:154`, `website/docs/reference/workflows.md:323`, `website/docs/reference/cli.md` | prose | yes |
| metaharness `website/docs/harnesses/b10x.md:105` | prose comparing the two converters | stale after removal; metaharness's to edit |

No code outside harness runs the subcommand. Every other `metaharness.event/1` hit (metaharness,
aep, swarm, agentplugins) produces or reads metaharness's own stream, not this converter's output.
The list must be re-run, and empty outside harness, before this story leaves draft.

## Outcome

`b10x-harness events` is gone; the `--json` loop record stays the harness's own transcript, and
metaharness turns it into `trace-ir/1`.

## Acceptance

- Before: `b10x-harness events --in <record> --out <file>` exits 0 and writes `metaharness.event/1`.
  After: clap refuses `events` as an unknown subcommand; the conformance binary and the tests above
  no longer name it.
- A new `contracts/cli/b10x-harness/<date>/` version without `events`; released versions
  byte-identical (invariant 13).
- The `--json` record is unchanged: its pinned fixtures are byte-identical.
- Evidence that `metaharness project --from harness-json --to trace-ir` reads a record written at the
  new harness head comes from metaharness's side (its `crates/metaharness-b10x/fixtures/golden/loop.jsonl`
  re-recorded, or an atlas integration test). A harness test that runs the metaharness binary would
  break invariant 2; the handover's acceptance asked for one, and this is the open point to settle
  with metaharness before `proposed`.
- `CHANGELOG.md`: one line under `Removed` naming `metaharness project --from harness-json`.
- `cargo xtask gate` exits 0.

## Scope

- `crates/harness-cli/src/lib.rs` (`Events`, `EventsOptions`, `events_command`, `mod metaharness`)
- `crates/harness-cli/src/metaharness.rs` (removed)
- `crates/harness-cli/src/contract.rs`, `contracts/cli/b10x-harness/<date>/`
- `crates/harness-conformance/src/main.rs`
- `crates/harness-cli/tests/started_cwd_adversary.rs`
- `docs/design/0003-workflow-runner.md`
- `website/docs/guides/sessions-and-events.md`, `website/docs/reference/cli.md`,
  `website/docs/reference/workflows.md`
- `CHANGELOG.md`

## Out of scope

- The `--json` record's format.
- metaharness's docs page `website/docs/harnesses/b10x.md:105`.

## Sequencing

After 0.17.1 (`b10x-chef-orchestrator`, 2026-10-22). Leaves draft only when the consumer list
outside harness is empty or each consumer has moved.
