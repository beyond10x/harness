---
format: aep.planning-md/3
id: story:environment-context-stable-before-per-run
kind: story
status: draft
title: Stable environment context comes before per-run context, so the prompt-cache prefix survives across runs
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: inferred
  path: crates/harness-cli/src/lib.rs
revision: 2
---
## Context

Filed under the orchestrator's D290 (2026-10-30), store-only, from a metaharness measurement (c5):
the second of two identical sessions read 0.701 of its input from the provider's prompt cache, and
the cached prefix broke at the per-run memory path. The instruction is assembled in
`crates/harness-cli/src/lib.rs`, `push_environment_context` (line 1885 at `f50af47`), which puts
per-run context (such as that path) before context that is stable across runs, so everything after
the first per-run line misses the cache. That ordering is inferred from the measurement and not yet
read line by line.

## Outcome

`push_environment_context` emits the context that is stable across runs first and the per-run
context after it, so the prompt-cache prefix two runs share covers all of the stable part.

## Acceptance

A second run identical to a first (same flags, same workspace, a different per-run memory path)
reads the whole shared prefix from cache: its reported cached input tokens cover every byte of the
instruction before the first per-run line, measured against the same provider as the c5
measurement, and a test pins the order (stable lines before per-run lines).

## Scope

- `crates/harness-cli/src/lib.rs` (`push_environment_context`), inferred.

## Out of scope

The content of the context lines; any provider-side cache control.
