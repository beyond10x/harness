---
format: aep.planning-md/3
id: story:flat-surface-measured
kind: story
status: implemented
title: The flat surface's cost against a real provider is measured
relations:
- derived_from: epic:measured-not-emulated
- serves: vision:b10x-owns-its-loop
- depends_on: story:measurement-apparatus-proved-on-the-emulator
scope:
- confidence: inferred
  path: STATUS.md
- confidence: inferred
  path: measurements
revision: 7
transitions:
- {from: "draft", to: "proposed", at: "2026-09-15T17:26:30Z", actor: "human:timo", revision: 3, imported: true}
- {from: "proposed", to: "active", at: "2026-10-29T11:10:47Z", actor: "human:timo", revision: 5}
- {from: "active", to: "implemented", at: "2026-10-29T11:11:27Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"test_result":1}}}
---
## Evidence

- `STATUS.md:14` — "published **flat by default** (every entry its own tool, its own schema) or under `tool_search`/`tool_describe`/`tool_invoke`"; next evidence: "measure what the flat surface costs or saves on a real provider. The three verbs' 33–44% discovery overhead is measured; the flat surface's schema-validation behaviour is not".
- `crates/harness-tools/src/flat.rs:7` — "Across three live runs, **33% to 44% of every tool call was** …" — the figure the flat surface was built against.
- `crates/harness-tools/src/verbs.rs:24` — the same measurement from the verbs' side, with 12.2% of something else beside it.
- `docs/reviews/2026-08-29-sota-comparison.md:62` — the finding: the three-verb indirection costs a third of tool calls to discovery, and `tool_invoke.arguments` is an untyped object with `strict: false`, so the provider cannot validate arguments.

## Context

The verbs surface was measured and found expensive; the flat surface that replaced it as the default
has not been measured at all. The half that matters most is the one the verbs could not have: with a
schema per entry, the provider itself rejects a malformed call before it is sent. Whether that
actually happens, and how often it saves a round trip, is a property of the provider, not of this
code.

Both surfaces remain shipped — `verbs` still serves metaharness's MCP surface and an arm that compares
them — so this measurement also decides whether keeping two surfaces is still worth its cost.

## Acceptance

One live comparison of the same task under `--surface flat` and `--surface verbs`, reporting tool
calls, tokens and provider-side argument rejections for each, retained as `vendor_live`.

## Resolution

Implemented on 2026-10-29 under `approval-record:wave-live-pins-d200`. Three pairs on
`claude-haiku-4-5-20251001` (`measurements/runs/m2-live-{flat,verbs}-{1,2,3}-2026-10-29.jsonl`):
flat completed 3 of 3 (6 calls, 0 discovery, 0 failed, 3-4 turns, 12,866-13,252 micro-USD at list
price); verbs completed 0 of 3, each ended on turn 2 by `llm-core` refusing calls to `find` and
`file_read` by their entry names. Provider-side argument rejections: 0 on both arms. The verbs
arm's cost is not measured; `story:verbs-survives-a-bare-entry-call` carries the fix in `llm` and
the re-measure.
