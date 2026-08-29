# Design 0002 — sub-agents, structured output, hooks

**Status:** proposed 2026-08-29, implemented the same day under `Unreleased`. Each section below
states what shipped and what was left as a labelled later milestone.

## The problem, in one line

`docs/reviews/2026-08-29-sota-comparison.md` finding #13 ranked five things every comparable harness
has and this one does not: sub-agents, structured output, hooks, an MCP client and multimodal input.
`README.md § Not owned here` said each *"is a decision about what this component owns rather than a
defect in it"*. This is the decision for the first three, in the order the operator gave them:
**sub-agents, structured output, hooks**. The MCP client and multimodal input stay out of scope
(§ 5).

The constraint every section answers to is the same one design 0001 answered to: **the published
toolset is the entire safety boundary**, and the approver is the review gate (AGENTS.md invariant
12). Nothing here may reach a tool without the gate, widen what a turn admits, or make a refusal
silent (invariants 8, 9).

---

## 0. What the three have in common: tools the loop owns

Two of the three are tools the model calls, and neither is a catalogue entry. `answer` performs no
neutral operation on a machine; `delegate` performs whatever the run's own tools perform, through
the run's own gate. They belong to the **loop**, not to `harness-tools`, and a `ToolPort` never sees
them:

- `AgentLoop::request` appends their specs to the port's `specs()` for every turn, under the same
  `MAX_TOOLS` bound and the same duplicate-name check — a port that already publishes a tool of
  that name refuses the run by name before the first request (`LoopError::Budget`-style, before
  any byte goes out).
- `AgentLoop::run_calls` resolves each call against the owned tools **first**. An owned call is
  never batched (it is not a port call, and `delegate` holds the model port), never routed by bare
  name, and never handed to `ToolPort::invoked`.
- An owned call still produces exactly one `Item::ToolResult` in the conversation, because a
  provider replaying a call without its result is a hard error on the next turn and a session that
  cannot be resumed.

Both are **opt-in per run** (`LoopConfig::output_schema`, `LoopConfig::delegation`, both `None`
by default). Every invocation written before this change means what it did.

---

## 1. Structured output — an `answer` tool, not a wire feature

### The alternative, and why not yet

