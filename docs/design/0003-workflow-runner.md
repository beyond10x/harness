# Design 0003 — `b10x-harness workflow run`: the loop walks a flow, the governor stays outside

**Status:** proposed 2026-08-29. Implements `ROADMAP.md` Phase 8. Each section states what the
first milestone ships and what is left as a labelled later milestone.

## The problem, in one line

`crates/harness-flow` (1,891 lines, 27 tests) can plan and walk a workflow and nothing binds it:
every `StepRunner` is in its own `tests.rs`, `crates/harness-cli/Cargo.toml` does not depend on it,
and the only way a workflow runs this loop today is `aep drive run` in AEP
spawning the binary once per `llm` step through `metaharness run b10x` (`drive.rs:3800-3803`),
with the step's prompt, `--context`, `--write-scope` and `--allow-program` — the loop never sees
the graph, every step starts cold, and a retreat pays for its context again. Phase 5's consumer
embeds the loop as a library, and a `aep drive → metaharness → b10x-harness` process tree per
step cannot be embedded. So the runner is this component's.

## 0. What is already decided, and where

| decided | where |
|---|---|
| the notation: a DAG of sub-trees; an edge joins siblings only; a retreat is `Repeat`; only `gives` crosses a group boundary | `crates/harness-flow/src/lib.rs:1-60` |
| the walk knows order and failure and nothing else; what a step *is* belongs to the caller behind `StepRunner` | `crates/harness-flow/src/run.rs:1-6` |
| a group is a context scope: same `scope` = one warm conversation; a new scope starts from `available` and nothing else | `run.rs:20-38`, `StepContext` |
| AEP projects `adp/default/2` into this notation, and says the projection is an ordering, not a government: guards, `declined`, early exit dropped | `crates/protocol-cli/src/flow.rs:16-31`; `fixtures/adp-default.projected.yaml:1-16` |
| the governor stays outside: this harness embeds nothing above it (invariant 2) and evaluates no gate | `AGENTS.md:24`; ep `docs/guide/harness.md:16-19` |
| a hook is declared, never discovered, and can only narrow | `crates/harness-cli/src/hooks.rs:3-13` |
| the answer is a tool the loop owns; a run under a schema ends in the `answer` call or `LoopStop::Unstructured` | design 0002 § 1 |
| a session is one file per conversation, replayed verbatim, never a credential or the instruction | `README.md` § Sessions |
| the argv surface is a contract; a changed flag opens a new dated version | `contracts/cli/b10x-harness/2026-08-29.1/README.md:1-6`, invariant 13 |
| this is not an eval arm; measured only as cost/tokens/wall-time under the same governor | `ROADMAP.md` Phase 8 |

## 1. The verb

```console
b10x-harness workflow plan --flow <FILE>                       # validate, print the plan; no endpoint
b10x-harness workflow run  --flow <FILE> --input <TEXT> [run flags…]
```

`workflow` is a subcommand with two verbs, beside `run`, `chat`, `sessions`, `tools`, `app-server`
and `events`. `plan` takes only `--flow` (and `--max-attempts`, see below) and contacts nothing,
like `tools`: it answers *does this document validate, and what runs in what order* for free.
`run` flattens `RunOptions` exactly as `chat` does, plus:

| flag | meaning |
|---|---|
| `--flow <FILE>` | the document; `.yaml`/`.yml` or `.json`, decided by extension, refused by name otherwise |
| `--input <TEXT>` | the task, given to every step beside its own prompt — the same word `run` uses |
| `--max-attempts <N>` | overrides every `repeat.max` in the document, **the root's included**, for a document that carries none; absent = the document's own bounds. The root is a group like any other, and the one that holds the steps of a flat projection |

`--output-schema` is **not a flag of `workflow run`**: the runner derives the schema each step
answers under (§ 2), so there is nothing for a file to shape and the flag is not declared. Typing it
is an unrecognised argument, which clap answers before any harness code runs — it is not a refusal
this component words.

Refused before anything runs, in the loop's own words: `--resume` (a flow names its own sessions —
§ 4), a document that does not validate (`FlowError`, at the path it was found), and a `run` payload
that is not an object.

