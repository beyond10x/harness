---
format: aep.planning-md/3
id: story:end-to-end-suite-leaves-no-fixture-binary
kind: story
status: implemented
title: The end_to_end suite leaves no fixture binary in TMPDIR
summary: controlled_fixture_binary() keeps a tempdir per suite run; build into CARGO_TARGET_TMPDIR or hold the TempDir so nothing is left in the shared TMPDIR.
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: crates/harness-cli/tests/end_to_end.rs
revision: 6
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T20:47:59Z", actor: "human:timo", revision: 4}
- {from: "proposed", to: "active", at: "2026-10-29T20:48:00Z", actor: "human:timo", revision: 5}
- {from: "active", to: "implemented", at: "2026-10-29T21:23:15Z", actor: "human:timo", revision: 6, decided_on: {"recorded":{"test_result":1,"verification":1}}}
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

Run with `TMPDIR` set to a fresh empty directory, `cargo test -p b10x-harness-cli --locked --test
end_to_end` exits 0 and leaves that directory empty (`find "$TMPDIR" -mindepth 1` prints nothing),
and the controlled-endpoint cases still pass. The fixture binary is built once per test process, as
now.

## Scope

- `crates/harness-cli/tests/end_to_end.rs` (`controlled_fixture_binary`): build into the crate's
  target directory (`env!("CARGO_TARGET_TMPDIR")`, as `adversary_u2_profile_write.rs:27` does). A
  `TempDir` held in the `static OnceLock` would never be dropped, so it is not an option.

## Out of scope

- Any change to the fixture itself or the cases that use it.

Revised 2026-10-29 from the acceptance critic's round (wave d221): the check uses a fresh `TMPDIR`,
because `tempfile`'s default `.tmp` prefix is shared with every other process.
