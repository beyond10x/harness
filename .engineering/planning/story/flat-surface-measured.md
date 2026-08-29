---
format: aep.planning-md/1
id: story:flat-surface-measured
kind: story
status: draft
title: The flat surface's cost against a real provider is measured
relations:
- derived_from: epic:measured-not-emulated
revision: 2
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