**The document format is `harness-flow`'s own `Flow`**, deserialised as it is; nothing is added to
the notation for this verb. YAML reading lands in `harness-flow` as `Flow::from_yaml` /
`Flow::from_json` (`serde_yaml_ng`, today a dev-dependency there, becomes a dependency), so the
CLI never parses a document itself.

## 2. A step is a turn — the `StepRunner` binding

`FlowRunner` in `crates/harness-cli/src/workflow.rs` implements `harness_flow::StepRunner` over one
`Prepared` (client, tools, approver, config, hooks) — the same objects `run` builds once in
`prepare` — and drives one `AgentLoop::run_in` per step, exactly as `chat` drives one per line.

**What a step reads from its `run` payload.** All optional; the projection today carries only the
first two (§ 7 E1 is what fills the rest):

| key | used as |
|---|---|
| `kind` | absent or `llm`: one model turn; `command`: one gated `run` call; `operator`: stop at this step and hand its `prompt` to a person |
| `state` | the step's name in prose and in the record |
| `summary` | the step's prompt when `prompt` is absent |
| `prompt` | the step's prompt |
| `context` | file paths **inside** the workspace, read like `--context` and named in the step input; a name that is absolute, that resolves outside the canonicalised workspace, or that is not there fails the step by name (`warning`, `code: "context-refused"`, naming the path and the workspace) with no model call, and the walk skips what needed it |

`kind` is a closed vocabulary. An unknown word, a non-string kind, a malformed command, or an
operator step with no non-empty `prompt` refuses the document in `workflow plan`, before a run is
prepared. A producer adding a new kind can therefore never turn it into a paid model step by
accident.

**The step input** is one user turn: the flow's `--input`, then *"You are in step `<path>`, attempt
`<n>` of section `<scope>`"*, then the handoffs in `available` rendered as *"Earlier sections
established:"* followed by one `name: value` line each, then the step's prompt. Nothing else
crosses: a sibling's transcript never reaches a step in another scope, which is the context rule
the notation already states.

**Only a section that came out clean contributes to `available`.** What a failed one produced is in
its own record — `GroupLeft.gave` says what it had — but it is a result nobody accepted, whether its
own steps failed or a governor declined its leave, and a later section reading `specification_id`
would have no way to tell an accepted value from a rejected one.

**Scope = conversation.** The runner keeps one `Vec<Item>` per `(scope, attempt)`. Steps with the
same `StepContext.scope` and `attempt` continue the same items; a new scope, or a new attempt of
the same scope, starts from an empty vector. `Repeat` therefore re-runs a section from nothing but
`available`, which is what *"the whole scope re-runs"* means in `harness-flow`'s own tests.

**How a step says what it did: the `answer` tool.** Every step runs under an output schema the
runner derives — the model never sees a schema file:

```json
{ "type": "object", "required": ["outcome"],
  "properties": {
    "outcome": { "enum": ["passed", "failed"] },
    "note":    { "type": "string" },
    "gives":   { "type": "object", "properties": { "<each name the enclosing group gives>": {} } } } }
```

`outcome` is the `StepOutcome`. `gives` is collected per scope, last write wins, and handed over
from `StepRunner::handoff` — the walk already fails a group whose handoff misses a promised name
(`HandoffIncomplete`), so a group that promised `specification_id` and never answered with it
fails by the notation's own rule, not by a new one. It fails **once**: a broken promise is not
retried whatever `repeat.max` says, because the section came out clean and still did not produce
what its own document declared, and a second attempt buys the same answer again at full price. A
caller who wants that retreat has the `leave` gate (§ 3), where somebody decided it. `LoopStop::Unstructured` (the model ended in
prose after the nudge) is `Failed`, with the prose kept in the session. A step whose group gives
nothing still answers `outcome`; the nudge and the `answer` path are unchanged from design 0002.

**Outcome mapping, exhaustively.**

