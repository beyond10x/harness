# Code review — 2026-08-29

**Status (same day):** every finding below is fixed in the working tree except as noted in
*Outcome*; gate after the fixes: 353 passed / 0 failed / 1 ignored, 24 suites, exit 0. Fixes were
made by four single-crate agents (tools, loop, cli, substrate) plus the orchestrator for shared
files; each defect carries a regression test named in `CHANGELOG.md`'s `Unreleased` section.

| # | outcome |
|---|---|
| H1 | fixed — `ToolPort::call_envelope`, `LoopConfig::unattended_ceiling` (default `Low`), 5 tests |
| H2 | fixed — `symlink_metadata` walk, link targets refused, 5 tests |
| H3 | fixed — lexical normalisation + absolute refusal in `Scope::refusal`, 3 tests |
| M1 | fixed — `bool` flag, end-to-end test through the embedded path |
| M2 | fixed — named refusal, exit 1; 2 end-to-end tests |
| M3 | fixed — `env_clear` + 6-name allow list, test via `/usr/bin/env` |
| M4 | fixed — shared `confined_exec_input`, snapshot required, 3 tests |
| M5 | fixed — non-string `argv` refused |
| M6 | fixed — deadline between calls, 3 tests |
| L1–L9, L11, L13, L14 | fixed |
| L10 | **documented only** (README, *Running with confinement*): nothing seeds `<workspace>/.cargo` |
| L12 | fixed — dated note in each affected contract README |

Open follow-up: a `--approve-up-to <risk>` flag on the command line, so a run can raise the
unattended ceiling without `--yes`'s all-or-nothing.


Scope: every non-test source file in `crates/`, the two contract checkers, dependencies. Method:
read against the invariants in `AGENTS.md`; every boundary claim below was reproduced at runtime
(a throwaway integration test, since deleted, or the built binary) unless marked *inferred*.
Gate at the time of review: 324 passed / 0 failed / 1 ignored, exit 0.

## High — the safety envelope does not hold where it says it does

| # | finding | evidence | fix |
|---|---|---|---|
| H1 | **The approval gate is never consulted.** The loop asks the approver only when `spec.approval == Required`; every shipped spec is `NotRequired`, so `DenyAll` (invariant 12) decides nothing and `--yes` changes nothing. A run with `--substrate-embedded --cgroup-root … --allow-program cargo` executes processes with no gate. `Envelope::needs_approval` exists and has zero callers. | `crates/harness-loop/src/lib.rs:722`; `crates/harness-tools/src/verbs.rs:166,185,206`; `crates/harness-tools/src/catalogue.rs:65`; `crates/harness-app-server/src/session.rs:434`; `b10x-harness tools` → 3× `"approval": "not-required"`; `grep needs_approval` → only `envelope.rs` | Derive approval in the loop from the **entry's** envelope (`tool_invoke` unwraps the entry the way `subjects` already does) against a ceiling in `LoopConfig`; delete `ToolSpec::approval` as `turn.rs:35` announces. Test: a `run` entry under `DenyAll` is refused. |
| H2 | **`file_write` escapes the workspace through a dangling symlink.** `resolve_new` walks up with `exists()`, which follows links, so a dangling link inside the workspace looks absent; the workspace passes containment; `fs::write` then follows the link and creates the file outside. Reproduced: `ws/link -> /outside/escaped.txt`, `file_write("link")` → `Ok`, outside file exists. Reachable through `LocalOperations::unconfined`, which metaharness's MCP server uses. | `crates/harness-tools/src/local.rs:196,230`; `../metaharness/crates/metaharness-cli/src/lib.rs:205` | Walk with `symlink_metadata().is_ok()`; refuse when the final component is a symlink; write with `create_new`/`O_NOFOLLOW` semantics. Regression test. |
| H3 | **A denied write scope is bypassed by spelling the path differently.** The glob is matched against the raw `path` argument. Reproduced under `target/**=denied`: `target/x` refused; `./target/x`, `crates/../target/x`, `/abs/ws/target/x` all allowed. | `crates/harness-tools/src/catalogue.rs:282-283`; `crates/harness-tools/src/scope.rs:144` | Normalise before matching (strip `./`, collapse `..`, refuse absolute) — or match the workspace-relative path the provider resolves. Keep the raw path for `subjects`. Test all four spellings. |

## Medium