Every provider-native path was considered: the Responses wire's `text.format = {json_schema,
strict}` and the Messages wire's `output_format` (a beta, header-gated on that route as of the last
documentation this repo has read). Both would give constrained decoding. Both would also cost a
new pinned contract version per wire (invariants 13–14), and the Messages one would ship on a
feature nothing here has seen live — a run against the real endpoint could refuse it by name and
that would be the first evidence either way.

What ships instead is the mechanism the Claude Agent SDK used before `output_format` existed and
Pydantic AI still uses: **the schema is published as a tool the model calls to finish**. It is
wire-neutral, needs no contract change, is testable end to end against both emulators today, and
composes with delegation (§ 2) for free — a delegate's structured answer is what the parent reads.
Provider-native constrained decoding behind the *same* `OutputSchema` value is **milestone M2**,
cut as new contract versions when a live run shows the tool path failing to adhere.

### The value

```rust
// harness-loop
pub struct OutputSchema {
    pub name: ToolName,        // default `answer`
    pub description: String,   // what the model is told; the default says "call this once, alone, last"
    pub schema: Value,         // a JSON Schema whose top level is an object
}
```

`OutputSchema::new(schema)` refuses by name a schema that is not an object at the top level: a
tool's `input_schema` must be one on both wires, and a refusal before the run beats a 400 after it.

### The mechanism

| moment | what the loop does |
|---|---|
| every turn | `answer` is in `tools`, after the port's specs |
| the model calls `answer` | the arguments **are** the answer: `LoopOutcome.structured = Some(arguments)`; a `ToolResult {"accepted": true}` is recorded; `LoopEvent::Answered { call_id, value }` is emitted; **every other call in that turn is refused** with *"refused: made in the same turn as `answer`, which must be called alone"*, because the answer was declared to be the last thing; the run stops `Completed`. The refusal does **not** say the run ended: the siblings are refused before the answer is tried, and an answer can still fail (below), leaving the run turning |
| the `answer` call itself fails — arguments that are not an object, arguments over `MAX_TOOL_ARGUMENT_BYTES`, a `before-call` hook that blocked it | a failed `ToolOutcome` saying so and **no stop**: the run turns again and the model can answer properly. Its siblings were already refused, by the sentence above, which is why that sentence claims nothing about the run's ending |
| the model ends a turn in prose with no `answer` call | one user item — *"Finish by calling `answer` with the result; nothing else is read."* — and one more turn, at most [`MAX_ANSWER_NUDGES`] = 1 times **per ending**; the nudge is a turn like any other and is charged to every ceiling. A `stop` hook that sends the run back to work (§ 3) starts a new ending, so the count is reset: a run that answered, was told to go on and then replied in prose is asked once more rather than ending `Unstructured` unasked |
| still prose | `LoopStop::Unstructured { asked_again: 1 }` — **not** `Completed`: a consumer that piped stdout to `jq` and got prose with exit 0 would be the silent failure invariant 8 forbids |

The loop parses nothing and validates nothing against the schema: what the provider accepted as
tool arguments is what the caller gets. Validation in the loop is **milestone M3**, and it is not
free — a JSON Schema validator is a dependency this workspace has argued against once already
(`Cargo.toml`, the note above `globset`).

### On the command line

`b10x-harness run --output-schema <FILE>`: the file is the schema. **Standard output is the answer
and nothing else** — one line of JSON — so the command composes; the model's prose deltas go to
stderr as notes. Under `--json` stdout is the event record and the answer is the **last** `answered` event before a `finished` whose `stop` is `completed` — a `stop` hook can withdraw an earlier one, so the first is not the answer. An `Unstructured` stop exits
2 like every other stop-without-an-answer. `chat` does not take it: a conversation has no single
end.

The session stores `structured` beside `text` (an optional field; a v1 session without it reads as
before).

---

## 2. Sub-agents — `delegate`, a fresh context on the same gate

### What a delegate is

A tool call that runs **a second `AgentLoop` to completion inside the first one's tool call**, over
a conversation that starts empty, and returns that loop's final text as the tool result. What the
child sees is exactly design 0001 § 4's *handoff*: the parent's standing instruction (environment
block, project instructions, catalogue brief — unchanged, so the delegate knows where it is) plus a
delegation preamble, and the `task` string. It does **not** see the parent's conversation, and the
parent never sees the child's — only the result. That is the point: a sub-tree that reads forty
files to answer one question costs the parent one tool result, not forty reads in its context.

### What it shares, and what it does not

| the child gets | why |
|---|---|
| the **same `ModelPort`**, reborrowed | the parent is blocked inside the call, so the port is idle; no second credential, no second client |
| the **same `ToolPort`** | delegation widens nothing: the child can do exactly what the parent's catalogue admits, entry for entry |
| the **same `ApprovalPort`** | a person is asked about the child's `file_write` exactly as about the parent's; the `ApprovalRequired` event arrives wrapped (below) so a renderer can say who is asking |
| the **same `HookPort`**, for `before-call` and `after-call` | an operator's hook on `run` fires in a delegate too, or it was not a hook on `run`. The `stop` hook does **not** fire at a delegate's end: a child's ending is not the run's ending, and a block there would turn the child again on the parent's carved budget |
| the **same cancellation token** | Ctrl-C reaches the innermost blocked read |
| **the remainder of the parent's budget** | every ceiling the parent has — turns excepted — is carved: `limit − spent so far`; a delegate spends the run's budget, never its own; the parent absorbs the child's usage and cost when the call returns, so the parent's ceilings bind on the sum and `stop_after_tokens` fires before the parent's next turn |
| **its own turn ceiling** | `Delegation::max_turns` (default 20): a child that loops does not spend the parent's remaining fifty turns |
| **the remaining wall clock** | as every tool call does |
| **its spend absorbed by the parent on every exit path** | a child that failed on the wire after three turns still spent them; the parent's ceilings bind on that too |
| **no `delegate`** | depth 1: `Delegation::depth` counts down and a child at depth 0 publishes no `delegate`. A tree of delegates is **milestone M4**, with concurrency, when a run shows a need |
| **no `answer`** | the child's result is its text; a schema for the child is **milestone M2**, one field on the call |

### The spec the model sees

```json
{ "name": "delegate",
  "input_schema": { "type": "object", "required": ["task"], "additionalProperties": false,
                    "properties": { "task": { "type": "string" } } } }
