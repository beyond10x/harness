---
format: aep.planning-md/3
id: dependency-blocker:tools-executor-is-not-generic
kind: dependency-blocker
status: cleared
title: tools' toolchain Executor runs only through the local process runner
relations:
- blocks: story:build-test-runs-through-the-tools-executor
revision: 2
transitions:
- {from: "open", to: "cleared", at: "2026-10-20T15:47:23Z", actor: "human:timo", revision: 2}
---
## What is missing

A `tools` release whose `tool_toolchain::executor::Executor` is generic over a runner trait (run one
argv within the time left, answer its output), so harness can pass its `Operations::run_within` and
one executor serves local and Substrate-confined toolchain runs. tools `0.3.0` (`e89380d8`) fixes the
runner to `tool_process::Process` (`crates/tool-toolchain/src/executor.rs:337-345`).

## Who clears it

The `tools` repository, asked by `b10x-chef-orchestrator` (2026-10-20). Cleared when that release is
tagged and its sha verified; `story:build-test-runs-through-the-tools-executor` then pins it.