| # | finding | evidence | fix |
|---|---|---|---|
| M1 | `--substrate-embedded` **requires a value** that is then ignored; README shows it bare; no CLI test exercises the embedded path. | `crates/harness-cli/src/lib.rs:222-223,374-375`; binary: `error: a value is required for '--substrate-embedded <SUBSTRATE_EMBEDDED>'`; `README.md:76`; `grep substrate-embedded crates/harness-cli/tests` → 0 | `bool` flag on `run` and `tools`; one end-to-end test through it. |
| M2 | **Silent fall-back to read-only** when confinement was asked for: driver fails to open, workspace name is not `ws_*`, or the daemon answers garbage → `read_only()` with no message. Contradicts the doc comment beside it. Operator asks for write+exec, gets a read-only run, the model reports the task done. | `crates/harness-cli/src/lib.rs:708-723,732-738` vs `:661-663` | Refuse the run (`Err`) naming the reason; at minimum a warning event. |
| M3 | Unconfined `run` **inherits the whole environment** — any credential in the MCP server's env reaches `cargo run`. The embedded path clears it. | `crates/harness-tools/src/local.rs:272-279`; contrast `crates/harness-substrate/src/embedded.rs:338` | `env_clear()` + allowlist (`PATH`, `HOME`, `LANG`, `TERM`). |
| M4 | The socket client's exec sends **no confinement request** (`sandbox`, `limits`, `wait`), unlike the embedded path. Parked today; the day the socket path is revived this runs without `required: true`. | `crates/harness-substrate/src/client.rs:302-309` vs `embedded.rs:344-370` | Send the same `ExecStartInput`, or remove `Client::exec` until the path works. |
| M5 | Non-string `argv` items are **silently dropped**: `["cargo", 5, "test"]` runs `cargo test`. | `crates/harness-tools/src/catalogue.rs:314-323` | Refuse an argv with a non-string item. |
| M6 | **No test for `max_duration_ms`** (STATUS admits it), and the deadline is checked only between turns, so one `run` can overshoot it by 600 s (unconfined) / 900 s (confined). | `grep -i deadline crates/harness-loop/src/tests.rs` → 0; `lib.rs:612-617`; `local.rs:50`; `embedded.rs:360` | Test it; check the deadline between calls in `run_calls`; pass remaining time into the exec timeout. |

## Low

| # | finding | evidence | fix |
|---|---|---|---|
| L1 | `file_read` reads the whole file before truncating — `max_bytes` bounds the reply, not memory. | `local.rs:173` | `File::open().take(limit+1)`; check `metadata.len()` first. |
| L2 | `search` cuts matched lines at 400 chars with no flag (invariant 8). | `local.rs:360` | `"line_truncated": true`. |
| L3 | Unconfined `file_read` can split a UTF-8 char at `limit` → U+FFFD; the confined one backs off. | `local.rs:176-181` vs `tools.rs:106-111` | Same backoff. |
| L4 | `dir_list` reports a symlink-to-directory as `"file"` with the link's size. | `local.rs:150-154` | Report `"symlink"`, or follow for kind. |
| L5 | `**/x` never matches `x` at the root: `**/*.md=denied` leaves `README.md` unrestricted. | `scope.rs:148-163` | `**/` = zero or more segments; test. |
| L6 | `x-client-request-id` is the same string on every request of a run. | `crates/harness-responses/src/lib.rs:303` | Per-request id; keep `session-id` stable. |
| L7 | Retry back-off `thread::sleep` ignores cancel (Ctrl-C waits ≤ 8 s). | `responses/src/lib.rs:284` | Sleep in slices, check `cancel`. |
| L8 | The embedded driver is opened **twice** per run on the same root. | `crates/harness-cli/src/lib.rs:712-714` | One driver behind `Arc`. |
| L9 | `workspace_create` id `ws_{ttl}_{pid}` collides for two creates in one process. | `embedded.rs:226` | Include a counter. |
| L10 | `--toolchain rust` needs a pre-seeded `<workspace>/.cargo`; nothing seeds it and nothing documents it → offline builds fail deep in cargo. | `crates/harness-substrate/src/toolchain.rs:117-119`; `grep -i seed README.md` → 0 | Document, or seed from the registry read-only. |
| L11 | `cargo audit`: `chacha20 0.10.1` **yanked** (reqwest→quinn→rand). `serde_yaml 0.9.34+deprecated` dev-dep in `harness-flow`. | `cargo audit`; `cargo tree -i serde_yaml` | `cargo update`; `serde_yaml_ng` (as entity-runtime did). |
| L12 | Released contract versions were edited after tag `0.1.0` (brand sweep); AGENTS.md records it, the version READMEs do not. | `git diff --stat 0.1.0..HEAD -- contracts/` → 9 files | Dated note in each affected `README.md`. |
| L13 | `check-app-server-profile.py` dies with a traceback on a corrupted fixture (exit 1, but no digest message). | tamper run | Catch and report by name. |
| L14 | Doc comments attached to the wrong item: `subjects` → `operations` (`verbs.rs:74-86`); `published` → `Confinement` (`cli/lib.rs:653-670`). `check_edges` builds a `named` set it never reads; duplicate `needs` are not refused (`flow/lib.rs:314-325`). | — | Tidy. |

## Verified clean

Credential custody (`Bearer` no `Display`, redacted `Debug`, per-call fetch, no ambient fallback);
opaque-item cross-wire refusal; absent usage stays absent; every wire bound refuses by name; SSE
per-line and per-stream caps; retry only before the first witnessed byte; compaction monotone and
labelled; rate arithmetic exact; JSON-RPC framing, interrupt watch and per-turn cancel token; both
contract checkers **fail on tamper** (digest → exit 1, fixture bytes → exit 1). reqwest's blocking
`timeout` is per read/connect/write as the comment claims (`reqwest-0.12.28/src/blocking/client.rs:383`).
