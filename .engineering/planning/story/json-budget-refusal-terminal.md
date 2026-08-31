---
format: aep.planning-md/1
id: story:json-budget-refusal-terminal
kind: story
status: implemented
title: An unusable budget is one JSON refusal and no session
summary: Classify CLI budget validation as a pre-run refusal so JSONL callers receive the promised terminal record.
relations:
- derived_from: epic:pinned-interfaces-honest
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Context

A scratch-directory E2E run against `b10x-harness 0.4.0` at `8c8fdac` supplied
`--json --max-cost-microunits 1` without `--prices`. The loop correctly refused the unenforceable
bound before its first provider request, but `run_command` classified every `LoopError` as a failed
run. With `--no-session`, stdout was empty; with session filing enabled, stdout held only a
`session` event and a session file was created for a run whose `Started` event never existed.

`README.md` and `STATUS.md` say a run refused before the loop starts exits `1` and writes one
`{"kind":"refused"}` line under `--json`. This case contradicted that published behavior and left a
JSONL driver with no terminal explaining the exit.

## Acceptance

An unenforceable command-line budget exits `1` before any provider request, emits exactly one JSON
`refused` line naming the budget, and files no session, while the loop retains its own validation
for library callers.

## Evidence to produce

A shipped-binary E2E case must pin the exit status, the single stdout event, the named refusal,
absence of a transport attempt, and an empty declared session directory; `bash scripts/gate.sh`
must pass.