```

Envelope: `effects: []`, `risk: Low`, `idempotency: NonIdempotent`, `access: []`. That is honest
under this design's rule that a spec is a claim about *this* call: the delegate call itself touches
nothing — every effect inside it is a call of its own, gated on its own entry's envelope. The
description tells the model what it is for: *"a fresh context for a self-contained sub-task —
research, a survey of many files, a change you can state in one sentence; it cannot see this
conversation, so say everything it needs; it reports once, in text."*

### The result

`{"stop": <LoopStop>, "turns": n, "text": "..."}`, `failed` when the child did not complete —
a bound it hit, a wire error, a cancellation — with the reason in the output, so the parent learns
the sub-task did not finish rather than reading a half-answer as whole. Bounded by
`MAX_TOOL_RESULT_BYTES` like every result, refused by name past it (the preamble tells the child to
report in under that).

### Events

Everything the child emits reaches the parent's sink **wrapped**: `LoopEvent::Delegated { call_id,
event: Box<LoopEvent> }`. A renderer indents; the JSONL record nests; the bridge ignores. The child's
`Usage` and `Cost` events therefore appear only inside `Delegated`, and the parent emits none of
its own for them — a reader summing top-level `Usage` events sees the parent's turns, and
`LoopOutcome.usage` (the parent's) includes the child's entries so totals are right.
`DelegateStarted { call_id, task }` and `DelegateFinished { call_id, stop, turns }` bracket it.

### On the command line

`--delegate` publishes it; `--delegate-turns <N>` sets the child's turn ceiling (default 20). Off
by default in this version — a new tool is a change in what the model can do — and the standing
instruction names it in one line when it is on.

---

## 3. Hooks — declared, never ambient; narrowing, never widening

### The two rules

1. **A hook is named on the command line, never discovered.** `--hooks <FILE>`. A hook found in
   the workspace would be a program the *repository* runs on the operator's machine, which is the
   ambient-fallback the safety envelope forbids for credentials, and the argument is the same.
2. **A hook runs after the gate and can only narrow.** `before-call` fires after the approver said
   yes; its `block` is one more refusal. It cannot approve, cannot change the call, and cannot
   reach a tool the run did not publish. A hook that widened would be a second gate nobody reviews.

### The port

```rust
// harness-loop
pub enum HookPoint { BeforeCall, AfterCall, Stop }
pub enum HookDecision { Proceed, Block { reason: String }, Failed { reason: String } }

pub trait HookPort {
    fn before_call(&mut self, call: &ToolCall, invoked: &ToolSpec) -> HookDecision;
    fn after_call(&mut self, call: &ToolCall, invoked: &ToolSpec, outcome: &ToolOutcome) -> AfterCall;
    fn on_stop(&mut self, text: &str) -> HookDecision;
}
pub struct AfterCall { pub note: Option<String>, pub decision: HookDecision }  // never a block
pub struct NoHooks;   // every default; the loop's default
```

`AgentLoop::with_hooks(&mut dyn HookPort)`. The loop spawns nothing: the port is the seam, exactly
as `ApprovalPort` is, and the process-running implementation lives in the shell.

| point | fires | `Proceed` | `Block { reason }` | `Failed { reason }` |
|---|---|---|---|---|
| `before-call` | after approval, before `ToolPort::call_within` | the call runs | failed `ToolOutcome`: *"`file_write` was blocked by a hook: {reason}"* | **fail closed**: same as `Block` — a hook that could not run did not say yes |
| `after-call` | after an outcome a tool produced, before it is recorded | the note (if any) is appended to the result under `hook_notes`; a string result is wrapped as `{"output", "hook_notes"}` — the model reads that the formatter ran | not a block: the effect has already happened, so there is nothing left to refuse, and exit 2 is read as a note | the failure is recorded as `HookRan { decision: failed }` and its reason reaches the model as a note. The outcome's own `failed` is untouched — an after-call hook cannot fail a result |
| `stop` | when the run would end `Completed` (prose, or after `answer`) | the run ends | the reason becomes one user item and the loop turns again — at most [`MAX_STOP_HOOK_CONTINUES`] = 3 times per run, then `stop-hook-exhausted` is warned and the run ends; an `answer` already given is withdrawn, and a later one replaces it | **fail open** with a `hook-failed` warning: a hook that crashed must not keep a run alive for ever |

**`after-call` does not fire for a call that never ran, and that is intended.** A name the run
never published, arguments over `MAX_TOOL_ARGUMENT_BYTES`, an approver's denial and a `before-call`
block all return before the tool, so there is no outcome a tool produced — and that is the only
thing this point is about. None of it is silent: every one of those refusals is a
`ToolCompleted { failed: true }` in the record, with `ApprovalResolved { approved: false }` beside a
denial and `HookRan { point: before-call }` beside a block, and each reaches the model as a failed
`ToolOutcome`. An audit that must see refusals reads those events; a hook that must see what a tool
did reads this point. Firing `after-call` on refusals would ask a hook to read an `outcome` no tool
wrote.

Batching and hooks: **a run with hooks attached batches nothing.** Every call goes through the one
path where hooks fire, so a hook fires exactly once per call and never twice on a miscounted group.
Hooks are opt-in per run, so the latency is paid only by runs that asked for them.

Every firing emits `LoopEvent::HookRan { point, call_id, decision }`, so a reader of a finished
run sees which hook decided what.

### The file and the protocol

```json
{ "version": 1,
  "hooks": [
    { "on": "before-call", "tools": ["file_write", "file_edit", "run"], "command": ["/usr/local/bin/guard"] },
    { "on": "after-call",  "tools": ["file_write", "file_edit"],        "command": ["/usr/bin/rustfmt-check"] },
    { "on": "stop",                                                       "command": ["/usr/local/bin/tests-pass"] } ] }
