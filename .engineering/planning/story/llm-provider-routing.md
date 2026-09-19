---
format: aep.planning-md/1
id: story:llm-provider-routing
kind: story
status: draft
title: Harness configuration resolves through the shared LLM catalog
relations:
- decomposes: epic:llm-adoption
- depends_on: story:llm-neutral-interface
scope:
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: Cargo.toml
- confidence: inferred
  path: contracts
- confidence: inferred
  path: crates/harness-cli
- confidence: inferred
  path: crates/harness-credential
- confidence: inferred
  path: website/docs/reference/configuration.md
revision: 2
---
## Context

Evidence: crates/harness-cli/src/provider.rs:246 rejects custom provider names before overrides. Replace the private catalog with the released routing/binding seam and preserve documented flag precedence, explicit credential sources, aliases and session provenance. Provide a reviewed migration from harness.toml provider tables; do not silently change provider, billing or write permissions. Reuse credential-default and renewal evidence without presenting historical defaults as new shared-library policy.

## Acceptance

A custom vLLM provider and existing built-in configurations resolve through shared LLM routing while precedence, credential selection and session provenance regression tests pass.

## Verification

Retain exact release/contract identities, baseline and candidate results, and the repository gate. No paid provider call or deployment occurs in the ordinary gate.

## Scope

- inferred: `crates/harness-cli` — adoption surface.
- inferred: `crates/harness-credential` — adoption surface.
- inferred: `Cargo.toml` — adoption surface.
- inferred: `Cargo.lock` — adoption surface.
- inferred: `website/docs/reference/configuration.md` — adoption surface.
- inferred: `contracts` — adoption surface.

## External prerequisites

- depends_on:llm/story:foundation-qualified

AEP 0.55.0 currently refuses cross-member targets while creating mutation locator evidence (kind contains disallowed character /). These are explicit references, not admitted graph edges. The local llm-foundation-release blocker gates adoption until the exact upstream release evidence exists.
