---
format: aep.planning-md/3
id: story:end-to-end-suite-leaves-no-fixture-binary
kind: story
status: draft
title: The end_to_end suite leaves no fixture binary in TMPDIR
summary: controlled_fixture_binary() keeps a tempdir per suite run; build into CARGO_TARGET_TMPDIR or hold the TempDir so nothing is left in the shared TMPDIR.
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: inferred
  path: crates/harness-cli/tests/common/mod.rs
revision: 2
---
## Context

`controlled_fixture_binary()` (`crates/harness-cli/tests/end_to_end.rs:104-125`) compiles the MCP
endpoint fixture into `tempfile::tempdir()…keep()`, so every run of the `end_to_end` suite leaves one
directory holding `b10x-harness-mcp-endpoint` in `$TMPDIR` and never removes it. On this machine
`$TMPDIR` is shared by every concurrent session; on 2026-10-20 the cwd adversary's three suite runs
left three such directories (removed by hand after `ls` showed only the binary in each).

## Outcome

Running the `end_to_end` suite leaves nothing behind in `$TMPDIR`.

## Acceptance

After `cargo test -p b10x-harness-cli --locked --test end_to_end`, no directory the suite created
remains under `$TMPDIR` (checked by a before/after listing of the suite's own prefix), and the
controlled-endpoint cases still pass. The fixture binary is built once per test process, as now.

## Scope

- `crates/harness-cli/tests/end_to_end.rs` (`controlled_fixture_binary`): build into the crate's
  target directory (`env!("CARGO_TARGET_TMPDIR")`) or keep the `TempDir` alive for the process.

## Out of scope

- Any change to the fixture itself or the cases that use it.
