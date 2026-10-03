---
format: aep.planning-md/3
id: executable-system-specification:harness-runtime
kind: executable-system-specification
status: conforming
title: Executable specification of shipped Harness behavior
relations:
- specifies: vision:b10x-owns-its-loop
- informed_by: specification:published-interfaces
model_digest: 4061ac1f6ba3f6b443dc0acb5873b593c3805870927041901582c377e0d9ab96
revision: 5
transitions:
- {from: "draft", to: "validated", at: "2026-10-03T20:11:35Z", actor: "human:timo", revision: 4}
- {from: "validated", to: "conforming", at: "2026-10-03T20:11:35Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"ess_conformance_coverage_v1":1}}}
---
## Retrofit scope

The baseline tree has no ESS system.yaml or ess-inputs.yaml. Derive the specification from existing library and CLI code and immutable contracts; do not add product semantics. Cover the loop's budgets, tool approval/refusal, terminal outcomes and workflow boundary, with explicit coverage limits for provider IO, external confinement and live integrations. Cite sources on declarations and mark unsupported facts UNMAPPED rather than guessing.

## Acceptance and evidence

A pinned ESS input manifest validates and compiles; named authored/synthesized scenarios drive real Harness code through Rust conformance tests; a deliberate behavior mutation makes a named case fail; task check and cargo xtask gate enforce specification validation and conformance. Retain generated suite/coverage inventory and a truthful report. Do not claim complete coverage of every adapter merely from a green core suite.

## Ownership

The session integration branch owns this retrofit. Parent controls planning-store writes and final gate integration; the public docs story owns website and its standalone builder. The AEP artifact remains draft until actual validation evidence is recorded.

## Validated retrofit — 2026-10-03

`spec/ess-inputs.yaml` pins ESS 0.52.0. Strict CLI validation reports `harness v1 — 4 file(s), 38 scenario(s), valid`. The exact release commit 4d6a4ecafc0feb4e11e4bee777b19c7351fa3647 also pins the native compiler/runner dependencies.

`conformance/model.json` and `conformance/suite.json` are checked against fresh native compilation/synthesis. There are 42 selected scenarios (38 authored, 4 generated), zero synthesis refusals. The real AgentLoop and Flow boundaries pass all 42 with no skips, failures, unsupported cases or errors in three consecutive runs. An inert target fails all 42. Temporary mutations to terminal-stop mapping, default approval and failed workflow status caused named scenario failures and were restored byte-for-byte; conformance/mutation-evidence.json records them. No production behavior is changed by the retrofit.

The first slice covers budget admission, loop terminal results and usage, tool admission/refusal and workflow scheduling/admission. It does not cover all Harness behavior. spec/COVERAGE.md records eight UNMAPPED boundaries: remaining loop control/accounting; dynamic context/narrowing; provider transport/protocols; credentials; session persistence/app-server; confinement/effectful tools; MCP; CLI profiles/workflow binding/contracts. Source-derived declarations and real boundary scenarios, not owner guesses, are needed to close them. Existing tests for these boundaries continue to run.

`cargo xtask specification` runs both the real target and inert-target calibration without requiring an ambient ESS CLI. `task check` delegates to the full source gate. The existing AGENTS cost-ceiling prose was corrected to match Budget::validate(priced); priced ceilings were already shipped.
