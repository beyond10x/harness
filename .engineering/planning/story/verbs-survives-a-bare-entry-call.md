---
format: aep.planning-md/3
id: story:verbs-survives-a-bare-entry-call
kind: story
status: draft
title: A verbs run survives a call to an entry by its own name
relations:
- derived_from: epic:measured-not-emulated
- serves: vision:b10x-owns-its-loop
revision: 1
---
## Evidence

- `measurements/runs/m2-live-verbs-{1,2,3}-2026-10-29.jsonl` and their `.stderr`: under
  `--surface verbs` on `claude-haiku-4-5-20251001`, `llm` 0.16.0, all three runs ended on turn 2 with
  `model wire refused: Protocol: model output contains a duplicate call or unpublished tool`.
- A fourth run through a local recording proxy (2026-10-29, not committed): turn 2's response
  streamed `tool_use` blocks named `find` and `file_read`, entry names the verbs surface does not
  publish; the provider accepted them.
- `llm` 0.16.0 `crates/llm-core/src/turn.rs`, `TurnOutcome::validate_for`: a call to a tool the
  request did not publish is a protocol error for the whole turn.
- `crates/harness-loop/src/lib.rs`: the loop answers an unpublished call as a failed outcome and
  routes a bare entry name over a catalogue (`unpublished-tool`, `unpublished-tool-routed`).
  Invariant 9: an unpublished tool is an outcome the model learns about, never the end of the run.

## Context

Since the wires moved to `llm`, the outcome check in `llm-core` refuses the turn before the loop
sees the call, so the loop's routing is unreachable on a live provider and a model that names an
entry directly ends a verbs run. The fix belongs in `llm` (return the call to the caller, or let the
caller opt out of that check); harness then re-pins `llm`.

## Acceptance

Three live `--surface verbs` runs of `prompts/surface-task.txt` through the re-pinned `llm` reach a
terminal the loop chose (completed or a named budget stop), none ended by the unpublished-name
refusal, recorded as `vendor_live` beside the 2026-10-29 flat runs, with tool calls, discovery
calls, tokens and cost reported per arm.
