---
format: aep.planning-md/3
id: review-result:adversary-undeclared-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:an-undeclared-field-is-refused (unit over 520b1f94)
relations:
- reviews: story:an-undeclared-field-is-refused
revision: 1
---
undeclared-field U1 adversary pass 1: CONFIRMED, 3 red cases, 2 warnings, 1 note. Uncommitted unit over
520b1f94 (tree harness-undeclared-u1, branch impl/an-undeclared-field-is-refused).

Added `crates/harness-catalogue/tests/an_undeclared_field_is_refused_adversary.rs` (2 cases) and one case
at the end of `crates/harness-loop/src/tests.rs`. Workspace `--no-fail-fast`: 1079 ran, 1076 passed, 3
failed (these), 6 ignored; `cargo xtask cli-contract` 12 pinned versions verified; clippy exit 0.

Findings:

1. `crates/harness-catalogue/src/catalogue.rs:437` (warning, introduced) — through `AgentLoop::invoke` a
   catalogue call with an undeclared field is refused only inside the port, after the approver and
   the `before-call` hook ran and with `after-call` firing for a write that never ran; under `DenyAll`
   the reply never names the field.
2. `crates/harness-wire/src/undeclared.rs:113` (warning, introduced) — every array element is judged by
   `items`, ignoring `prefixItems`, so an answer valid against an operator's 2020-12 output schema is
   refused where 0.16.0 accepted it.
3. `crates/harness-mcp/src/lib.rs:188` (note, pre-existing) — MCP calls are forwarded untouched; a
   closed MCP schema is not enforced on harness's side, as in 0.16.0. Outside the story.

Attacked and could not break: flat surface, `tool_invoke` alone and in batches, bare names, verb-level
fields; workflow command steps; delegate children; approval-checkpoint resume; hooks cannot rewrite
calls; bridge mode; `null` optionals, duplicate keys, string-encoded arguments, `__proto__`, empty and
unicode keys; toolchain roles' nested provider maps; open `answer` schemas; refusal wording leaks
nothing; invariants 6 and 13.

```findings
[{"file":"crates/harness-catalogue/src/catalogue.rs","line":437,"category":"acceptance","severity":"warning","verdict":"CONFIRMED","origin":"introduced","message":"Through the loop a catalogue call with an undeclared field is refused only inside the port, after the approver and before-call hook ran; under DenyAll the reply never names the field."},
{"file":"crates/harness-wire/src/undeclared.rs","line":113,"category":"boundary","severity":"warning","verdict":"CONFIRMED","origin":"introduced","message":"The walker judges every array element by items and ignores prefixItems, so a valid 2020-12 answer is refused where 0.16.0 accepted it."},
{"file":"crates/harness-mcp/src/lib.rs","line":188,"category":"judgement","severity":"note","verdict":"CONFIRMED","origin":"pre-existing","message":"MCP tool calls are forwarded untouched; a closed MCP schema is not enforced on harness's side."}]
```
