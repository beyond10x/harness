---
format: aep.planning-md/3
id: story:started-record-carries-the-loops-cwd
kind: story
status: active
title: The started record carries the loop's working directory
summary: LoopEvent::Started gains cwd (absolute, canonicalised as the CLI resolves its workspace); the metaharness converter writes it to session.started.cwd. Additive.
relations:
- serves: vision:b10x-owns-its-loop
revision: 3
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T12:28:48Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-20T12:28:48Z", actor: "human:timo", revision: 3}
---
## Context

At harness 0.15.0 (`dd5c2a66`) `LoopEvent::Started` (`crates/harness-loop/src/event.rs:114`) has no
working-directory field; the `--json` writer serialises `LoopEvent` as it is
(`crates/harness-cli/src/render.rs:247-253`); the only path on the record is `context[].source` on
`project.instructions`, which is the instructions file; and harness's own metaharness converter
writes `"cwd": Value::Null` (`crates/harness-cli/src/metaharness.rs:358`). Confirmed at runtime by
metaharness's re-recorded 0.15.0 golden and its native fixture transcript (2026-10-20).

Consumer: metaharness `story:b10x-record-carries-the-loops-cwd` sets the b10x `session.started.cwd`
to `null` and its H7 row ("the working directory is ours") to `unk` because the record names none;
under `--hermetic strict --cwd <copy> --scratch-cwd` that run exits 3 today. A follow-up there reads
the field once a harness release writes it.

No released contract pins the `started` record: `contracts/cli/b10x-harness/2026-09-18/` pins argv
only, and `spec/` declares no loop event. The addition is additive.

Requested by metaharness through `b10x-chef-orchestrator` (2026-10-20), approved under the owner's
exception 5 (atlas ADR 0096 §7, the session format). Draft text:
`~/.cache/metaharness-wave-adr0096/w3/harness-draft-loop-cwd.md`.

## Outcome

`b10x-harness --json` names the working directory the loop runs in on its `started` record, so a
consumer that attests where the agent worked decides it from the loop's own record instead of from
the launcher's plan.

## Acceptance

- The `started` record carries `cwd`: the loop's working directory as an absolute path, canonicalised
  the way the CLI resolves its workspace (`crates/harness-cli/src/lib.rs:2501-2504`).
- The converter (`crates/harness-cli/src/metaharness.rs`) writes it into `session.started.cwd`.
- A test pins the field on a run launched from a known directory (the shipped binary, `--json`).
- A record without the field (an older loop's, or an embedder that sets none) still deserialises,
  and the converter still answers `null` for it.
- `CHANGELOG.md` names it as a record addition; existing fields unchanged; argv byte-identical
  (`cargo xtask cli-contract`).

## Scope

- `crates/harness-loop/src/event.rs` (`Started` gains `cwd`) and the loop configuration that carries
  it into `crates/harness-loop/src/lib.rs` (the `Started` emission, `lib.rs:2319`).
- `crates/harness-cli/src/lib.rs` (sets it from the resolved workspace), `src/metaharness.rs` (the
  converter), `src/render.rs` (test constructions).
- Every other construction of `LoopEvent::Started` in tests (`event.rs`, `render.rs`);
  `crates/harness-app-server/src/session.rs:320` matches with `..` and needs no change.
- `CHANGELOG.md`.

## Out of scope

- Any change to how the working directory is chosen.
- The rest of the `--json` `LoopEvent` record's format (atlas ADR 0096's open trace-format point).
