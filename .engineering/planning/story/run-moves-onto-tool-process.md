---
format: aep.planning-md/3
id: story:run-moves-onto-tool-process
kind: story
status: draft
title: Harness's run moves onto the tools repository's tool-process toolset
summary: 'After 0.15.0 and tools'' tool-process release: run is served by b10x-tool-process, which bounds a descendant holding stdout and kills a setsid descendant; fixes a pre-existing unbounded output-thread join.'
relations:
- decomposes: epic:harness-on-llm-and-tools
- serves: vision:b10x-owns-its-loop
revision: 1
---
## Context

harness's `run` tool stayed local through tools-1 (`story:harness-consumes-tools-crates`): tools
0.1.0 has no process toolset, so the exec/grep half of the old `harness-tools/src/local.rs` moved
into `crates/harness-catalogue/src/local.rs` unchanged rather than onto a tools crate.

That code carries a pre-existing bug, present in 0.14.1 and carried forward by the move (reported by
`b10x-chef-orchestrator`, 2026-10-20; the lines were `harness-tools/src/local.rs:604,622-623` before
the move, now in `crates/harness-catalogue/src/local.rs`): after the program exits, `exec` joins the
output threads with **no deadline**, so a run whose descendant still holds stdout runs past its
bound and answers `timed_out: false`. Measured: `sh -c "x &"` with a backgrounded descendant holding
stdout took 3.0 s against a 300 ms bound and reported not timed out; a `setsid` descendant escapes
the process-group kill the same way.

tools fixes this in a `tool-process` toolset (tools 0.2.0). When `tool-process` is released, harness's
`run` should move onto it, the same way `fs`/`code-editor` replaced the reader and writer in tools-1.

## Outcome

harness's `run` is served by the `tool-process` toolset from the `tools` repository; a run whose
descendant holds stdout is bounded and answers `timed_out: true` when it exceeds its deadline; a
`setsid` descendant does not escape the kill.

## Acceptance

`cargo xtask gate` exits 0 on a tree where `run` is served by `b10x-tool-process` (or harness's
adapter over it), and a test shows `sh -c "sleep 3 &"` (a descendant holding stdout) against a
300 ms bound answers `timed_out: true` within a small multiple of the bound, and a `setsid`
descendant is killed with the group.

## Sequencing

After 0.15.0 (tools-1 + `story:llm-port-is-a-public-crate`), and after `tools` releases a
`tool-process` toolset (tools 0.2.0 or later). Blocked until then.

## Out of scope

- The fix in `tool-process` itself (the `tools` repository).
- Any `run` behaviour change before `tool-process` ships; the bound-join bug is pre-existing and is
  not fixed by hand in harness-catalogue (it moves onto the fixed toolset instead).
