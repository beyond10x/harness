---
format: aep.planning-md/3
id: review-result:adversary-u1-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:harness-builds-on-llm-0-7 (wave llm-1, unit commit 5bd3f9b3)
relations:
- reviews: story:harness-builds-on-llm-0-7
revision: 1
---
unit: story:harness-builds-on-llm-0-7 — worktree `harness-on-llm-u1`, HEAD 5bd3f9b3 (unit) over 8825e9c3 (spike) over base 22ddc1df; my cases are uncommitted in that tree
verdict: red (NEEDS-CHANGE)
cases: executed 391→400, red 6
origin: introduced 6 / pre-existing 1 / undecided 1
wrote-outside-worktree: 2 paths — `~/.cache/harness-wave-2026-10-20-w01/u1/scratch/adversary-1/` (13 files) and `~/.cache/claude-tmp/b10x-m1-fixture/` (9 files, written by `measurements/dry-run-compaction.sh` under `TMPDIR`)
needs-coordinator: yes — (a) the origin of row 1 is `pre-existing` as the brief instructed, but the base 22ddc1df retried these classes, so by the procedure's definition the pin exposed it; (b) the one-line fixture fix for row 2 changes an existing fixture default rather than adding a scenario, so I did not apply it

## 1. `git --no-pager diff --stat` (working tree, test files only)

```
 crates/harness-cli/src/llm_port.rs                 |  45 ++++++++   (mod tests only)
 crates/harness-cli/tests/end_to_end.rs             | 122 +++++++++++++++++++++
 crates/harness-cli/tests/fixtures/fake_messages.py |  24 ++++
 .../harness-cli/tests/fixtures/fake_responses.py   |  30 +++++
 4 files changed, 221 insertions(+)
```

## 2. Cases added (each run alone, before the suite; logs in scratch `case-*.log`)

Fixture scenarios added to both emulators: `failed-once`, `rate-limited-once`, `overloaded-once` (one in-stream provider failure before any output, then an answer).

| Case | Asserts | Now | Red output (verbatim, run alone) |
|---|---|---|---|
| `llm_port::tests::an_in_stream_provider_failure_before_any_output_is_offered_for_another_attempt` | `from_llm_error` on llm 0.7.0's `Unavailable`/`RateLimited`/`Transport` (as its decoders build them) is retriable | red | `panicked at crates/harness-cli/src/llm_port.rs:771:13: Responses `server_error`, Messages `overloaded_error`: … WireError { code: Transport, message: "not this time", retriable: false }` — `test result: FAILED. 0 passed; 1 failed` |
| `llm_port::tests::an_api_key_is_never_a_subscription_account_on_either_wire` | key → `ApiKey`/`x-api-key`/`Metered` on Messages, `Bearer`/`Metered` on Responses | green | mutant pin, no finding |
| `end_to_end::a_responses_server_error_before_any_output_is_retried_through_the_shipped_binary` | one `turn-retried`, exit 0, `completed` | red | `panicked at crates/harness-cli/tests/end_to_end.rs:2748:5: Responses `server_error`: the far side's own failure before any output is attempted again: ["started", "turn-started"]` |
| `…a_responses_rate_limit_before_any_output_is_retried…` | same | red | `…Responses `rate_limit_exceeded`: … ["started", "turn-started"]` |
| `…a_messages_overload_before_any_output_is_retried…` | same, `--wire anthropic-messages` | red | `…Messages `overloaded_error`: … ["started", "turn-started"]` |
| `…a_messages_api_error_before_any_output_is_retried…` | same | red | `…Messages `api_error`: … ["started", "turn-started"]` |
| `…a_responses_overload_before_any_output_is_retried…` (`server_is_overloaded`) | same | green | control: the loop's retry works through the port when llm sets the flag |
| `…a_cold_gateway_is_retried_through_the_shipped_binary` (HTTP 503 once) | same | green | control: `llm-http` keeps 408/429/5xx retriable |
| `…a_messages_turn_that_reports_its_input_figures_files_usage_through_the_shipped_binary` | Messages `text` run records a `usage` event | red | `panicked at crates/harness-cli/tests/end_to_end.rs:2849:5: a turn whose provider reported its input figures files them: ["started", "turn-started", "text-delta", "text-delta", "finished"]` |

## 3. Suite run (after the cases existed)

`cargo test -p b10x-harness-cli --locked --no-fail-fast` → exit 101. Log: `scratch/adversary-1/suite-cli.log`.

