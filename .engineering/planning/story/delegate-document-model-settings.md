---
format: aep.planning-md/1
id: story:delegate-document-model-settings
kind: story
status: implemented
title: Honor model and effort declared by named delegates
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: crates/harness-cli/src/agents.rs
- confidence: cited
  path: crates/harness-cli/src/lib.rs
- confidence: cited
  path: crates/harness-loop/src/agent.rs
- confidence: cited
  path: crates/harness-loop/src/lib.rs
- confidence: cited
  path: crates/harness-loop/src/tests.rs
- confidence: cited
  path: docs/design/0002-sub-agents-structured-output-hooks.md
revision: 5
---
## Context

Atlas's relocated real consumer compatibility suite reads Agentplugins' aep-plan agents. Their model and effort declarations currently fail at crates/harness-cli/src/agents.rs:173, while crates/harness-wire/src/turn.rs already defines model and sampling.reasoning_effort and both provider adapters already project them. Agent has its existing typed home in crates/harness-loop/src/agent.rs; the missing behavior is document-to-delegate configuration, not a new provider or credential route. The operator authorized resolving this failure as part of the dependency-cycle cleanup.

## Acceptance

A named delegate loaded from an agent document sends the document's resolved model and effort on its own requests while preserving the parent's settings, inherited tool restrictions, shared budget and credential/endpoint selection, and unspecified settings inherit the parent.

## Scope

- cited: crates/harness-cli/src/agents.rs
- cited: crates/harness-cli/src/lib.rs
- cited: crates/harness-loop/src/agent.rs
- cited: crates/harness-loop/src/lib.rs
- cited: crates/harness-loop/src/tests.rs
- cited: CHANGELOG.md
- cited: docs/design/0002-sub-agents-structured-output-hooks.md

## Validation and boundaries

Exercise parsed settings, provider alias expansion, child requests and parent restoration through deterministic tests; keep unknown fields and unenforceable declarations refused. Verify cost ceilings remain enforceable for the selected child model. Run the full Harness gate and the real Atlas consumer suite using the exact candidate. No provider, authentication, billing or endpoint integration changes belong to this fix. Frozen released contracts remain unchanged. One bounded implementation story; there is no multi-story decomposition to dispatch to critics. Atlas story:relocate-consumer-compatibility owns the cross-repository relocation and publication order.
