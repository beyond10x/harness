---
format: aep.planning-md/3
id: story:failed-run-ends-with-a-finished-line
kind: story
status: active
title: A run that fails ends with a finished line naming its cause
summary: 'LoopError exits emit finished{stop: failed, cause, message, after_compaction}'
relations:
- decomposes: epic:embedded-by-a-consumer
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: conformance/model.json
- confidence: cited
  path: crates/harness-cli/src/render.rs
- confidence: cited
  path: crates/harness-cli/tests/end_to_end.rs
- confidence: cited
  path: crates/harness-cli/tests/fixtures/mcp_endpoint.rs
- confidence: inferred
  path: crates/harness-conformance
- confidence: cited
  path: crates/harness-loop/src/lib.rs
- confidence: cited
  path: crates/harness-loop/src/tests.rs
- confidence: cited
  path: spec/domains/loop.yaml
revision: 11
transitions:
- {from: "draft", to: "proposed", at: "2026-10-26T21:05:42Z", actor: "human:timo", revision: 10}
- {from: "proposed", to: "active", at: "2026-10-26T21:05:42Z", actor: "human:timo", revision: 11}
---
## Context

A one-hour trial on 2026-10-26 ran an agent built on Harness 0.20.0 through metaharness
(`b10x-harness run ... --execution-path metaharness --json --no-session --approve-up-to high`, an
openai-responses endpoint, a 128,000-token declared window). 7 of 41 Harness steps ended without the
JSON line naming their action. The three transcripts read end right after a
`conversation-compacted` warning (109,063, 125,439 and 120,978 of 128,000 declared tokens), with an
empty next turn; 9 other compacted sessions finished. The transcripts themselves were not kept; the
figures are the trial record's.

What the caller saw: metaharness's b10x adapter turns the `finished` line into `session.ended`
(`metaharness-b10x/src/seam.rs`, the `"finished"` arm: `is_error = kind != "completed"`, `subtype =
kind`) and invents nothing when the stream ends without one (`finish`). The Loom session executor
then reports `the session ended in an error (<kind>)` when there is a `session.ended`, and otherwise
only `the session ended without the JSON line naming its action`.

What Harness does when a started run fails: `AgentLoop::run_over` emits `finished` only on `Ok`
(`crates/harness-loop/src/lib.rs`, `run_over`); on a `LoopError` it returns the error and the
stream simply stops. `b10x-harness run --json` prints a terminal line only for a run that never
started (`refused_line`, `stopped` in `crates/harness-cli/src/lib.rs`); a run that started and
failed writes `error: ...` to stderr and exits 1, with nothing on stdout naming the end.

Approved by session `b10x-chef-orchestrator`, 2026-10-26 (`approval-record:wave-llm-0-16-final-line`).

## Outcome

Every run that started ends its event stream with exactly one `finished` event, and a run that
failed names its cause in it, so a caller reading the stream records why the run ended instead of
"ended without the JSON line".

## Acceptance

1. Reproduction first (red on `main`): a run with a declared context window that compacts near the
   window, after which the next model request fails on the wire. Today its event stream ends after
   `turn-started` with no `finished`, and `b10x-harness run --json` against a loopback endpoint
   prints no terminal line.
2. After the change the same run's last event is
   `{"kind":"finished","stop":{"kind":"failed","cause":"<cause>","message":"<one line>","after_compaction":true},"turns":N}`,
   once, and `AgentLoop::run_in` still returns the same `LoopError`. `cause` names the class:
   `provider-<wire error code>` (`provider-refused`, `provider-transport`, `provider-protocol`,
   `provider-unauthorized`, `provider-rate-limited`, `provider-too-large`, `provider-unsupported`),
   `environment`, `budget` or `config`. `after_compaction` is true exactly when the failing request
   was the first one after a compaction.
3. Under `--json` the binary's last stdout line is that `finished` line, and the exit status of a
   started run that failed stays 1.

## Out of scope

- Recovering from a context-overflow refusal by compacting and retrying (its own story, if the
  provider's refusal is confirmed as the cause).
- metaharness carrying `cause` and `message` into `session.ended` (metaharness's store).
- A process killed from outside (child exit): Harness cannot write a line after a signal; that end
  is metaharness's to name.
