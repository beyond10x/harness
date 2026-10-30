# Retrofit coverage inventory

This inventory separates known shapes from executed semantics. The ESS report's
`complete_inventory` describes the declarations selected by `spec/ess-inputs.yaml`; it does not
mean the repository's entire implementation has an ESS domain.

| Boundary | Declared and executed | Production source |
| --- | --- | --- |
| Budget admission | All six zero bounds, valid empty budget, cost ceiling requires a priced run | `crates/harness-loop/src/budget.rs:11,67` |
| Native loop outcomes | Completed, provider incomplete/output limit, input/output/turn ceilings, cancellation before dispatch, missing usage with and without token ceilings | `crates/harness-loop/src/lib.rs:123,2103,2813,2836` |
| Approval and tool reach | Cheap read dispatches; medium-risk write is denied by DenyAll and admitted by ApproveAll; unpublished tool refuses; failed tool results remain visible and the model continues | `crates/harness-loop/src/approval.rs:49`; `crates/harness-loop/src/lib.rs:2074,3162`; `crates/harness-wire/src/envelope.rs:218` |
| Loop control states | Entity `harness.loop.Loop`: seven states, 17 transitions, each command edge its own transition. Covered: the turn, the tool round trip, approval (approved, denied, deferred), resumption from the serialized checkpoint (approved, denied), the compaction summary turn (folded, kept), retry, retries exhausted, a refusal that is not retriable, and cancellation. Every command is also refused for an unknown run and in each state that does not await it | `crates/harness-loop/src/lib.rs:857,2706,2738,2796,2818,3104,3122,3197,3224,3363,3462,4553,4598` |
| Run ledger | `harness.loop.RunLedger` (`usage`, `cost_micro_usd`, `turns`), compared on every terminal answer of a fresh run; a resumed run writes none (`ledger: null`) | `crates/harness-loop/src/lib.rs:352-363,1027,2341,2367` |
| Approval checkpoint | `harness.approval-checkpoint/1` as `harness.loop.ApprovalCheckpoint`: every field typed, the format a one-variant enum; the deferred answer carries the production value, and the resume scenarios load that exact JSON back through `resume_approval` | `crates/harness-loop/src/lib.rs:282-290,2403-2415,2452-2455,4616` |

### The Loop entity

`harness.loop.Loop` is the run's control state as observed at its ports, not a new public type. A
command answers the one port call the loop is blocked on: `AnswerTurn`, `CallTool`, `FailTurn` and
`CancelTurn` answer `ModelPort::turn`; `AnswerSummary` and `FailSummary` answer the summary turn
(the model request with no tools, `lib.rs:3224`); `Approve`, `Deny` and `Defer` answer
`Approver::decide`; `CompleteTool` lets `ToolPort::invoke` return. The adapter
(`crates/harness-conformance/src/boundary.rs`) runs one real `AgentLoop` per scenario on a thread
whose ports block on a channel, and names each outcome from the port the loop asks next and from
the `LoopEvent`s it emitted. It computes no state of its own. The view `harness.loop.Loops`
publishes `state`, `turns` and `retries` as the adapter observed them.

`turns` counts as `RunState.turns` does (`lib.rs:2738`), summary turns included (`lib.rs:3231`).
`retries` is the attempt number of the last `TurnRetried` (`lib.rs:2824`), back to 0 when a turn is
answered. `unfolded` says the conversation passed the compaction trigger (80% of a 4096-token
window, `lib.rs:1105,3122`) with at least `SUMMARY_MIN_FOLD_BYTES` to fold (`lib.rs:1119,3207`):
the adapter's long tool text is 16 KiB, so a `long` call is followed by a summary turn before the
next conversation turn.

Two temporary production mutations were each caught by the suite and reverted: `MAX_TURN_RETRIES`
3 → 2 failed 5 scenarios (`FailTurn` exhausted and retried, transitions `fail` and `retry`, authored
`loop-retries-exhausted`); not counting a summary turn (`lib.rs:3231`) failed 9.

**Serde decision.** `RunLedger` derives `Serialize` and `Deserialize` with `deny_unknown_fields`
(`lib.rs:350-351`), as `ApprovalCheckpoint`, `RunState` and `Usage` already do. The adapter
serializes the production value instead of copying its fields by hand, so a field added to the
ledger fails the declared schema rather than vanishing from the evidence. The derive changes no
behaviour: nothing in the loop serializes the ledger.

**ESS 0.77.0 workarounds.**

- ESS 0.77.0 accepts `summary:` on fields but refuses it on a transition, a state and an enum type.
  Their source citations are YAML comments in `spec/domains/loop.yaml`.
- The view is `read_your_writes`, which ESS 0.77.0 reads only behind a consistency token, so the
  adapter returns a write counter as the `ConsistencyToken` on every Loop answer, refusals
  included. The view is read from the same observations that decided the answer.
- ESS 0.77.0 refuses an authored response literal that gives some of a struct's fields
  (`ESS-AUTHOR-001`, `response literal checkpoint: invalid_input`). Pinning the whole checkpoint
  would pin `LoopConfig`, declared `Json`, so the scenarios pin the deferred `stop` and the ledger,
  and the declared schema (one-variant `format`, every field typed) holds the checkpoint.
- ESS 0.77.0 admits a command into a multi-`from` transition from any of its states, so each
  command edge is its own transition.

