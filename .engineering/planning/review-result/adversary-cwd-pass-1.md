---
format: aep.planning-md/3
id: review-result:adversary-cwd-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:started-record-carries-the-loops-cwd (unit over 6f46427d)
relations:
- reviews: story:started-record-carries-the-loops-cwd
revision: 1
---
started-record cwd U1 adversary pass 1: 1 red case, 2 warnings, 4 notes, no blocker. Uncommitted unit
over base 6f46427d (tree harness-cwd-u1, branch impl/started-record-carries-the-loops-cwd).

Added `crates/harness-cli/tests/started_cwd_adversary.rs` (6 cases through the shipped binary) and two
checkpoint cases appended to `crates/harness-loop/src/tests.rs`: 7 green, 1 red.

| case | asserts | result |
|---|---|---|
| `a_workspace_whose_name_is_not_utf8_is_not_recorded_under_a_name_that_does_not_exist` | `cwd` is the exact path or absent, never a lossy rendering | red |
| `a_symlinked_workspace_is_recorded_as_its_target_and_as_the_session_files_it` | `--workspace link/./` → the target, equal to the session file's `workspace` | green |
| `every_chat_line_names_the_directory_it_works_in` | `chat --json`, 2 lines → 2 `started` with the canonical cwd | green |
| `every_workflow_step_names_the_directory_it_works_in` | `workflow run --json` → every `started` carries it | green |
| `a_delegate_and_a_forked_pair_name_their_parents_directory` | nested `started` carry the parent's cwd | green |
| `the_converter_states_the_directory_the_record_the_binary_wrote_names` | `run --json` → `events --in` → `session.started.cwd` | green |
| `a_checkpoint_whose_run_named_no_directory_has_the_bytes_an_older_loop_wrote_and_resumes` | no `cwd` key written; resumes | green |
| `a_checkpoint_carries_its_directory_across_a_process_and_resumes_only_in_it` | round-trips; refused in another cwd | green |

Red: `started.cwd` is "…/ws-latin1-�t�": a directory that does not exist (`started_cwd_adversary.rs:179`,
exit 101).

Findings:

1. `crates/harness-cli/src/lib.rs:2205` (warning) — `display().to_string()` records a non-UTF-8
   workspace as a lossy U+FFFD string naming a directory that does not exist; the converter passes
   it on.
2. `crates/harness-cli/src/lib.rs:3315` (warning) — under `--substrate <socket>` writes and execs land
   in the daemon's `--workspace-id` workspace (`harness-substrate/src/tools.rs:347`) while `cwd` names
   the `--workspace` read view.
3. `crates/harness-cli/src/lib.rs:2205` (note) — confined `run` children execute with cwd `/workspace`
   (bind mount, substrate-host `process.rs:2182-2185`): same directory, different mount path.
4. `crates/harness-loop/src/lib.rs:2219` (note) — `cwd` joins checkpoint config equality, so a pre-field
   checkpoint resumed by an embedder that now sets a cwd is refused, contrary to the CHANGELOG; no
   caller of `resume_approval` exists.
5. `crates/harness-cli/src/lib.rs:2205` (note) — the record re-resolves the workspace after
   `LocalOperations` canonicalised its root; a symlink retargeted between them makes the two differ.
6. `README.md:111` (note) — the `--json` row does not name `cwd`.

Attacked and could not break: relative, `nested/..` and `link/./` spellings; flag vs launch directory;
`chat`, `workflow run`, delegates and forked pairs; old `started` JSON and old checkpoint bytes; the
converter on present/absent/null; argv (`cargo xtask cli-contract`: 12 pinned versions); no secret,
home path or environment value beyond the directory.

Residue: `bridge_mode.rs:963` (`an_interrupt_is_answered_before_the_terminal_frame_when_the_model_client_will_not_build`)
failed 1 of 3 workspace runs ("16 of 20 attempts … under the 17"), unrelated to the unit (bridge mode
sets no cwd and drops `Started`); not checked against the base.

```findings
[{"file": "crates/harness-cli/src/lib.rs", "line": 2205, "category": "boundary", "severity": "warning", "verdict": "INFEASIBLE", "origin": "introduced", "message": "A non-UTF-8 workspace is recorded as a lossy U+FFFD string naming a directory that does not exist, and the converter passes it on as session.started.cwd; started_cwd_adversary.rs:179 is red."},
{"file": "crates/harness-cli/src/lib.rs", "line": 3315, "category": "judgement", "severity": "warning", "verdict": "INFEASIBLE", "origin": "introduced", "message": "Under --substrate <socket> writes and execs land in the daemon's --workspace-id workspace while started.cwd names the read-only --workspace view."},
{"file": "crates/harness-cli/src/lib.rs", "line": 2205, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "Confined run children execute with cwd /workspace (bind mount), so a pwd inside the run never equals started.cwd although it is the same directory."},
{"file": "crates/harness-loop/src/lib.rs", "line": 2219, "category": "contract-drift", "severity": "note", "verdict": "INFEASIBLE", "origin": "introduced", "message": "cwd joins checkpoint config equality, so a pre-field checkpoint resumed by an embedder that now names a cwd is refused, contrary to the CHANGELOG's 'still resumes'; no caller of resume_approval exists."},
{"file": "crates/harness-cli/src/lib.rs", "line": 2205, "category": "judgement", "severity": "note", "verdict": "INFEASIBLE", "origin": "introduced", "message": "The record re-resolves the workspace after LocalOperations already canonicalised its root, so a symlink retargeted in between makes the record name a different directory than the tools act in."},
{"file": "README.md", "line": 111, "category": "contract-drift", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "The --json row lists what started carries and does not name the new cwd field."}]
```
