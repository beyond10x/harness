# Design 0001 — the tool envelope, where effects land, and what a step sees

**Status:** proposed 2026-08-23; **landed in `0.1.0`** (2026-08-24), with the deviations recorded
here rather than left for a reader to notice. § 1 — `ToolSpec::envelope` and `ToolPort::subjects`
exist; `approval` is still on the spec and marked for retirement (`crates/harness-wire/src/turn.rs`).
§ 2 — publication follows substrate's facts as designed, but the embedded backend **imports
`substrate-host`** rather than speaking only the wire, argued in
`crates/harness-substrate/src/embedded.rs`; the socket client exists beside it and is parked
(`STATUS.md`). § 3 — the three-verb amendment below is what shipped. § 4 — a group is a context
scope and `gives:` is in the notation (`crates/harness-flow/src/run.rs`, `StepContext`). § 5 — all
five steps landed. The text below is left as argued.

## The problem, in one line

The harness publishes three read-only tools because **the toolset is the entire safety boundary** —
`README.md` says so outright: *"this harness's effects are exactly what its toolset admits and
nothing constrains it further."* A `write` tool would write to the operator's real machine and a
`bash` tool would run on it. To publish either, something has to exist underneath.

Two halves follow, and they are independent:

1. **What a tool declares about itself** — the shape `flux-tools` and connector operations already
   use: effects, risk, idempotency, access, and per-invocation subjects.
