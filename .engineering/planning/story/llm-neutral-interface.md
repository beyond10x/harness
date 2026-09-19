---
format: aep.planning-md/1
id: story:llm-neutral-interface
kind: story
status: draft
title: Harness consumes neutral LLM types and projections
relations:
- decomposes: epic:llm-adoption
scope:
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: Cargo.toml
- confidence: inferred
  path: contracts
- confidence: inferred
  path: crates/harness-loop
- confidence: inferred
  path: crates/harness-messages
- confidence: inferred
  path: crates/harness-responses
- confidence: inferred
  path: crates/harness-wire
revision: 2
---
## Context

Evidence: crates/harness-wire/src/port.rs and turn.rs mix model-facing values with tool execution/approval metadata; crates/harness-responses and harness-messages own current projections. Adapt only the model side to the released LLM contract; ToolPort, envelopes, approvals, loop budgets and sandbox authority stay here. Plan explicit compatibility adapters/re-exports and new contracts where released bytes change. Preserve opaque state across existing saved sessions or give a named migration refusal.

## Acceptance

Existing loop, streaming, cancellation, tool-authority and saved-session fixtures pass against released LLM types and adapters with no consumer dependency cycle.

## Verification

Retain exact release/contract identities, baseline and candidate results, and the repository gate. No paid provider call or deployment occurs in the ordinary gate.

## Scope

- inferred: `crates/harness-wire` — adoption surface.
- inferred: `crates/harness-responses` — adoption surface.
- inferred: `crates/harness-messages` — adoption surface.
- inferred: `crates/harness-loop` — adoption surface.
- inferred: `Cargo.toml` — adoption surface.
- inferred: `Cargo.lock` — adoption surface.
- inferred: `contracts` — adoption surface.

## External prerequisites

- depends_on:llm/story:foundation-qualified

AEP 0.55.0 currently refuses cross-member targets while creating mutation locator evidence (kind contains disallowed character /). These are explicit references, not admitted graph edges. The local llm-foundation-release blocker gates adoption until the exact upstream release evidence exists.
