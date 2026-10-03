---
format: aep.planning-md/3
id: executable-system-specification:harness-runtime
kind: executable-system-specification
status: draft
title: Executable specification of shipped Harness behavior
relations:
- specifies: vision:b10x-owns-its-loop
- informed_by: specification:published-interfaces
revision: 1
---
## Retrofit scope

The baseline tree has no ESS system.yaml or ess-inputs.yaml. Derive the specification from existing library and CLI code and immutable contracts; do not add product semantics. Cover the loop's budgets, tool approval/refusal, terminal outcomes and workflow boundary, with explicit coverage limits for provider IO, external confinement and live integrations. Cite sources on declarations and mark unsupported facts UNMAPPED rather than guessing.

## Acceptance and evidence

A pinned ESS input manifest validates and compiles; named authored/synthesized scenarios drive real Harness code through Rust conformance tests; a deliberate behavior mutation makes a named case fail; task check and cargo xtask gate enforce specification validation and conformance. Retain generated suite/coverage inventory and a truthful report. Do not claim complete coverage of every adapter merely from a green core suite.

## Ownership

The session integration branch owns this retrofit. Parent controls planning-store writes and final gate integration; the public docs story owns website and its standalone builder. The AEP artifact remains draft until actual validation evidence is recorded.