| loop said | step is |
|---|---|
| `Completed` with an `answer` of `outcome: passed` | `Passed` |
| `Completed` with `outcome: failed`, or `Unstructured` | `Failed` |
| any budget stop — `MaxTurns`, `MaxOutputTokens`, `MaxInputTokens`, `MaxCost`, `Deadline`, `ProviderIncomplete` | `Failed`, and the reason is in the record |
| `Cancelled` | the flow stops: what ran is filed, `flow-finished` is **not** emitted, exit 2 |
| `LoopError` (a wire, credential or protocol failure) | **the flow aborts**, exit 1 — a broken wire is nobody's failed step, and a walk that recorded a network blip as `Failed` would misreport the plan |
| `kind: operator` | `Paused { reason: prompt }`: terminal `flow-paused`, exit 0; the step is reached, not failed, and no downstream skip, group leave, retreat or `flow-finished` is emitted |

**Budgets.** `max_turns`, `max_output_tokens`, `max_output_tokens_per_turn` bound each step, as
each `run_in` counts them. `max_cost_microunits` and `max_duration_ms` bound the **flow**, and the
runner is what makes that true: `run_in` *writes* the caller's `RunLedger` rather than adding to it
(found by story B — `chat` is per line for the same reason), so the runner keeps the cumulative
ledger and the flow's start instant itself and hands each step a budget derived from what
**remains** — ceiling minus spend so far, duration minus elapsed. The loop's own per-step
enforcement then does the work. A step that would start with nothing left is `Failed` with a
`warning` (`code: "flow-budget"`) naming the ceiling and the spend, and no model call. The flag
help says so.

**Tools, scope and approvals** are the run's — built once in `prepare`, the same catalogue for
every step. Per-section narrowing in this milestone is the `before-call` hook's, which already
sees every call with its invoked entry and can refuse by path. A published toolset per group is
M2 (§ 6).

## 3. Transitions — the fourth hook point

The governor is any program. It is consulted at the two moments a section boundary is crossed,
through the `--hooks` file, under the same rules as the three points that exist: declared, never
discovered; it can only narrow; an argv, never a shell; the credential removed from its
environment; `HOOK_TIMEOUT` and `MAX_HOOK_STDOUT_BYTES` unchanged.

**In `harness-flow`,** two defaulted methods on `StepRunner`, so the walk can be told *no* at a
boundary without knowing what a hook is:

```rust
pub enum Gate { Proceed, Refused { reason: String } }

fn entering(&mut self, path: &str, attempt: u32) -> Gate { Gate::Proceed }
fn leaving(&mut self, path: &str, attempt: u32, failed: bool, handoff: &Handoff) -> Gate { Gate::Proceed }
```

| moment | `Refused` means | walk does |
|---|---|---|
| `entering` a group | this section may not run now | the group is skipped **as failed**: `NodeSkipped` for every step inside it, `GroupLeft { failed: true }`, and what needed it is skipped — the same as a failed group |
| `leaving` a group that came out clean | the governor does not accept the section's result | the attempt is marked failed; with attempts left, `Repeat` re-enters — **this is how an engine forces a retreat**; at the bound, `GroupLeft { failed: true, exhausted }` as today |
| `leaving` a group that failed | nothing changes | already failed; the refusal is recorded and that is all. The `handoff` it is shown is **empty** unless this was the last attempt: an attempt that is going round again is never asked what it hands over, so there is nothing for a governor to read, and it must not infer *the section produced nothing* from that |

Both emit a new `FlowEvent::TransitionRefused { path, moment, attempt, reason }` before the
consequence. `leaving` is asked **after** `handoff`, once per attempt, so the governor sees what the
section is handing over. The root is a group and is gated like one: a refused `entering` of the
root runs nothing, exit 2.

**In `harness-cli`,** `hooks.rs` learns `on: "transition"` in the same file version — a file naming
a point this build does not know was already refused by name, so an older binary refuses a file
with `transition` in it rather than ignoring the hook. The protocol, one JSON document on stdin:

```json
{ "hook": "transition", "flow": "adp/default", "path": "root.implement-to-review",
  "moment": "enter" | "leave", "attempt": 2, "of": 3,
  "failed": false,                  // leave only
  "handoff": { "specification_id": "…" },   // leave only
  "workspace": "/abs/path" }
```