```

- `command` is an **argv, never a shell string** — the same rule `run` has.
- `tools` filters by the **invoked entry's** name (`file_write`, not `tool_invoke`); absent means
  every call — the loop's own `answer` and `delegate` included, consulted with their own spec.
- The hook reads one JSON document on stdin — `{ "hook", "call": {call_id, name, arguments},
  "entry", "outcome"?, "text"?, "workspace" }` — and answers with its exit status: `0` proceed,
  `2` block with the reason from `{"reason": …}` on stdout or else from stderr, anything else
  `Failed`. Stdout past 16 KiB is `Failed` by name. A hook is killed at `HOOK_TIMEOUT` = 60 s,
  and that is `Failed`. (The port is not told the run's remaining wall clock — the loop's deadline
  check between calls is what bounds the overshoot, as it does for a tool call.)
- Hooks run **on the operator's machine, unconfined**. They are the operator's own programs,
  declared by the operator, and never the model's. One thing they do not inherit: the environment
  variable this run's credential was named in (`--api-key-env`, `--oauth-token-env`) is removed
  from the child's environment — unconfined is not the same as handed the run's own bearer.
- The answer is written to stdout **once, at the end, and only when the run completed**: a `stop`
  hook may withdraw an answer and a later one replace it, so the line a consumer reads is the
  answer the run ended with.

---

## 4. What lands where

| crate | change |
|---|---|
| `harness-loop` | `OutputSchema`, `Delegation`, `HookPort`/`HookDecision`/`HookPoint`/`NoHooks`; `LoopConfig::{output_schema, delegation}`; `AgentLoop::with_hooks`; `LoopStop::Unstructured`; `LoopOutcome::structured`; events `Answered`, `DelegateStarted`, `DelegateFinished`, `Delegated`, `HookRan`; the owned-tool resolution in `run_calls`; the nudge and the stop hook in `drive`; the nested loop |
| `harness-cli` | `--output-schema`, `--delegate`, `--delegate-turns`, `--hooks`; `hooks.rs` (file, protocol, process); renderer arms; session field; the argv pin `contracts/cli/b10x-harness/2026-08-29` **updated in place** — it is unreleased, and invariant 13 immutability starts at release |
| `harness-app-server` | ignores the new events; publishes none of the three (the client mediates its own tools; hooks and delegation there are the client's) |
| `harness-wire` | **nothing.** No new field, no new event: the answer travels as a tool call, and that is the reason § 1 chose it |

## 5. Not in this design

- **MCP client.** The loop would become a client of a protocol, and the tools it discovers would be
  ones nothing here confines (design 0001 § 2). metaharness is the MCP side of this family.
- **Multimodal input.** `Item::UserText` is text; an image item is a new neutral value on both
  wires and a new contract version each. Nothing that measures this harness has asked for it.
- **Provider-native structured output (M2), schema validation in the loop (M3), delegate trees and
  parallel delegates (M4).** Each waits for the evidence that the shipped path is not enough.

## 6. Evidence this needs

All of it is `provider_emulated` until a live run: the `answer` path on both emulators (call, nudge,
`Unstructured`), a delegate that reads three files and reports, a `before-call` hook that blocks a
write, an `after-call` hook that notes, a `stop` hook that turns the run once more. The first live
measurement to take is the one § 1 named: how often a real model ends in prose under `answer`.
