---
format: aep.planning-md/1
id: story:provider-refusal-details
kind: story
status: active
title: Preserve structured provider refusal diagnostics
relations:
- informed_by: story:provider-terminal-and-retry-semantics
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: STATUS.md
- confidence: cited
  path: contracts/provider-wires/anthropic-messages/2026-09-07
- confidence: cited
  path: crates/harness-messages/src/lib.rs
- confidence: cited
  path: crates/harness-messages/src/refusal.rs
- confidence: cited
  path: crates/harness-messages/tests/contract.rs
- confidence: cited
  path: crates/harness-xtask/src/provider_contract.rs
revision: 5
---
## Outcome

A consumer can distinguish the provider's declared refusal category and explanation from a generic incomplete turn. This serves O3 by making real-provider verification observable.

## Evidence and contract

The Messages decoder currently reads message_delta.delta.stop_reason and drops stop_details. The consuming local deployment observed refused benign diagnostics with partial text, but cannot recover the provider's reason. The provider documents stop_details on message_delta at https://platform.claude.com/docs/en/test-and-evaluate/strengthen-guardrails/handle-streaming-refusals . Existing neutral StreamEvent::Warning is the typed home for this diagnostic; no new domain entity or provider field enters harness-wire.

## Acceptance

- A streamed refusal emits a bounded diagnostic warning containing only the declared category and explanation; absent/null details remain explicitly unknown.
- Refusal stays incomplete; no automatic retry, model switch, credential change, prompt substitution or successful-completion conversion is introduced.
- Contract fixtures pin warning output and preserve request bytes. Existing released contract directories remain immutable; add a new dated contract.
- Synthetic diagnostic tests and the required source gate pass. Live category evidence is collected only by the consuming deployment and is never represented by synthetic fixtures.

## Scope

Messages decoder and tests, a new provider contract directory and its independent verifier, changelog and status documentation. No request transport, authentication, model selection or credential-custody change.

## Verification

The focused Messages crate tests and the complete `cargo xtask gate` passed on stable Rust 1.98.1. The independent provider-contract checker and decoder regression both verify the new synthetic refusal fixture. The stable toolchain was refreshed and unchanged. A consuming live deployment run remains pending; no provider refusal has been explained or resolved by this source verification alone.
