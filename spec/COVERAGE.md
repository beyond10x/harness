# Retrofit coverage inventory

This inventory separates known shapes from executed semantics. The ESS report's
`complete_inventory` describes the declarations selected by `spec/ess-inputs.yaml`; it does not
mean the repository's entire implementation has an ESS domain.

| Boundary | Declared and executed | Production source |
| --- | --- | --- |
| Budget admission | All six zero bounds, valid empty budget, cost ceiling requires a priced run | `crates/harness-loop/src/budget.rs:11,67` |
| Native loop outcomes | Completed, provider incomplete/output limit, input/output/turn ceilings, cancellation before dispatch, missing usage with and without token ceilings | `crates/harness-loop/src/lib.rs:123,2102,2812,2835` |
| Approval and tool reach | Cheap read dispatches; medium-risk write is denied by DenyAll and admitted by ApproveAll; unpublished tool refuses; failed tool results remain visible and the model continues | `crates/harness-loop/src/approval.rs:49`; `crates/harness-loop/src/lib.rs:2073,3161`; `crates/harness-wire/src/envelope.rs:218` |

The stop `harness.loop.Stop` is a tagged union on `kind`: one variant per `LoopStop` variant
(`crates/harness-loop/src/lib.rs:127-197`), each carrying exactly that variant's fields, required,
with their serde spelling. ESS therefore refuses a stop no `LoopStop` can be, such as a `completed`
stop carrying `limit_ms` (`crates/harness-conformance/tests/stop_union.rs`). The variants
`max-cost`, `deadline`, `awaiting-approval`, `unstructured` and `failed` are declared but not
executed: declaring their shape is not executing those paths, and `failed` is never returned by
`run_in`.

ESS 0.77.0 writes a union value adjacently tagged: the tag, and a payload variant's fields under
`value` (`{"kind": "max-turns", "value": {"limit": 1}}`); a unit variant such as `completed` is the
tag alone. `LoopStop` serializes internally tagged, its fields beside `kind`, and ESS has no form
for that. The scenarios therefore write the adjacent form, and the adapter
(`crates/harness-conformance/src/boundary.rs`, `tagged`) moves the production serialization's
fields under `value` without computing any. The unit variant needs specification format `ess/22`.

The following are **UNMAPPED** into this first ESS slice. They remain implemented and covered by
existing repository tests, which are not automatically ESS conformance evidence. Closing each item
requires source-derived domain declarations and scenarios that execute its named production
boundary; no owner decision or invented ownership/cardinality is needed merely to admit these gaps.

- **UNMAPPED: remaining loop control and accounting.** Cost accumulation, elapsed deadlines,
  deferred approval checkpoint/resume, structured output, retry, hooks, delegation and compaction.
  Sources: `crates/harness-loop/src/lib.rs:223,2649,2826,3161`;
  `price.rs:90`, `answer.rs`, `delegate.rs`, `hook.rs`. Add controlled clock/provider fixtures and
  real checkpoint round trips before claiming these stop variants or controls execute.
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
