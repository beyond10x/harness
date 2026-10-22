---
format: aep.planning-md/3
id: story:workflow-verb-is-removed
kind: story
status: implemented
title: The workflow verb is removed
summary: workflow plan and workflow run are removed with the crates only they use; multi-step governed work is Loom (Atlas ADR 0096). Breaks metaharness-aep-eval native, approved.
relations:
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-10-22T02:18:25Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-22T02:18:25Z", actor: "human:timo", revision: 3}
- {from: "active", to: "implemented", at: "2026-10-22T07:41:28Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":2}}}
---
## Context

Timo, 2026-10-22, relayed by `b10x-chef-orchestrator`: "remove harness workflow". Atlas ADR 0096
puts multi-step governed work in Loom (L3). The verb is `workflow plan|run`
(`crates/harness-cli/src/workflow.rs`, `crates/harness-workflow`, `crates/harness-flow`).

## Consumers

metaharness `origin/main` `a34551e` runs `b10x-harness workflow plan` and `workflow run` from
`crates/metaharness-aep-eval/src/lib.rs:260`, `:297`, `:907` and `:996` (`aep-eval native`). No
Cargo dependency on `b10x-harness-flow` or `b10x-harness-workflow` exists outside harness
(search of `~/beyond10x`, 2026-10-22). Prose names the verb in aep
`website/docs/guides/integrate-a-harness.md:391-397`, atlas `architecture/adr/0004-…md:16,54`
and atlas `README.md:314,328`.

## Outcome

`b10x-harness workflow` is gone, with the crates only it uses.

## Acceptance

- clap refuses `workflow` as an unknown subcommand.
- A new `contracts/cli/b10x-harness/<date>/` version without `workflow`; released versions
  byte-identical.
- `crates/harness-workflow` and `crates/harness-flow` are removed unless another crate in the
  workspace still needs them.
- design 0003's runner and README § Workflows are gone; the website does not mention the verb.
- `CHANGELOG.md`: one line under `Removed`.
- `cargo xtask gate` exits 0.

## Sequencing

0.18.0, with `harness events` removed in the same release. Timo approved breaking metaharness
`aep-eval native` against 0.18.0 ("Yeah, no, I want you to ship this.", relayed by
`b10x-chef-orchestrator`, 2026-10-22 ~05:0x); metaharness drops the call in parallel. The
CHANGELOG's Removed line names `metaharness-aep-eval` as the affected consumer.
