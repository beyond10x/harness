---
format: aep.planning-md/3
id: review-result:adversary-u1-pass-3
kind: review-result
status: active
title: Adversary pass 3 on story:harness-builds-on-llm-0-7 (the llm 0.8.0 move, commit 97ff3d7f)
relations:
- reviews: story:harness-builds-on-llm-0-7
revision: 1
---
unit: story:harness-builds-on-llm-0-7, pass 3 (the llm 0.8.0 move) — worktree `harness-on-llm-u1`, HEAD 97ff3d7f over 4dbfa35b; my pass-3 cases are uncommitted in that tree
verdict: NEEDS-CHANGE (documents only; no red case)
cases: executed 406→413, red 0
origin: introduced 2 / pre-existing 0 / undecided 0 (plus 1 note on llm's own known item)
wrote-outside-worktree: 1 path — `~/.cache/harness-wave-2026-10-20-w01/u1/scratch/adversary-3/` (8 files)
needs-coordinator: none

## 1. `git --no-pager diff --stat` (working tree, test files only)

```
 crates/harness-cli/tests/end_to_end.rs             | 219 +++++++++++++++++++++
 crates/harness-cli/tests/fixtures/fake_messages.py |  40 ++++
 .../harness-cli/tests/fixtures/fake_responses.py   |  68 +++++++
 3 files changed, 327 insertions(+)
```

## 2. Cases added (each run alone, before the suite; logs in `scratch/adversary-3/case-*.log`)

Fixture scenarios added to both emulators, same names in the same positions: `failed-mid-output` (one text delta, then the provider's own failure, then an answer on the next request), `failed-after-keepalive` (lifecycle payloads only — `response.created`+`response.in_progress`, `message_start`+`ping` — then the failure, then an answer), `shown-text-differs` (Responses: deltas say `provider emulation passed`, the terminal message says `SERVER TEXT`).

| Case | Asserts | Now |
|---|---|---|
| `a_responses_server_error_after_a_text_delta_is_final_through_the_shipped_binary` | exit 1, no `turn-retried`, exactly one `text-delta`, no `finished` | green |
| `a_messages_api_error_after_a_text_delta_is_final_through_the_shipped_binary` | same, `--wire anthropic-messages` | green |
| `a_responses_server_error_after_lifecycle_payloads_only_is_retried_through_the_shipped_binary` | one `turn-retried`, exit 0, `completed` | green |
| `a_messages_overload_after_message_start_and_ping_only_is_retried_through_the_shipped_binary` | same | green |
| `a_responses_turn_keeps_the_text_it_showed_and_the_record_says_the_server_differed` | deltas concatenate to the shown text; a `warning` with code `shown-text-kept` is in the record; the session's `assistant-text` is the shown text, not `SERVER TEXT` | green |
| `a_denied_call_reaches_the_responses_wire_as_a_failed_result_carrying_the_refusal` | turn 2's `function_call_output` envelope is `{"ok": false, "output": "…needs a person's decision…"}` (read from the emulator's `--record`) | green |
| `a_denied_call_reaches_the_messages_wire_as_a_failed_result_carrying_the_refusal` | turn 2's `tool_result` has `is_error: true` and the loop's refusal as content | green |

No case went red: every behaviour the pin changed holds through the bridge. Each ran `test result: ok. 1 passed; 0 failed`.

## 3. Suite run (after the cases existed)

`cargo test -p b10x-harness-cli --locked --no-fail-fast` → exit 0. Log: `scratch/adversary-3/suite-cli.log`.

```
lib 265 ok · argv_pin_adversary 5 ok · argv_pin_consumer 3 ok · argv_pin_consumer_adversary 6 ok · bridge_mode 28 ok
context 4 ok · end_to_end 70 ok · help_text 3 ok · withheld 3 ok · workflow 26 ok
```

413 executed (406 before my seven), 0 red. The five retry-class cases are green on 0.8.0.

## 4. Findings

| # | file:line | What is wrong | Shown by | Verdict | Origin | Severity |
|---|---|---|---|---|---|---|
| 1 | `STATUS.md:17` (also `STATUS.md:27`, `ROADMAP.md:104`, `spec/COVERAGE.md:35`) | The move updated `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` only; the documents that state what is built still say the wire is `b10x-llm-responses`/`-messages` "(llm tag `0.7.0`)", "pinned by tag `0.7.0`" and "(tag `0.7.0`)" as present facts, while `Cargo.toml:77-83` pins `0.8.0`. (`AGENTS.md:46`, `README.md:439,494` say "since llm 0.7.0", which stays true.) | `grep -n '0\.7\.0'` on the documents | NEEDS-CHANGE | introduced | warning |
| 2 | `Cargo.lock:3967` | The brief and the implementor's report say only the seven llm packages and their transitives moved; the diff also moves `tempfile 3.27.0`'s dependency edge from `getrandom 0.4.3` to `getrandom 0.3.4` — a dev dependency of this workspace, not an llm transitive (the `getrandom` packages themselves were already both present). Harmless, but not "nothing else". | `git diff 4dbfa35b...HEAD -- Cargo.lock` | CONFIRMED | introduced | note |
| 3 | `crates/harness-cli/src/llm_port.rs:627` | llm 0.8.0's own `Known` entry: the four newly retriable classes read back retriable when relayed by an llm-gateway with nothing streamed first, so the loop's three attempts stack on the gateway's (`story:relayed-failures-keep-their-retry-decision`, llm's next wave). Reached only by a run whose base URL is an llm-gateway; not fixable here. | llm `CHANGELOG.md` `[0.8.0]` Known | INFEASIBLE | introduced | note |

## 5. Attacked and not broken

- Retry boundary: before output retried, after one delta final, on both wires; `message_start`+`ping` and `response.created`+`response.in_progress` count as "before" (`llm-messages/src/decode.rs:217`, `llm-responses/src/stream.rs:21-24`); no partial text duplicated; the port passes `may_retry()` through unchanged.
- Shown text: `from_llm_event` carries `shown-text-kept`/`shown-text-unpaired` as `StreamEvent::Warning`; the session files the shown text. No harness test asserts a server text llm no longer produces (every emulator scenario's deltas equal its terminal text; the fifth commit touched no end-to-end case).
- Capabilities: `..Capabilities::text(…)` yields `output_streaming: true`, `input_during_output: false`; `bind()` (now calling `validate_protocol`) passes on both protocols — every end-to-end case builds through `llm_port::client`.
- Rejected call encoding: unchanged between 0.7.0 and 0.8.0 (`tool_result_envelope` exists at 0.7.0 `request.rs:641`); pinned on both wires above.
- Messages `content_block_start` opening text now shown as a delta: the emulator opens text and thinking blocks empty, so no count moved.

## 6. Paths written outside the worktree

- `~/.cache/harness-wave-2026-10-20-w01/u1/scratch/adversary-3/` — 7 × `case-e2e-*.log`, `suite-cli.log`

## 7. Findings block

```findings
[{"file": "STATUS.md", "line": 17, "category": "contract-drift", "severity": "warning", "verdict": "NEEDS-CHANGE", "origin": "introduced", "message": "STATUS.md:17,27, ROADMAP.md:104 and spec/COVERAGE.md:35 still state the wire is pinned at llm tag 0.7.0 as a present fact while Cargo.toml pins 0.8.0; the move updated CHANGELOG.md only"},
{"file": "Cargo.lock", "line": 3967, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "the lockfile also moves tempfile 3.27.0's getrandom edge from 0.4.3 to 0.3.4, a dev dependency rather than an llm transitive, contrary to the report's 'nothing else'"},
{"file": "crates/harness-cli/src/llm_port.rs", "line": 627, "category": "judgement", "severity": "note", "verdict": "INFEASIBLE", "origin": "introduced", "message": "per llm 0.8.0's Known entry the four newly retriable classes read back retriable when relayed by an llm-gateway, so the loop's three attempts stack on the gateway's; llm's story owns it"}]
```
