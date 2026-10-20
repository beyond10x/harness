---
format: aep.planning-md/3
id: review-result:adversary-u1-pass-2
kind: review-result
status: active
title: Adversary pass 2 on story:harness-builds-on-llm-0-7 (wave llm-1, correction commit 36a587c5)
relations:
- reviews: story:harness-builds-on-llm-0-7
revision: 1
---
unit: story:harness-builds-on-llm-0-7, pass 2 — worktree `harness-on-llm-u1`, HEAD 36a587c5 (correction) over 5bd3f9b3 over 8825e9c3 over base 22ddc1df; my pass-2 cases are uncommitted in that tree
verdict: red (NEEDS-CHANGE)
cases: executed 403→406, red 3 new (`a_source_refusal_names_the_reference_and_never_the_variable_or_the_path`, `the_two_emulators_offer_the_same_scenarios`, `a_messages_turn_whose_reported_figures_are_dropped_says_so_without_a_budget`); the five red-by-decision cases excluded
origin: introduced 5 / pre-existing 0 / undecided 0
wrote-outside-worktree: 2 paths — `~/.cache/harness-wave-2026-10-20-w01/u1/scratch/adversary-2/` (6 files) and `~/.cache/claude-tmp/b10x-m1-fixture/` (9 files, rewritten by `measurements/dry-run-compaction.sh`)
needs-coordinator: none (the `<before>` count 403 is my suite run minus my three cases; the correction's own `cases:` line was not in the brief)

## 1. `git --no-pager diff --stat` (working tree, test files only)

```
 crates/harness-cli/src/llm_port.rs     | 16 ++++++++   (mod tests only)
 crates/harness-cli/tests/end_to_end.rs | 70 ++++++++++++++++++++++++++++++++++
 2 files changed, 86 insertions(+)
```

## 2. Cases added (each run alone, before the suite; logs in `scratch/adversary-2/case-*.log`)

| Case | Asserts | Now | Red output (verbatim, run alone) |
|---|---|---|---|
| `llm_port::tests::a_source_refusal_names_the_reference_and_never_the_variable_or_the_path` | `Source::environment("NOT=VALID")`'s refusal does not name the variable (AGENTS.md:141) | red | `panicked at crates/harness-cli/src/llm_port.rs:795:9: the variable is not named, per AGENTS.md: the environment variable `NOT=VALID`: invalid secret reference` |
| `end_to_end::the_two_emulators_offer_the_same_scenarios` | both `--list-scenarios` answers are equal (README.md:462) | red | `panicked at crates/harness-cli/tests/end_to_end.rs:2885:5` — left lacks, right has `"absent-cache-write"`; the other 33 names agree |
| `end_to_end::a_messages_turn_whose_reported_figures_are_dropped_says_so_without_a_budget` | `absent-cache-write` with no card and no ceiling: no `usage` (as documented) **and** a `warning` naming usage | red | `panicked at crates/harness-cli/tests/end_to_end.rs:2914:5: the figures the route did report and this side dropped are warned about: ["started", "turn-started", "text-delta", "text-delta", "finished"]` |

## 3. Suite run (after the cases existed)

`cargo test -p b10x-harness-cli --locked --no-fail-fast` → exit 101. Log: `scratch/adversary-2/suite-cli.log`.

```
lib:        test result: FAILED. 263 passed; 2 failed   (pass-1 in-stream case by decision + pass-2 source-refusal)
argv_pin_adversary 5 ok · argv_pin_consumer 3 ok · argv_pin_consumer_adversary 6 ok · bridge_mode 28 ok · context 4 ok
end_to_end: test result: FAILED. 57 passed; 6 failed   (4 retry-class by decision + 2 pass-2 cases)
help_text 3 ok · withheld 3 ok · workflow 26 ok
```

Runtime observation: `bash measurements/dry-run-compaction.sh --out <scratch>/m1-dry-compaction-16000.jsonl` on the corrected tree → exit 0; 9 `usage`, 9 `cost`, 1 `compacted`, 8 `tool-requested` in one conversation. Pass-1 finding 2 and the compaction id fix hold end to end.

## 4. Findings

| # | file:line | What is wrong | Shown by | Verdict | Origin | Severity |
|---|---|---|---|---|---|---|
| 1 | `crates/harness-cli/tests/fixtures/fake_responses.py:25` (README.md:462) | The correction added `absent-cache-write` to `fake_messages.py`'s `SCENARIOS` only; README.md:462 says the two emulators' "scenario names are the same on both sides", and the test that held that went with the deleted suites (`fake_messages.py:12` still cites it). Reached by anyone reading the README or running `--list-scenarios`. | e2e case red | NEEDS-CHANGE | introduced | warning |
| 2 | `crates/harness-cli/src/llm_port.rs:610` | `usage()` drops a partially reported usage whole (`output_tokens: 8`, `cache_read: 7` known, input total unknown under M25) because `harness_wire::Usage` has no optional input, and nothing says so: with no budget set the record reads as a provider that reported nothing. AGENTS.md invariant 7's "preserved and warned about, never dropped". Fix: in `LlmPort::turn`, emit `StreamEvent::Warning` when `observation.usage.is_some()` and `usage()` is `None`. Reached by any Messages-compatible gateway that omits `cache_creation_input_tokens`. | e2e case red | NEEDS-CHANGE | introduced | warning |
| 3 | `AGENTS.md:141` | "a refusal names the reference (`oauth-token-file`), never the path, the variable or the value" — the per-request refusal llm builds keeps that; the construction-time refusals do not: `llm_port.rs:108,113` name the path, `:127` the variable, `lib.rs:1591` the API-key path. The sentence should promise "never the value". | unit case red | CONFIRMED | introduced | note |
| 4 | `crates/harness-cli/src/llm_port.rs:634` | The `Unavailable && Dispatch::NotSent ⇒ Unauthorized` rule infers the cause from `Dispatch`'s default. Checked every llm 0.7.0 site that builds `Unavailable`: `llm-providers/src/auth.rs:120` (credential, NotSent), `llm-messages/src/decode.rs:716` and `llm-responses/src/stream.rs:462,466` (Accepted), `llm-blocking/src/lib.rs:71` (never reaches `from_llm_error`). Sound today; a future `Error::new(Unavailable, …)` in a transport before dispatch would read as a credential problem. | read | CONFIRMED | introduced | note |
| 5 | `README.md:101` (CHANGELOG.md:40-43 same) | The mode rule omits that every parent directory must be owned by the caller or root (`llm-credentials/src/file.rs:109-111`; a group- or world-writable parent is admitted only when root-owned and sticky). A token under another user's directory is refused and the document does not say so. | read | CONFIRMED | introduced | note |

Pass-1 rows, on this tree: 2, 3, 4, 5, 6, 7 corrected (2 and the id fix verified by the compaction rehearsal above; 5 by the correction's 0644 case). 1 red by decision. 8 (bridge start-window) unchanged, 28/28 again.

## 5. Attacked and not broken

- Restored usage assertions equal the base byte for byte in the numbers (49/7 thinking, 49/7/59 second wire; `workflow.rs` differs from 22ddc1df only in fixture paths).
- `absent-cache-write` case asserts the stop on `--json` (`stop.kind`, `stop.name`) and on stderr, not only the exit code.
- Compaction id fix: the only scenarios issuing calls on several turns of one conversation are `compaction` (numbered now) and `flow-fails-second` (one call per section, each its own session); the two literal ids in tests (`end_to_end.rs:2475,2484`) are `delegate-pair`'s, untouched.
- Credential refusal path: the 0644 token case shows `Unauthorized`, the reference, no `Transport`, no value; `Secret` is `Zeroizing` with a redacted `Debug` and no `Display`.
- STATUS.md:27 now matches the Responses row's form.

## 6. Paths written outside the worktree

- `~/.cache/harness-wave-2026-10-20-w01/u1/scratch/adversary-2/` — `case-unit-source-refusal.log`, 2 × `case-e2e-*.log`, `suite-cli.log`, `dry-run-compaction.log`, `m1-dry-compaction-16000.jsonl`
- `~/.cache/claude-tmp/b10x-m1-fixture/` — 9 files, the compaction script's generated workspace (same path as pass 1, rewritten)

## 7. Findings block

```findings
[{"file": "crates/harness-cli/tests/fixtures/fake_responses.py", "line": 25, "category": "contract-drift", "severity": "warning", "verdict": "NEEDS-CHANGE", "origin": "introduced", "message": "absent-cache-write is served by the Messages emulator only, while README.md:462 says the two emulators' scenario names are the same on both sides and nothing tests it since the suites were deleted"},
{"file": "crates/harness-cli/src/llm_port.rs", "line": 610, "category": "acceptance", "severity": "warning", "verdict": "NEEDS-CHANGE", "origin": "introduced", "message": "a partially reported usage (output and cache-read known, input total unknown) is dropped whole with no warning, so a run with no budget set cannot tell a provider that reported nothing from one whose figures this side dropped"},
{"file": "AGENTS.md", "line": 141, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "the sentence promises a refusal never names the path or the variable, but Source::file and Source::environment (llm_port.rs:108,113,127) and credential_from (lib.rs:1591) name them at construction; only the value is never named"},
{"file": "crates/harness-cli/src/llm_port.rs", "line": 634, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "the Unavailable-and-NotSent-is-Unauthorized rule keys on Dispatch's default; every llm 0.7.0 site that builds Unavailable without dispatch is secret_error today, so it holds, but a future pre-dispatch Unavailable in a transport would read as a credential problem"},
{"file": "README.md", "line": 101, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "the token-file rule omits that every parent directory must be owned by the caller or root (llm-credentials file.rs:109-111), so a token under another user's directory is refused without the document saying so"}]
```
