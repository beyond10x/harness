---
format: aep.planning-md/3
id: story:verbs-survives-a-bare-entry-call
kind: story
status: implemented
title: A verbs run survives a call to an entry by its own name
relations:
- derived_from: epic:measured-not-emulated
- serves: vision:b10x-owns-its-loop
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T12:51:12Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-10-29T12:51:12Z", actor: "human:timo", revision: 4}
- {from: "active", to: "implemented", at: "2026-10-29T12:51:13Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":2}}}
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

## Resolution

`llm` 0.17.0 adds `Controls::return_unpublished_calls` (default: refuse). `harness-llm` sets it on
every turn (`crates/harness-llm/src/lib.rs`, `to_llm_request`), so a call to a tool the turn did not
publish comes back to the loop, which routes a bare entry name over the catalogue or answers the
call as a failed outcome (invariant 9).

- Port test: `crates/harness-llm/tests/llm_0_17.rs`,
  `a_call_to_an_unpublished_entry_reaches_the_loop_instead_of_ending_the_turn`, a loopback
  Messages stream calling `find` with only `tool_search` published. With the control set to
  `false` the test fails, so it holds the setting.
- Live, 2026-10-29, Claude subscription login, `claude-haiku-4-5-20251001`:
  `measurements/runs/m2-live-verbs-{1,2,3}-llm017-2026-10-29.jsonl`, `vendor_live`. 3 of 3
  completed with the right answer; 6 calls (1 discovery, 0 failed), 4 turns, 14,121-14,432 input
  tokens, 17,823-18,638 micro-USD at list price per run, beside the flat arm's 12,866-13,252
  (`measurements/README.md`, *The same two measurements on `llm` 0.17.0*).