```
lib:        test result: FAILED. 262 passed; 1 failed; 0 ignored
argv_pin_adversary 5 ok · argv_pin_consumer 3 ok · argv_pin_consumer_adversary 6 ok · bridge_mode 28 ok · context 4 ok
end_to_end: test result: FAILED. 54 passed; 5 failed; 0 ignored
help_text 3 ok · withheld 3 ok · workflow 26 ok
```

Total executed 400 (implementor's 391 + 9 added); red 6.

Runtime observations (not tests): `bash measurements/dry-run-compaction.sh --out <scratch>/m1-dry-compaction-16000.jsonl` → exit 2, `BudgetUnobservable { max_cost_microunits, "a model request omitted usage…" }` after one turn, event kinds: 0 `usage`, 0 `cost` (the committed base runs `measurements/runs/m1-dry-*.jsonl` carry 9 of each). `bash measurements/prove-spend-ceiling.sh` → case C: `model wire refused: Transport: secret reference `oauth-token-file` refused: secret source does not satisfy the required access protections`, 0 cost events.

## 4. Findings

| # | file:line | What is wrong | Shown by | Verdict | Origin | Severity |
|---|---|---|---|---|---|---|
| 1 | `crates/harness-cli/src/llm_port.rs:627` | `may_retry()` is passed through unchanged; llm 0.7.0 marks in-stream `response.failed` `server_error`/`rate_limit_exceeded` and Messages `error` `overloaded_error`/`api_error` final (`llm-responses/src/stream.rs:477`, `llm-messages/src/decode.rs:710-752`), so the loop ends the run on the first attempt where the base's wires retried (base `harness-responses/tests/provider_emulated.rs:341` asserted retriable). Reached by any run whose provider hiccups before output. | unit case + 4 e2e cases, red | CONFIRMED | pre-existing (per brief; see needs-coordinator) | warning |
| 2 | `crates/harness-cli/tests/fixtures/fake_messages.py:72` | `usage_object` omits `cache_creation_input_tokens`; under llm M25 (`llm-messages/src/usage.rs:107-116`) the input total is `None`, so **every** Messages-emulator turn files no usage and no cost. `dry-run-compaction.sh` and `prove-spend-ceiling.sh` (both in this unit's diff) and STATUS.md:18's "scorer over the `compacted`, `usage` and `cost` events" no longer function on this wire. Fix: default `cache_creation=0` as the vendor reports it, and restore the 49/7/59 assertions at end_to_end.rs:660 and :1241. | e2e usage case red; script runs above | NEEDS-CHANGE | introduced | blocker |
| 3 | `crates/harness-cli/tests/end_to_end.rs:1241` | `a_run_that_broke_on_the_wire_files_what_it_bought_on_the_second_wire_too` now asserts the run filed nothing — the inverse of its name and of its Responses sibling at :1182-1193. Rewritten on the spike (8825e9c3), outside the story's three `#[ignore]` tests. | diff `22ddc1df..8825e9c3` | NEEDS-CHANGE | introduced | warning |
| 4 | `measurements/prove-spend-ceiling.sh:45` | The token file is written at the default umask; llm-credentials refuses any token file not owner-only with one link (`llm-credentials/src/file.rs:116`), which the base's `harness-credential` never checked (no mode check in `oauth.rs`). Case C dies before any request. The new requirement is in no document (CHANGELOG/README/STATUS). | script run above | NEEDS-CHANGE | introduced | warning |
| 5 | `crates/harness-cli/src/llm_port.rs:630` | A credential-source refusal (llm `Unavailable`, from `llm-providers/src/auth.rs:113-121`) reaches the operator as `WireErrorCode::Transport`, so "your token file is world-readable" reads as a network failure. | script run above | CONFIRMED | introduced | note |
| 6 | `STATUS.md:27` | The Messages-wire row still claims "exact request/header/event contracts" and "Both wires pass the same loop scenarios against deterministic local sockets" — `contract.rs` and `provider_emulated.rs` were deleted; the sibling Responses row (:17) was rewritten, this one was not. | read | NEEDS-CHANGE | introduced | warning |
| 7 | `AGENTS.md:132` (also `AGENTS.md:48-50`, `README.md:464`) | `BearerSource`/`Bearer`/`StaticBearer` have no consumer outside `harness-wire` (`grep`: only the re-export at `harness-wire/src/lib.rs:19` and a doc comment at `harness-cli/src/lib.rs:1633`); the Safety envelope and invariant 3 still describe them as the credential path, while the live path is `llm_credentials::Secret` via `llm_port::Source`. | read | CONFIRMED | introduced | note |
| 8 | `crates/harness-cli/tests/bridge_mode.rs:1216` | Start-window flake: 28/28 in my suite run, 5/5 in the implementor's single reruns, 1 of 8 suite runs under 17/20. The path it drives (`BaseUrl::new` refusal inside `binding()`) is identical on the spike and 0.7.0, so the pin narrowing the window is a hypothesis; the race is between the pipelined interrupt and `drive_turn`'s terminal frame. No deterministic case constructible without an app-server change. | implementor logs | INFEASIBLE | undecided | note |

## 5. Attacked and not broken

- Translation fidelity: bounds and id rules are byte-identical between `harness-wire` and `llm-core` (`bound.rs`, `id.rs`); opaque payloads are stored verbatim on both sides before and after (`reasoning` item, `thinking` block), so on-disk sessions replay.
- Credentials: API key never becomes a subscription account (green pin); `Source`/`Credential` Debug redacted; no error path formats a token.
- `max` effort: Messages sends `output_config.effort: max`, Responses `reasoning.effort: xhigh` (`llm-core/src/controls.rs:261`, `llm-messages/src/codec.rs:178`); the constant pin catches its mutant.
- Mid-stream retry stays the loop's: HTTP 503 and `server_is_overloaded` retry end to end (two green controls); the bridge never retries on its own.
- Cancellation: polled at 10 ms, ends `Cancelled`; covered by `bridge_mode.rs:1187` against `slow`.
- The three re-enabled tests: `reached_its_work` accepts only exit 1 plus the R45 string; the answer-prose rewrite matches README:315.

## 6. Paths written outside the worktree

- `~/.cache/harness-wave-2026-10-20-w01/u1/scratch/adversary-1/` — `case-unit-instream.log`, `case-unit-apikey.log`, 7 × `case-e2e-*.log`, `suite-cli.log`, `prove-spend-ceiling.log`, `dry-run-compaction.log`, `m1-dry-compaction-16000.jsonl`
- `~/.cache/claude-tmp/b10x-m1-fixture/` — 9 files, the compaction script's generated workspace

## 7. Findings block

```findings
[{"file": "crates/harness-cli/src/llm_port.rs", "line": 627, "category": "contract-drift", "severity": "warning", "verdict": "CONFIRMED", "origin": "pre-existing", "message": "in-stream provider failures before any output (Responses server_error/rate_limit_exceeded, Messages overloaded_error/api_error) arrive from llm 0.7.0 as final, so the loop ends the run on the first attempt where the base's wires retried"},
{"file": "crates/harness-cli/tests/fixtures/fake_messages.py", "line": 72, "category": "acceptance", "severity": "blocker", "verdict": "NEEDS-CHANGE", "origin": "introduced", "message": "usage_object omits cache_creation_input_tokens, so under llm M25 every Messages-emulator turn files no usage or cost and dry-run-compaction.sh / prove-spend-ceiling.sh no longer produce the events STATUS.md:18 describes"},
{"file": "crates/harness-cli/tests/end_to_end.rs", "line": 1241, "category": "acceptance", "severity": "warning", "verdict": "NEEDS-CHANGE", "origin": "introduced", "message": "the second-wire 'files what it bought' test was rewritten on the spike to assert the failed run filed nothing, outside the story's three ignored tests"},
{"file": "measurements/prove-spend-ceiling.sh", "line": 45, "category": "contract-drift", "severity": "warning", "verdict": "NEEDS-CHANGE", "origin": "introduced", "message": "llm-credentials refuses a token file that is not owner-only with one link; the script's token is written at the default umask so case C dies before any request, and the requirement is documented nowhere"},
{"file": "crates/harness-cli/src/llm_port.rs", "line": 630, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "a credential-source refusal (llm Unavailable) is reported to the operator as WireErrorCode::Transport"},
{"file": "STATUS.md", "line": 27, "category": "contract-drift", "severity": "warning", "verdict": "NEEDS-CHANGE", "origin": "introduced", "message": "the Messages-wire row still claims the deleted contract.rs and provider_emulated.rs suites as present while the Responses row was rewritten"},
{"file": "AGENTS.md", "line": 132, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "the Safety envelope and invariant 3 describe BearerSource/Bearer/StaticBearer as the credential path, but nothing outside harness-wire uses them since llm_port::Source over llm_credentials took over"},
{"file": "crates/harness-cli/tests/bridge_mode.rs", "line": 1216, "category": "concurrency", "severity": "note", "verdict": "INFEASIBLE", "origin": "undecided", "message": "the start-window interrupt test misses 17/20 in about one suite run in eight; the path it drives is identical on the spike and 0.7.0 and no deterministic case is constructible without an app-server change"}]
```
