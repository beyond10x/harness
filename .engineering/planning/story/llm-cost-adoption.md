---
format: aep.planning-md/1
id: story:llm-cost-adoption
kind: story
status: draft
title: Harness prices usage through LLM while retaining loop ceilings
relations:
- decomposes: epic:llm-adoption
- depends_on: story:llm-provider-routing
scope:
- confidence: inferred
  path: Cargo.lock
- confidence: inferred
  path: Cargo.toml
- confidence: inferred
  path: crates/harness-cli
- confidence: inferred
  path: crates/harness-loop
revision: 2
---
## Context

Evidence: crates/harness-loop/src/price.rs and budget.rs; story:budgets-bind-when-usage-is-unknown. Adopt shared pricing/accounting and attempt evidence without counting a nested request twice. Loop-level turn/token limits remain here; cache accounting, absent usage, equality and unknown prices keep their tested behavior. Reconcile pending OAuth/vLLM backlog with exact LLM owning stories; retain historical evidence and do not bulk-archive.

## Acceptance

Budget regression fixtures demonstrate unchanged or explicitly versioned stop behavior and correct nested/fallback accounting using released LLM costs.

## Verification

Retain exact release/contract identities, baseline and candidate results, and the repository gate. No paid provider call or deployment occurs in the ordinary gate.

## Scope

- inferred: `crates/harness-loop` — adoption surface.
- inferred: `crates/harness-cli` — adoption surface.
- inferred: `Cargo.toml` — adoption surface.
- inferred: `Cargo.lock` — adoption surface.

## External prerequisites

- depends_on:llm/story:foundation-qualified

AEP 0.55.0 currently refuses cross-member targets while creating mutation locator evidence (kind contains disallowed character /). These are explicit references, not admitted graph edges. The local llm-foundation-release blocker gates adoption until the exact upstream release evidence exists.
