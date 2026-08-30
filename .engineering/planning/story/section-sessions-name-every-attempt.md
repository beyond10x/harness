---
format: aep.planning-md/1
id: story:section-sessions-name-every-attempt
kind: story
status: active
title: A section's session names every attempt above it, so a re-entered ancestor overwrites nothing
summary: session_for names a session <flow-run>.<path>.<attempt> with the section's own attempt; when the root or a retreat re-enters, the section's attempt 1 runs again under the same id and overwrites the first file — walk 7 lost the transcript with the red validator
owner: harness
tags:
- record
- workflow
relations:
- serves: vision:b10x-owns-its-loop
revision: 3
---
# Story: a section's session names every attempt above it

## Outcome

`FlowRunner::session_for` names a section's session `<flow-run>.<path>.<attempt>`, where
`attempt` is the section's own. When an ancestor is re-entered — the root retreating, or a
retreat group going round again — the section runs its attempt 1 a second time under the same
path, and the second file overwrites the first. The seventh paid native walk (metaharness
`native-eval.hUbOP5`, 2026-08-30) filed `…root.receive.1` twice and `…root.specify.1` twice; the
first `specify` attempt of the first root attempt — the one whose validator exited 1 — is gone
from the sessions, and only the event record still says it happened.

After this story a session's name carries the attempt of every open scope on the way down —
`<flow-run>.root.1.specify.2`, or an equivalent that cannot collide — so every attempt that ran
has a transcript, and `sessions` lists as many files as sections that ran.

## Acceptance

- A walk in which the root retreats files one session per `(scope, attempt)` at every level; a
  test asserts the count equals the `group-entered` events that opened a conversation.
- The id stays sortable by flow run and readable: which section, which attempt, under which
  ancestor's attempt.
- `website/docs/guides/workflows.md` § sessions says how a session is named.

## Evidence

The end-to-end walk of `adp-default.projected.yaml` with a scenario that fails once inside the
retreat, and a re-run of metaharness `run-native.sh` whose `sessions` listing shows no repeated
id.
