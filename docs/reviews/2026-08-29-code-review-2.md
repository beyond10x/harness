# Code review — 2026-08-29, second pass

Review of the tree at `c1da42d` (the first review's fixes), `/code-review high`, 13 verifier
passes: 9 confirmed, 2 plausible, 2 refuted. Every finding below is fixed in the working tree;
gate after the fixes: 362 passed / 0 failed / 1 ignored, exit 0 (`bash scripts/gate.sh`). Each
fix carries a regression test unless the row says otherwise.

| # | finding | evidence | fix |
|---|---|---|---|
| 1 | **A write scope was judged on the caller's spelling; the provider wrote where the path landed.** A link inside the workspace (`ok/link -> target/x`) or a leave-and-re-enter path (`../<ws>/target/y`) matched no rule and landed in a `target/**=denied` tree. | `catalogue.rs` scope check took the lexical path; `local.rs` `resolve_new` canonicalises; `scope.rs` `normalise` keeps a leading `..` | `Operations::lands` (default: the path as written; `LocalOperations` resolves; `Split` asks the effects provider). The catalogue puts the landing through the scope too and refuses naming both spellings. Substrate needs no override: its guarded filesystem resolves with `RESOLVE_NO_SYMLINKS`. 3 tests. |
| 2 | **Every socket body lacked the top-level `op`** the pinned daemon's decoder reads first; each was refused `request.schema-invalid` before the input was looked at, so the confinement fix of the first review never reached a daemon. | `substrate-daemon/src/app/operations.rs` `decode_mutation`: `object.get("op")` → `schema_invalid`; `object.len() != 2` → the same | `mutation(op, input)` builds `{op, input}` for `exec.start`, `workspace.create`, `workspace.file.write`. 2 tests pin the shape. Recorded in `STATUS.md` as the **hypothesis** for the parked socket path; not yet run against a daemon. |
| 3 | **A rule spelled `./target/**` matched nothing**, silently: only the value was normalised. `**/` matching zero directories also widened existing rules with no note. | `scope.rs` `find` used `rule.paths` verbatim; `glob_matches` has no `./` handling | The rule's glob is normalised at match time; an absolute or climbing rule is refused by `ScopeRule::parse`. CHANGELOG names the `**/` widening. 2 tests. |
| 4 | **The gate decided on the entry but reported the verb.** `ApprovalRequired`, `ApprovalPort::decide` and the refusal all carried `tool_invoke`; an approver never saw `file_write`, and the model read "`tool_invoke` was not approved". | `lib.rs` `invoke`: `call_envelope` for the decision, `spec`/`call.name` for everything else | `ToolPort::call_envelope` → `ToolPort::invoked(&call) -> Option<ToolSpec>`: the entry's whole spec. Event, approver and refusal name the entry; the refusal names the verb too; `DenyAll` says a retry cannot help; the standing instruction says not to. AGENTS.md updated. 1 test. |
| 5 | **`GET /v1/machine` before every exec** for a snapshot the daemon holds for its lifetime and that publication had already read — three round trips per run, and two documents. | `client.rs` `exec`; `substrate-host/src/lib.rs` `machine()` returns a field set once in the constructor | `Client` holds the snapshot in a `OnceLock` from the first `machine()`; the CLI probes and serves with one client. 1 test. |
| 6 | `ConfinedOperations::run` indexed `argv[0]`; a library caller with an empty argv panicked mid-turn. | `tools.rs`; `Operations::run` is public and `Split` forwards it | `argv.first()` with a named refusal, as `LocalOperations` answers. No test: the catalogue path is already pinned and the method is one line. |
| 7 | `check-app-server-profile.py` still died with a traceback on a trace line whose `frame` is valid JSON but not an object; `check-provider-wires.py` the same for a stream event. | `frame.get` outside the `try`; `payload.get` unguarded | `isinstance` checks, named failures. Verified by hand on `{"direction":"client","frame":"oops"}` and `data: [1,2]`: both report by name, exit without traceback. |
| 8 | Unconfined `run` dropped `CARGO_HOME`, `RUSTUP_HOME`, `RUSTUP_TOOLCHAIN`, `CARGO_TARGET_DIR`, `SSL_CERT_FILE`; `cargo` under a relocated rustup found no toolchain. `std::env::var` also dropped any non-UTF-8 value. No override hook. | `local.rs` `INHERITED_ENV`, `exec` | Six names added (all paths, none a secret); `LocalOperations::inheriting` names more; `var_os`. Proxy variables stay out by default — a proxy URL can carry a credential. 1 test. |
| 9 | CHANGELOG said a slow call "can no longer overshoot" the deadline; an in-flight call still runs to its own timeout (600 s / 900 s). | `local.rs` `MAX_RUN_SECONDS`; `lib.rs` `timeout_ms: 900_000`; `Operations::run` takes no budget | Sentence corrected; the in-code comment says the same. Passing the remaining budget into a call is open work. |
| 10 | The deadline tests used a 40 ms budget and 60 ms calls; one scheduling stall on a shared CI runner skips the call they expect to see. | `tests.rs` `DEADLINE_MS`, `SLOW_CALL`; 0/80 local failures under 15× oversubscription | 200 ms / 300 ms; the three tests take 0.6 s together. |

Also done while there: the deadline check between turns and between calls reads one
`deadline_passed`. Refuted by the verifiers and left alone: the `ws_` rule appearing in three
places (unreachable through any harness path), and CI's embedded end-to-end on `ubuntu-latest`
(the probe needs only `openat2`).

Not done, cleanup only: the duplicated token steps in `gate.yml` (GitHub Actions has no YAML
anchors; a composite action is the fix), the `Adopted` struct's ceremony, and `dir_list`'s
undocumented third `kind` value.

Follow-ups, done the same day:

| follow-up | outcome |
|---|---|
| remaining wall-clock budget into `Operations::run` | `ToolPort::call_within` → `Catalogue::invoke_within` → `Operations::run_within`; the unconfined kill timer and the confined `timeout_ms` are the smaller of their ceiling and what is left; result says `timeout_ms`. 4 tests. |
| `--approve-up-to <risk>` | on `run`; `conflicts_with = "yes"`; `file_edit` still asks whatever the ceiling (non-idempotent). 1 test. |
| `tests/live.rs` against a daemon | **run, and the hypothesis was half right.** `op` was missing — and it is a caller-minted operation id, not the operation's name. Three more client defects behind it: no read query, exec output never fetched (`exec_id` vs `id`), 10 s read timeout on a call that waits for the exit. All fixed; the live test passes against a daemon built from `f1cfc1c` in a delegated scope, including a 12 s confined exec. |

Still open: the embedded driver's exec is unexercised on this machine (needs the same delegated
scope); `bridge_mode::an_unknown_request_mid_turn_is_refused_without_killing_the_server` failed
once under a loaded gate (`completed` where `interrupted` was expected) and passed 5/5 alone —
a race between the `slow` fixture and the interrupt, not yet pinned.