2. **Where the effect actually lands** — [`beyond10x/substrate`](https://github.com/beyond10x/substrate),
   the successor to `flux-system`.

A third question rides along with the DAG and is answered in § 4: **what does a step see?**

---

## 1. What a tool declares

`harness_wire::ToolSpec` today is `name`, `description`, `input_schema`, `approval`. That is enough
to publish a tool and not enough to decide anything about it. The proposal is the vocabulary
`flux-spec` already froze, because it has been through the argument once:

```rust
pub struct ToolSpec {
    pub name: ToolName,
    pub description: String,
    pub input_schema: Value,
    // new
    pub effects: Vec<Effect>,        // Read | Write | Network | Process | Filesystem
    pub risk: Risk,                  // Low | Medium | High | Destructive
    pub idempotency: Idempotency,    // Idempotent | NonIdempotent | Conditional
    pub access: Vec<AccessKind>,     // Filesystem | Process | Network | Secret | …
}
```

and on the port, per invocation rather than per tool:

```rust
pub trait ToolPort {
    fn specs(&self) -> &[ToolSpec];
    /// The concrete things *this call* touches: `file:crates/x/src/y.rs`, `proc:cargo test`.
    fn subjects(&self, call: &ToolCall) -> Vec<Subject>;
    fn call(&mut self, call: &ToolCall) -> ToolOutcome;
}
```

### The one rule worth stating

**A spec is a claim; the subjects are the fact.** A tool that declares `Write` and is handed a path
outside the workspace must be refused on the *subject*, not on the spec — the spec was correct and
the call was not. This is flux's own position: *"a generic `Read` is pure unless paired with a
concrete access kind"*. Without it, declaration becomes a promise nobody checks, which is worse than
no declaration at all because it reads like a boundary.

### The vocabulary invariant, taken unchanged

From `flux-spec` (C-184): **a variant names a consequence class — what could go wrong, who sees it,
whether it can be undone — never an application domain.** "Runs the test suite", "creates a planning
artifact" and "opens a pull request" are `Process`, `Write` and `Network` consequences of specific
domains. Giving each domain a variant grows an unbounded catalog on a frozen wire enum, and flux
already carries one deprecated variant (`Calendar`) that proves the point.

### `approval` becomes derived, not declared

Today a spec carries `Approval::NotRequired`. Under this design the loop *computes* the disposition
from `(spec, subjects, policy)` and the tool stops asserting it. A tool that could declare itself
approval-free is a tool that can opt out of the envelope.

---

## 2. Where the effect lands: substrate

### The boundary substrate itself sets

Substrate's README is explicit: *"Cross-component consumers use the separately released native
`substrate-daemon` artifact and owner-released wire contract; they do not import this implementation
crate. Every operation crosses an authenticated socket boundary."*

So the new crate is **`harness-substrate`, a client of the wire** — an owner-permissioned Unix
socket, subject derived from kernel peer credentials, never from HTTP data. It is not a dependency
on substrate's implementation, and the harness gains no ability to weaken what the daemon enforces.

### The property this whole design turns on

> *"Without a delegated cgroup root, workspace operations remain served and exec confinement facts
> are absent, so exec admission answers `exec.sandbox-unavailable`."*

Substrate **refuses rather than degrading**. That gives the standing principle — *by default
something insecure and open-ended like bash is not allowed; provide tools instead* — a mechanism
rather than a policy:

**A tool whose effects cannot be confined here is not published at all.**

Not disabled, not gated, not refused at call time: absent from `specs()`, so the model never sees
it, never plans around it, and never spends a turn being told no. The toolset is a function of what
the machine can confine, computed once at startup from substrate's own probe.

### Amendment (2026-08-29): silent to the model, stated to everyone else

The paragraph above is right about the model and was wrong about the record. Publication by absence
means the run's own account of itself cannot distinguish two very different machines: one where a
`run` entry was never wanted, and one where it was **declared** and refused. On a real run they were
confused for weeks — a driven session whose only legal route was starting a program got six entries
instead of seven, no error, no warning and no fact anywhere, hand-wrote files instead, and the
failure was read as the model's.

The fix is additive and changes no gate. Where a program set was declared and the machine does not
confine execution, `Facts::withheld` produces a `Withheld { tool, reason }` naming the predicate
that failed **as the machine stated it**:

| what the machine said | reason |
|---|---|
| no facts at all (`Facts::none()`) | states no capability facts at all — no daemon answered, or none was asked for |
| `exec.argv-only` absent or `false` | must be true and this machine says `nothing` / `false` |
| `exec.cgroup-limits` short | must state `cpu`, `memory` and `processes`, and names the ones it does not |
| `workspace.guarded-io` absent, with a confinement named | must be true and this machine says `nothing` — takes `file_write` and `file_edit` with it |

Every reason a *stated* fact produced also carries one line about cgroups, because the term of
substrate's exec conjunction that fails on a developer machine is `probe_cgroup`, which reads the
**probing process's** own `/proc/self/cgroup` — so a login shell in `session-M.scope` and the same
command under `systemd-run --user --scope` get different answers from one machine, and a reason that
named only the absent fact would send a reader into substrate's configuration for a fault in how the
harness was started. The *no facts at all* reason carries no such line: nothing probed anything.

**Declared, and only declared.** A run that named no programs withholds nothing — inventing the want
would put a line about `run` in front of every read-only run there has ever been.

It travels as `LoopConfig::withheld` → `LoopEvent::Started { withheld }` (additive, skipped when
empty, so older records are byte-identical) → one `note:` line on stderr and a `withheld` array in
`b10x-harness tools`. It is reported and acted on nowhere: the tool is still absent from `specs()`,
so nothing about the first tier moved.

### Three tiers, and which is which

| tier | question | decided by | example refusal |
|---|---|---|---|
| **publication** | may this tool exist here at all? | substrate's confinement facts, at startup | no delegated cgroup root → the toolset has no `run` |
| **authorization** | may this call happen? | subjects × policy | `file:/etc/passwd` is not in the workspace |
| **approval** | does a person say yes? | spec risk × subjects × policy | `Destructive` and not pre-approved |

Today the harness has only the third, and only as a boolean the caller passes with `--yes`.

---

## 3. The toolset the eval actually needs

| entry | operation | effects | risk | subject |
|---|---|---|---|---|
| `file_read` | `file.read` | `Read`, `Filesystem` | Low | `file:<path>` |
| `dir_list` | `dir.list` | `Read`, `Filesystem` | Low | `file:<path>` |
| `search` | `search` | `Read`, `Filesystem` | Low | `file:<path>` |
| `file_write` | `file.write` | `Write`, `Filesystem` | Medium | `file:<path>` |
| `file_edit` | `file.edit` | `Write`, `Filesystem` | Medium | `file:<path>` |
| `run` | `shell` | `Process` | High | `proc:<program>` — **and it is not `bash`** |

### Amendment: three verbs, and the names are ours everywhere

The entries above were originally `workspace_list`, `workspace_read`, `workspace_grep`,
`workspace_write` and `workspace_edit` — this component's own vendor vocabulary — and each was
published to the model as its own tool. Both halves changed, for one reason.

The evaluation compares four arms across three harnesses that each name their tools differently:
`Bash` here, `run` there, `Write` and `workspace_write` for one act, and a Codex write travelling as
`apply_patch` with the path inside a patch envelope. Everything downstream of a run therefore had to
learn one vendor's vocabulary — and the corpus in `engineering-protocols/conformance/eval/` selects
on tool names, so it was written in Claude Code's and was blind to every other harness. Two patches
tried to widen it and both put *more* vendor names into a document that should hold none.

So the fix moved upstream of the judge:

* **The entries are named by metaharness's neutral operations**, which is the vocabulary both sides
  already share. The `operation` column above is that name, and it is what a reader of a run sees.
* **The model is offered exactly three verbs**, on every harness:

  ```text
  tool_search   {query?, effect?}   -> the tools this run has
  tool_describe {name}              -> one tool's arguments, effects, risk
  tool_invoke   {name, arguments}   -> call it
  ```

`tool_invoke`'s own envelope cannot be honest — what it does depends on the entry it names — so it
declares every effect any entry can have and `Idempotency::Conditional`, and the *subject* a policy
sees is unwrapped from the entry inside it rather than read off the verb. A gate that read the
verb's own arguments would see one opaque blob for every call in the run.

There are two bindings and the model cannot tell them apart. In-process, `harness_tools::Verbs`
implements `ToolPort` and the b10x loop publishes it directly. For a vendor harness,
`metaharness mcp-serve` publishes the same three verbs over the same catalogue on stdio, and the
launch pairs it with `--tools ""` — so an arm that must not have a shell does not have one, rather
than having one denied a turn at a time.

The cost, stated rather than discovered: a model that would have called `file_read` directly now
spends a turn on `tool_describe` first, or guesses the arguments. Whether that shows up in the
measured arms is an experiment, and it is the one the first live run under this surface answers.

### Why `run` and not `bash`

An open shell is unbounded by construction. `sh -c` composes, redirects and substitutes, so **the
subject of the call is not knowable before it runs** — and a subject that cannot be computed cannot
be authorized, which collapses the middle tier of the table above into nothing. flux draws the same
line from the other side: its `bash` is *"an explicit, gated shell — `flux-system` itself never
interprets argv as shell"*.

`run` takes an argv and a program drawn from a **declared set**. Anything outside it is refused by
name. The set is small and per-workflow: a development flow needs `cargo` and `protocol` and nothing
else.

This is not a new idea in this system — it is the driven arm's behaviour, moved from a denial to a
shape. In the live arm-c run, **14 of 45 seam denials were the driver refusing a compound command**
(*"the command composes or redirects, and this run admits one simple invocation at a time"*), and 17
more were read-only orientation tools the model reached for. The seam has been simulating this
design by rejecting; here the tool simply has the shape that makes the rejection unnecessary.

---

## 4. Context management in workflow execution

`harness-flow` today schedules and holds no conversation. That is deliberate — the scheduler tests
with no provider — but the interesting question is what each step *sees*, and the sub-tree design
answers it almost for free.

### Three models, and what each costs

| model | who does it | cost |
|---|---|---|
| one conversation for the whole run | every vendor CLI | cheapest in tokens, worst in isolation: step 9 can be derailed by step 2's dead end |
| one conversation per step | `protocol drive` today — a fresh metaharness session per workflow state | perfect isolation, worst cost: **14.0M tokens against raw's 4.6M for the same deliverable**, six cold sessions |
| **one conversation per sub-tree** | proposed | the boundary already exists |

### Why the third falls out of the DAG

A group is already opaque to its siblings: `implement: needs: [shape]` waits on the whole sub-tree
and *cannot* name a step inside it. The symmetric statement is the context rule — **if a sibling
cannot depend on a step inside a group, it must not see that step's transcript either.** The
boundary that makes a group substitutable is the same boundary that makes it a context scope. One
concept, two consequences.

### The mechanism

- A group owns a **context**: the item list the loop replays each turn.
- **Entering** a group seeds its context from the parent's *handoff*, never from the parent's items.
- A step appends to its group's context; steps in one group share it and stay warm.
- **Leaving** a group emits a handoff and drops its items.
- The handoff is a **declared field in the notation** — `gives:` — so what crosses a boundary is
  written down in the document rather than inferred from whatever the model happened to say last.

```yaml
- group: shape
  needs: [receive]
  gives: [specification_id, task_ids]
  nodes: [...]
```

The cost consequence is the whole point: `shape`'s three steps share one warm context and only the
boundary pays for a cold start. Arm c's 3.4× token multiplier over arm a is six cold sessions, not
six units of work.

### Three problems this does not solve, named rather than left to be discovered

1. **Compaction inside a long group.** A group whose steps exceed the context window needs a
   strategy, and every strategy is lossy. Out of scope here; it is the next design.
2. **A failed step that retries.** The failed attempt **stays** in context. A retry that cannot see
   why it failed repeats it — this is not an optimisation to be reclaimed later.
3. **A handoff is a claim.** It is model-authored prose, so anything a later step must *rely* on
   belongs in the tree as evidence, not in the handoff as a sentence. The handoff carries pointers;
   the tree carries facts.

---

## 5. Build order

1. `ToolSpec` gains effects / risk / idempotency / access, and `ToolPort` gains `subjects()`. No
   behaviour change: the loop begins computing approval from the declaration instead of a flag.
2. `harness-substrate` — a wire client, and a startup probe. Publication becomes a function of
   confinement facts.
3. `workspace_write` and `workspace_edit`, landing in substrate's confined workspace.
4. `run`, over a declared command set.
5. Context scopes and `gives:` in `harness-flow`.

Steps 1 and 5 are independent of substrate and can start immediately. Step 2 is the long pole and
gates 3 and 4.

## 6. Decisions this design does not take

- **Does the harness connect to a substrate daemon the operator runs, or start one?** Recommended:
  connect. Starting a daemon is a deployment decision with a cgroup-delegation prerequisite, and a
  harness that starts its own would need to hold that.
- **Is `run`'s command set declared in the flow document or in a toolset config?** Recommended: the
  flow, because the workflow is the thing that knows which commands a step needs, and a config makes
  one machine's toolset different from another's for the same document.
- **Is the handoff model-authored or computed?** Recommended: model-authored, bounded, and recorded
  in the event stream — with § 4's third caveat standing.
