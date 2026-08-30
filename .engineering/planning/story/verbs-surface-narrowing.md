---
format: aep.planning-md/1
id: story:verbs-surface-narrowing
kind: story
status: draft
title: A named agent narrows entries reached through a verb, not only tools named directly
relations:
- derived_from: epic:adoption-follow-ups
revision: 3
---
## What is missing

A named agent's `tools:` list narrows what its child may use — but only when the call names a tool
directly. Under the `verbs` surface a call names `tool_invoke` and the catalogue entry is an
argument, so the narrowing admits the verb and the port decides the entry. Named agents are a
flat-surface feature until that is answered.

- `crates/harness-loop/src/lib.rs` — the guard, which says exactly this where a reader meets it.
- `crates/harness-loop/src/agent.rs`, `Agent::admitted` — the intersection that does the narrowing.

## Why it was left

The narrowing had already had to move once. Filtering only the *published* toolset left a hidden
tool reachable by name, because the model has the name from its own instructions and does not have
to guess — a boundary that only hides is not one. It now filters publication *and* refuses the call
by the rule that already refuses an unpublished tool.

Extending that to entries inside a verb means deciding *where* the entry is inspected. Doing it in
the loop would put catalogue knowledge in a layer that deliberately has none; doing it in the port
would give the port a second gate that could disagree with the loop's. Neither is obviously right,
and a wrong answer here is a permission boundary that reports as one and is not.

## Acceptance

A delegate run as an agent granted `[Read, Grep]` is refused a `file_write` reached through
`tool_invoke`, by the same rule and with the same message as one that named `file_write` directly —
one rule, not two that can drift.

The eval's native arm runs `flat`, so this is not currently observable there; whatever answers it
should be observable somewhere before it is believed.

## Scope

Derived 2026-08-30 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Primary surface:** `crates/harness-loop` — cited, the story names both its files
- **Files:** `crates/harness-loop/src/lib.rs:3517-3538` — cited, the guard whose own comment states
  this gap ("under the `verbs` surface the call names `tool_invoke` and the entry is an argument")
- **Files:** `crates/harness-loop/src/agent.rs:158` `Agent::admitted` — cited, the intersection
- **Files:** `crates/harness-loop/src/lib.rs:3709` `AgentLoop::port_specs` — cited, the other half of
  the one chokepoint; `LoopConfig::admits` doc at `:344-357` names it as the single enforcement site
- **Files:** `crates/harness-loop/src/tests.rs:5021`
  `a_narrowed_run_is_offered_less_and_refused_the_rest_by_the_same_rule` — cited, the sibling test
  the verb case extends; `ScriptedTools::invoking`/`routing` at `:212-222` already fake a
  verb-shaped `invoked`, so no real catalogue is needed
- **Symbols:** `LoopConfig::admits`, `AgentLoop::invoke`, `AgentLoop::port_specs`, `Agent::admitted`,
  warning code `unpublished-tool` — cited
- **Also likely:** `crates/harness-wire/src/port.rs:174` `ToolPort::invoked` — inferred, the only
  answer that names the entry without giving the loop catalogue knowledge
- **Also likely:** `crates/harness-tools/src/verbs.rs:110` `Verbs::invoked_entry` and `:258`/`:301`
  `invoked` — inferred, the port-side alternative the story says it cannot yet choose between
- **Also likely:** `crates/harness-cli/src/lib.rs:2444` `Published::invoked`,
  `crates/harness-cli/src/agents.rs:52-56` — inferred, forwarding and name-mapping only
- **Documents:** `CHANGELOG.md:353-356` — cited, carries "Named agents are a flat-surface feature
  until that is answered" verbatim and must change when it is
- **Documents:** `crates/harness-loop/src/agent.rs:1-53` module doc and
  `crates/harness-loop/src/lib.rs:344-357` — cited, both state the flat-only scope in prose
- **Documents:** `docs/design/0002-sub-agents-structured-output-hooks.md` § 2 — inferred; `:279`
  states the same "invoked entry's name, not `tool_invoke`" rule for **hooks**, a precedent for the
  fix rather than the fix
- **No live provider run needed:** cited. The loop's tests run on `ScriptedModel`/`ScriptedTools`
  in-crate with no network; `crates/harness-cli/tests/end_to_end.rs:159` already exercises
  `--surface verbs` against a local Python emulator. Implementable and verifiable entirely inside
  this repository.
- **Confidence:** high — the story names the defect site and the tree confirms it at `lib.rs:3523`
- **Would collide with:** any unit touching `harness-loop`'s gate path —
  `crates/harness-loop/src/lib.rs`, `crates/harness-loop/src/agent.rs`,
  `crates/harness-loop/src/tests.rs`; and, if the fix lands port-side instead,
  `crates/harness-tools/src/verbs.rs` plus `crates/harness-wire/src/port.rs`

Open when scoped: which layer the fix lands in — the story states the choice is open, so this scope
is wider than the eventual diff. `--agents-dir` appears in no test file, so a CLI-level acceptance
would be a new end-to-end file rather than an edit. `epic:adoption-follow-ups` is still an unfilled
template and gives no steer.
