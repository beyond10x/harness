---
format: aep.planning-md/3
id: review-result:adversary-llm-port-pass-1
kind: review-result
status: active
title: Adversary pass 1 on story:llm-port-is-a-public-crate (llm-port unit over 8dc9e4e3)
relations:
- reviews: story:llm-port-is-a-public-crate
revision: 1
---
llm-port U1 adversary pass 1: 1 red case, 2 notes, no blocker, no warning. Uncommitted unit over base
8dc9e4e3 (tree harness-llm-port-u1, branch impl/llm-port-is-a-public-crate).

Added `crates/harness-llm/tests/adversary_public_surface.rs`, 8 cases through `harness_llm` and
`harness_wire` only, the way a consumer links it: 7 green, 1 red.

| case | asserts | result |
|---|---|---|
| `renew_codex_login_from_an_async_consumer_returns_its_answer_rather_than_panicking` | inside a runtime it answers as outside one | red |
| `a_resolver_is_asked_once_per_request_on_the_responses_wire_as_a_bearer` | 3 turns → 3 resolves, `Bearer lease-1..3` | green |
| `a_request_retried_after_a_failure_asks_again_and_once` | a 500 then a full turn → 2 resolves | green |
| `a_forked_port_asks_the_shared_resolver_once_per_its_own_request` | fork + parent → 2 resolves | green |
| `a_turn_cancelled_before_it_starts_asks_the_resolver_nothing` | 0 resolves, `Cancelled` | green |
| `a_resolver_that_never_answers_ends_on_the_callers_cancel` | `Cancelled` within 10 s; 1 resolve | green |
| `a_sentinel_secret_reaches_no_error_no_event_and_no_debug` | secret absent from error Debug/Display, sink events, `Source` Debug | green |
| `the_port_runs_from_an_async_consumer_over_spawn_blocking` | build in a runtime, turn under `spawn_blocking` | green |

Red output (`cargo test -p b10x-harness-llm --locked --test adversary_public_surface
renew_codex_login_from_an_async_consumer`, exit 101):
`panicked at crates/harness-llm/src/lib.rs:390:10: Cannot start a runtime from within a runtime`;
`left: Err("panicked")`, `right: Ok(Err("no Codex access token in /nonexistent/adversary/codex-login.json; ..."))`.

Findings:

1. `crates/harness-llm/src/lib.rs:390` — `renew_codex_login`, now public, panics when called from a
   thread driving a tokio runtime; its docs promise only an `Err`. No caller reaches it that way
   today (`harness-cli/src/lib.rs:1539` calls it from synchronous code; the three consumers do not
   call renewal). The body is unchanged from the module; making it public exposed it.
2. `STATUS.md:13` — the Source row's claim that only `b10x-harness-llm` depends on the `b10x-llm-*`
   crates is true per `Cargo.lock` but cites `cargo test -p b10x-harness-llm --locked`, which does
   not measure it; `cargo tree -i b10x-llm-core --workspace` does.

Attacked and could not break: surface width (rustdoc `all.html` lists exactly the story's set);
the unchanged move (every `diff --no-index` hunk is visibility, docs, re-exports, `resolver`, the
`+ Send` bound and assertion, or tests); once-per-request resolution on both wires, on retry, fork
and cancel (llm 0.8.0 `prepare_auth` resolves once per request; its HTTP client never retries);
`Send` (a rustc probe adding an `Rc<()>` field fails E0277, the control compiles); invariant 2 (the
seven `llm-*` crates pinned by tag `0.8.0`; only `b10x-harness-llm` depends on them in `Cargo.lock`).
Two harness-cli call sites changed (`lib.rs:257-261` `llm_port::Protocol`, `lib.rs:1690-1692` an
explicit coercion), both forced by the `+ Send` return and the dependency move.

Not testable here: an environment variable changed between two requests (`set_var` is unsafe in
edition 2024 and `unsafe_code` is forbidden workspace-wide, test targets included).

```findings
[{"file": "crates/harness-llm/src/lib.rs", "line": 390, "category": "contract-drift", "severity": "note", "verdict": "INFEASIBLE", "origin": "introduced", "message": "renew_codex_login, now public, panics with 'Cannot start a runtime from within a runtime' when called from a thread driving a tokio runtime, although its docs promise only an Err; no caller reaches it that way today."},
{"file": "STATUS.md", "line": 13, "category": "judgement", "severity": "note", "verdict": "CONFIRMED", "origin": "introduced", "message": "the Source row says only b10x-harness-llm depends on the b10x-llm-* crates and cites cargo test -p b10x-harness-llm --locked, which does not measure that; cargo tree -i b10x-llm-core --workspace would."}]
```