The stop `harness.loop.Stop` is a tagged union on `kind`: one variant per `LoopStop` variant
(`crates/harness-loop/src/lib.rs:127-197`), each carrying exactly that variant's fields, required,
with their serde spelling. ESS therefore refuses a stop no `LoopStop` can be, such as a `completed`
stop carrying `limit_ms` (`crates/harness-conformance/tests/stop_union.rs`). The variants
`max-cost`, `deadline` and `unstructured` are declared but not executed: declaring their shape is
not executing those paths. The Loop scenarios execute `awaiting-approval` (the deferred run's
returned stop) and `failed`, which `run_in` never returns: the Loop adapter reads it from the
`LoopEvent::Finished` a failed run emits (`lib.rs:123-124`).

ESS 0.77.0 writes a union value adjacently tagged: the tag, and a payload variant's fields under
`value` (`{"kind": "max-turns", "value": {"limit": 1}}`); a unit variant such as `completed` is the
tag alone. `LoopStop` serializes internally tagged, its fields beside `kind`, and ESS has no form
for that. The scenarios therefore write the adjacent form, and the adapter
(`crates/harness-conformance/src/boundary.rs`, `tagged`) moves the production serialization's
fields under `value` without computing any. The unit variant needs specification format `ess/22`;
the Loop refusals' `{subject: state}` error field needs `ess/23`, which `spec/system.yaml` declares.

The following are **UNMAPPED** into this first ESS slice. They remain implemented and covered by
existing repository tests, which are not automatically ESS conformance evidence. Closing each item
requires source-derived domain declarations and scenarios that execute its named production
boundary; no owner decision or invented ownership/cardinality is needed merely to admit these gaps.

- **UNMAPPED: remaining loop control and accounting.** Cost accumulation, elapsed deadlines,
  structured output, hooks and delegation. Sources: `crates/harness-loop/src/lib.rs:223,2650`;
  `price.rs:90`, `answer.rs`, `delegate.rs`, `hook.rs`. Add controlled clock/provider fixtures
  before claiming these stop variants or controls execute.
- **UNMAPPED: the rest of compaction and retry.** Elision without a summary turn (a fold under
  `SUMMARY_MIN_FOLD_BYTES`, `lib.rs:1387,3207`), the trigger measured from a provider's reported
  input tokens, the retry backoff's duration, and cancellation inside the backoff
  (`lib.rs:2817-2818,2836`). The Loop scenarios use one fixed window and backoff.
- **UNMAPPED: approval checkpoint refusals and contents.** `resume_approval` refuses a malformed
  checkpoint and one taken under another configuration (`lib.rs:2403-2420`); a deferral with an
  empty checkpoint id fails the call (`lib.rs:4627`); a delegate cannot defer (`lib.rs:4616-4625`).
  None is a Loop scenario. The checkpoint's `config` (`LoopConfig`), `invoked` (`ToolSpec`) and
  `state.items` (`Item`) are declared `Json`, their structure unmodelled.
- **UNMAPPED: `unfolded` in the Loops view.** The view does not publish whether a summary turn is
  due, so ESS 0.77.0 notes that refusals guarded by it cannot be checked through the view.
- **UNMAPPED: the `--json` LoopEvent record.** The events are observed by the adapter, not declared
  as ESS events.
- **UNMAPPED: dynamic context and inventory narrowing.** Sources:
  `crates/harness-loop/src/context.rs:124`, `environment.rs`, and
  `crates/harness-cli/src/environment.rs`. Add real refresh/narrowing scenarios and boundary views.
- **UNMAPPED: provider protocols and transport.** Sources: `crates/harness-llm/src/lib.rs`
  over `llm`'s `llm-responses`, `llm-messages` and `llm-http` (tag `0.17.0`).
  Scripted native turns do not prove HTTP, streaming, provider item replay or cancellation of
  network I/O. Extend from the existing contract fixtures.
- **UNMAPPED: credential lifecycle.** Sources: `crates/harness-llm/src/lib.rs` (`Source`,
  `renew_codex_login`) over `llm-credentials`. Add synthetic secret-source and renewal
  scenarios; never use live secrets.
- **UNMAPPED: session persistence and hosted app-server.** Sources:
  `crates/harness-cli/src/transcript.rs:47,393` and `crates/harness-app-server/src/lib.rs:78`.
  Add real temporary transcript and JSON-RPC round trips; loop return values alone do not prove them.
- **UNMAPPED: confinement and effectful tools.** Sources:
  `crates/harness-substrate/src/embedded.rs:80`, `crates/harness-catalogue/src/lib.rs` and
  `crates/harness-substrate/tests/conformance.rs`. The counting ToolPort performs no confined OS
  effect. Retain the existing substrate conformance gate while modeling this boundary separately.
- **UNMAPPED: MCP discovery, transport and local admission.** Sources:
  `crates/harness-mcp/src/lib.rs:34,48,84`. Add real MCP fixtures exercising digest-pinned profile
  intersection; an unpublished flat tool refusal does not prove MCP policy admission.
- **UNMAPPED: CLI profiles and contracts.** Sources:
  `crates/harness-cli/src/profile.rs`, `contract.rs`; `contracts/cli/`.
  Native loop scenarios prove the library boundary, not CLI argument parsing, profile merge or
  command effects.

No conflicting source semantics were resolved by guessing. One pre-existing prose disagreement is
visible: `AGENTS.md` invariant 10 described cost ceilings as always unenforceable, while
`Budget::validate(priced)` and the shipped rate-card implementation admit priced ceilings. This
retrofit follows the implementation and records both priced/unpriced cases; updating the prose is a
documentation correction included in this change.