`0` proceeds; `2` refuses with `{"reason": "…"}` on stdout, else stderr; anything else is
`Failed { reason }`, read **fail closed** at both moments — a governor that could not answer did
not say yes, exactly as `before-call`. `HookRan` is emitted for the record as it is for every
point, with `point: "transition"`.

## 4. The record — events, sessions, exit status

**Events.** `FlowEvent` already serialises with `kind` in kebab-case (`flow-started`,
`group-entered`, `layer-ready`, `step-started`, `step-finished`, `node-skipped`,
`group-repeating`, `handoff-incomplete`, `group-left`, `flow-finished`, `flow-paused`, and `transition-refused`
from § 3). Under `--json` they go on stdout in the same stream as the loop's events, one per line;
a step's loop events (`started` … `finished`) appear between its `step-started` and
`step-finished`. On stderr each renders as one line in the run's own style — `flow ▸ root.shape
(attempt 1 of 3)`, `step ✓ root.shape.specify`, `step ✗ root.implement-to-review.verify`,
`retreat ↺ root.implement-to-review (2 of 3): <reason>`. `b10x-harness events` maps a kind it does
not know to `opaque` today (`metaharness.rs:207`); the `flow.*` families for metaharness are M2.

**Sessions.** One session per `(scope, attempt)`, id `<flow-run-id>` followed by every open scope on
the way down as its own name and the attempt it is on — `….root.2.implement-to-review.3.verify.1` —
where the flow-run id is `Session::new_id()` taken once. **Not `<flow-run-id>.<path>.<attempt>`**,
which is what shipped first: an ancestor that is re-entered runs everything under it from attempt 1
again, so the second pass overwrote the first (walk 7, 2026-08-30, lost the `specify` attempt whose
validator exited `1`). A name may not contain a `.` — `FlowError::DottedName` — which is what makes
the pairs readable back; saved through `Session::save` as it closes, with
what it cost; `--no-session` writes nothing; `--session-dir` is honoured. Under `--json` a
`{"kind":"session", …}` line is emitted as each scope's session is filed — the same shape `run`
prints last — and the last line of a finished flow is `flow-finished`. A walk that reaches an
operator step instead ends at `flow-paused { flow, path, reason, reached, failed, skipped,
retreats }`; `reached` includes the operator step, which has no `step-finished`. `--resume` is refused (§ 1);
resuming a **flow** is M2.

**Exit status**, in the table `reference/cli.md` already has: `0` the flow came out clean
(`Report::status() == Completed`) **or is awaiting an operator**; `2` it finished and did not — a step failed, a section was skipped or
exhausted, or the run was cancelled — inspect `flow-finished`; `1` refused before it started, or
aborted on a `LoopError`.

`workflow plan` prints the plan as text — one line per layer, indented per group, `repeat` bounds
beside the group — or, under `--json`, the `Plan` itself; exit `0` valid, `1` refused.

## 5. What lands where

| crate | change |
|---|---|
| `harness-flow` | `Flow::from_yaml`, `Flow::from_json` (`serde_yaml_ng` promoted to a dependency); `Gate`; `StepRunner::{entering, leaving}` with defaults; the two consequences in `walk`; `FlowEvent::TransitionRefused`; tests for every row of the § 3 table, with no provider |
| `harness-cli` | `workflow.rs`: `WorkflowCommand { Plan, Run }`, `FlowRunner`, the step input, the derived schema, sessions per scope, the stderr rendering; `lib.rs`: the `Workflow` arm of `Command`, the refusals in § 1, exit mapping; `hooks.rs`: the `transition` point and its payload; `render.rs`: one arm per flow event; `Cargo.toml`: `harness-flow` |
| `contracts/cli/b10x-harness` | the argv pin regenerated: a **new dated version** if `2026-08-29.1` has been released, else updated in place as 0002 did |
| `harness-loop` | one word and no behaviour: `HookPoint::Transition`, so the boundary consultation files the same `LoopEvent::HookRan` every other point files. No port method, nothing in `AgentLoop` asks at it, and a `HookPort` implementation has nothing to add — the loop still does not know it is inside a flow |
| `harness-wire`, `harness-app-server` | **nothing.** No new event kind, no new wire item: the flow's events are the flow crate's |
| docs | `README.md` § Workflows; `website/docs/guides/workflows.md`; `reference/cli.md` (commands, exit status); `CHANGELOG.md` Added; `STATUS.md` test counts; `ROADMAP.md` Phase 8 status line |

## 6. Not in this milestone

- **Resuming a flow.** A crashed flow leaves its per-scope sessions and its record; nothing today
  turns them back into a cursor. M2: a flow cursor file beside the sessions, `workflow run
  --resume <flow-run-id>`.
- **A published toolset per group.** **Half done (2026-08-30): the write scope.** A step's node
  carries the map's own first-match-wins `scope:` list, and it is now laid over the run's for the
  length of that step and taken off again however it ends — refused in the tool, before the write,
  as a failed `ToolOutcome` (invariants 9, 12). It can only narrow: both layers are asked and the
  first refusal wins, so a generated document cannot give back what `--write-scope` denied, and a
  node declaring nothing runs under the run's exactly as before. A `scope` this build cannot read
  refuses the document by node path at read time rather than falling through to the run's. It is
  the half with a safety consequence — a narrower toolset is a smaller surface, but an unenforced
  `denied` is a stated rule that does not hold. What is still open is the rest: the **tool list**
  and **`--allow-program`** are the run's for every step, so a section cannot yet publish fewer
  entries than the run does and a `command` step whose program the run does not declare is still a
  refused step. That half rebuilds `Published` per section through the same `published()` path.
- **Parallel layers.** `Plan` says what may run beside what; the walk is sequential over
  `&mut dyn StepRunner`, and the loop holds one client. M3, after a measurement says it is worth
  a client per thread.
- ~~**Command steps.**~~ **Done (M2, 2026-08-30).** A `run.command` argv is one `run` call made
  through `AgentLoop::call` — the gate a model's call meets, in its order — without a model turn,
  filed into the section's conversation as the call and its result, and read as the step's
  outcome: exit `0` passed, anything else failed by name (`workflow.rs`, *A `command` step is a
  call, not a turn*). What is still open is a toolset per step: a command runs under the run's own
  `--allow-program`, so a verifier the run does not publish is a refused step.
- ~~**Operator steps.**~~ **Done (M2, 2026-08-31).** `run.kind: operator` with a non-empty prompt
  returns the scheduler's typed `Paused` outcome before the step narrows scope, checks a budget or
  reaches a tool, approval, call hook or provider. The pause propagates through every open group
  without leaving or repeating it, emits one terminal `flow-paused`, and exits 0. Resume remains
  the separate cursor work above: the pending tail is not reported as skipped.
- **The metaharness projection of `flow.*`** into `trace-ir/1` families, or their listing under
  `CONTROL_PLANE_EVENTS`. Only when an eval asks for a native-flow record.
- **Live evidence.** Everything below is `provider_emulated` until one authorized run walks the
  projected `adp/default/2` under a real governor (invariant 18).

## 7. The other side — AEP

Two things this design asks of that repository, each its own story there, neither blocking § 5:

- **E1 — `aep govern workflow flow --map` is accepted and ignored.** `FlowArgs.map` is declared
  (`crates/protocol-cli/src/flow.rs:59`) and never read; `project(workflow, max_attempts)` takes
  no map. The verb's own help says a node *"carries what a harness actually does in that state"*
  with one. The story: thread the step map so each node's `run` carries the `llm` step's `prompt`,
  `context` and `scope`, and a `command` step its argv — the keys § 2 reads.
- **E2 — a governor program.** `aep drive transition` (name to be decided there): reads the
  § 3 JSON on stdin, positions the engine on the run's cursor, and answers `enter`/`leave` from
  `evaluate`/`transition` — `Blocked { reasons }` is exit 2 with the engine's own words. That
  makes a native flow *governed* by the same engine that governs the driven arm, with no crate
  dependency in either direction, which is the bridge `ROADMAP.md` Phase 8 names. Waits for § 3
  to be pinned by a released contract.

## 8. Evidence this needs

Without a provider, in `harness-flow`: every row of the § 3 table — a refused `entering` skips the
group as failed and names every step inside it; a refused `leaving` on a clean attempt re-enters
until the bound and is exhausted by name; a refused `leaving` on a failed attempt changes nothing;
`from_yaml` reads the committed fixture and `from_json` reads its JSON twin.

Through the shipped binary, over **both** emulators, `provider_emulated`:

| test | proves |
|---|---|
| `a_flow_walks_its_plan_and_files_one_session_per_scope` | two groups, three steps: order, `available` rendered into the second scope's first turn and not the first scope's transcript, two session files with the ids of § 4 |
| `a_walk_whose_root_retreats_files_one_session_for_every_section_that_ran` | a root with `repeat: {max: 2}` and a step of its own: four `group-entered`, four session files, no two ids alike, and the attempt that failed still readable under the root attempt it ran in |
| `a_step_that_answers_failed_skips_what_needed_it_and_the_flow_exits_2` | `outcome: failed`, `node-skipped` for the dependant, `flow-finished` not clean |
| `a_transition_hook_that_refuses_a_leave_re_enters_the_section_until_its_bound` | `repeat: {max: 2}`, a hook script refusing every `leave`: two attempts, `transition-refused` twice, exhausted, exit 2 |
| `a_transition_hook_that_refuses_an_enter_skips_the_section_by_name` | `enter` refused: no model call for that section, its steps `node-skipped`, exit 2 |
| `a_hook_that_cannot_answer_a_transition_fails_closed` | a hook exiting 3: `Failed`, read as a refusal |
| `the_projected_adp_workflow_walks_end_to_end` | `fixtures/adp-default.projected.yaml` over both emulators with a scenario answering every step `passed`: exit 0, one session per scope, `flow-finished` clean |
| `an_operator_step_pauses_with_exit_zero_and_contacts_neither_wire` | both emulators record zero requests; one `flow-paused` names the path and prompt, the operator step is reached rather than failed, and no finish, skip, leave, retreat, tool, approval or call-hook event follows |
| `workflow_plan_prints_the_layers_without_an_endpoint` | no `--base-url`, no credential, no socket |
| `a_flow_that_does_not_validate_is_refused_before_any_session` | a cycle: exit 1, the `FlowError` text, no session directory written |
| `a_wire_failure_aborts_the_flow_and_files_what_ran` | the emulator dies mid-step: exit 1, the sessions so far on disk |

Both emulators need one new scenario each that answers a fixed sequence of `answer` calls, one
per step, keyed by turn — the same mechanism the `answer` scenarios of 0002 use.

## 9. Stories, order, and who does what

Each story is one sub-agent in its own worktree with its own `target/` (`AGENTS.md` shared-target
rule), leaving a patch and no commit. Sizes are estimates.

| wave | story | scope | files | est. |
|---|---|---|---|---|
| 1 | **A** the notation's half | § 3 gates, `TransitionRefused`, `from_yaml`/`from_json`, tests | `crates/harness-flow/**`, workspace `Cargo.toml` (`serde_yaml_ng`) | ~350 lines |
| 1 | **E1** (AEP) | thread `--map` into `run` payloads | `crates/protocol-cli/src/flow.rs`, its tests, the fixture twin | ~200 lines |
| 2 | **B** the verb | § 1, § 2, § 4 without the hook; contract pin; emulator scenarios; e2e | `crates/harness-cli/src/{workflow,lib,render}.rs`, `Cargo.toml`, `contracts/cli/**`, `tests/workflow.rs`, both `fake_*.py` | ~900 lines |
| 2 | **C** the words | README, guide, reference, CHANGELOG, STATUS, ROADMAP line | docs only | ~250 lines |
| 3 | **D** the governor's port | § 3 in `hooks.rs`, the runner's calls, the three hook tests | `crates/harness-cli/src/{hooks,workflow}.rs`, `tests/workflow.rs` | ~300 lines |
| 4 | integration | one wave branch, `--no-ff`, full gate, both emulators, install | — | — |
| later | **E2**, M2 items | § 6, § 7 | — | — |

B starts from A's patch applied; D from B's. C runs beside B against this document and is
corrected against B's tests before landing.
