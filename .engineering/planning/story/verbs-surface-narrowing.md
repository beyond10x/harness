---
format: aep.planning-md/1
id: story:verbs-surface-narrowing
kind: story
status: draft
title: A named agent narrows entries reached through a verb, not only tools named directly
relations:
- derived_from: epic:adoption-follow-ups
revision: 2
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
