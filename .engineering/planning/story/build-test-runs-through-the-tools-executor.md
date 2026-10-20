---
format: aep.planning-md/3
id: story:build-test-runs-through-the-tools-executor
kind: story
status: active
title: Toolchain operations run through the tools executor, local and confined
summary: invoke_toolchain is replaced by tool_toolchain::executor over a runner that calls the active Operations; waits on the tools release that makes Executor generic over a runner.
relations:
- decomposes: epic:harness-on-llm-and-tools
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-20T17:54:39Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-10-20T17:54:39Z", actor: "human:timo", revision: 4}
---
## Context

harness runs a toolchain operation's plan through its own `invoke_toolchain`
(`crates/harness-catalogue/src/catalogue.rs:519-558`), over the `Operations` trait's `run_within`, so
one code path serves both the local reader-and-runner and the Substrate-confined provider.

tools `0.3.0` (`e89380d8`) ships `build-test` v1alpha1 and `tool_toolchain::executor`, "harness
0.14.1's toolchain executor moved here": the contract's answer shape (`runs` for a plan that can start
several processes), one deadline shared by a plan's processes, every path argument judged where it
lands, named refusals (`build-test.Refused`, `build-test.CheckFailed`). Its `Executor` is fixed to
`tool_process::Process`, the local unconfined runner, and does not apply a provider's sandbox
environment (`executor.rs:51-60`), so harness cannot run a confined plan through it.

Decided by `b10x-chef-orchestrator` (2026-10-20, option B, atlas ADR 0097 §3): one executor serves
local and Substrate-confined runs. tools makes `Executor` generic over a runner trait; harness passes
its `Operations`. This story waits on that tools release.

## Outcome

Every toolchain operation, local or confined, runs through `tool_toolchain::executor`, with
`Operations::run_within` as its runner; harness's `invoke_toolchain` and its plan code are gone.

## Acceptance

- `invoke_toolchain` is replaced by the tools executor over a runner that calls the active
  `Operations` (`LocalOperations` or `ConfinedOperations`); `crate::toolchain::CommandPlan` and the
  plan half of `src/toolchain.rs` are replaced by `tool_toolchain::executor::plan`.
- Reply changes against 0.15.x are listed in `CHANGELOG.md`: `runs` as a list for every operation
  whose plan can start several processes (`rust_fmt`), the shared deadline, path arguments judged
  where they land, a `-`-prefixed value refused outside a declared `--`, named refusals.
- The confined path keeps its confinement: the substrate conformance suite runs a toolchain operation
  confined and passes.
- `cargo xtask gate` exits 0; argv byte-identical.

## Scope

- `crates/harness-catalogue/src/catalogue.rs` (`invoke_toolchain`), `src/toolchain.rs`,
  `src/operations.rs` (the runner adapter), `crates/harness-substrate` tests,
  `Cargo.toml`/`Cargo.lock` (the tools tag that carries the generic executor), `CHANGELOG.md`.

## Out of scope

- The executor itself (the `tools` repository).
- `run` (`story:run-moves-onto-tool-process`) and `search` (`story:search-moves-onto-tool-search`).

## The tools release (2026-10-20)

tools `0.4.0`: tag object `ea3cac7c`, commit `6017fccc5a0110f0f8f1da5f0a56570825cf95b5` (verified by
`b10x-chef-orchestrator`: tools main, bot commit and tagger, `CHANGELOG.md` `[0.4.0]`). API, from the
tools session:

- `tool_toolchain::{Runner, RunRequest, RunOutput}`; `Executor<R = Process>`;
  `Executor::with_runner(runner, providers)`.
- `trait Runner { type Error: Display; fn root(&self) -> &Path; fn run(&self, RunRequest) ->
  Result<RunOutput, Self::Error> }`; `RunRequest { argv, provider, sandbox, time_left }` is
  `#[non_exhaustive]`, `sandbox` being the provider's declared `SandboxSpec` with placeholders
  unexpanded.
- The build-test checks run in the executor before the runner is asked.
- Breaking: `ResolvedProvider` gained the public field `sandbox`, so the struct literal in
  `crates/harness-substrate/src/tests.rs` (line 1348 at 0.15.1) needs it.
- The local `Process` runner does not apply the sandbox environment (tools
  `story:build-test-applies-provider-environment`, draft); harness's confined runner applies it as
  `ConfinedOperations` does today.

harness implements `Runner` over its active `Operations` (`run_within`), so local and confined runs
share the one executor (option B).
