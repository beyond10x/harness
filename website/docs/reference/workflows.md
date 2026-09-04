---
title: Workflow reference
description: Workflow documents, scopes, derived output, events, sessions, hooks, and exit statuses.
---

# Workflow reference

`workflow` walks a document instead of answering one request. The loop holds the whole graph, so the
steps of a section share one conversation, a section that did not come out clean is re-entered from
what crossed into it, and an operator program can refuse either side of a section boundary. No
vendor harness and no external sequencer is involved.

The notation is the one `harness-flow` already carries. A node is a **step** (one thing that runs)
or a **group** (a sub-tree with its own nodes and its own edges), an edge may only join siblings,
and a group is the unit of scope, of reuse and of reporting.

## The document

```yaml
id: development
root:
  id: root
  nodes:
    - id: receive
    - id: shape
      needs: [receive]
      nodes:
        - id: specify
        - id: decompose
          needs: [specify]
    - id: implement
      needs: [shape]
```

`implement` needs the whole `shape` sub-tree, not one of its steps: a group is opaque to its
siblings, which is what makes it substitutable.

An `id` must be unique among its siblings and may not contain a `.`, because that is the character a
path joins names with — `root.shape.specify`. A document that declares one is refused by name.

A group may add `repeat: {max: N}` — run this section again while it does not come out clean — and
`gives`, the list of names it promises to hand its siblings when it leaves. Nothing else crosses a
group boundary.

Each step carries a `run` payload the notation never reads. Harness reads these keys of it, all
optional:

| Key | Used as |
|---|---|
| `kind` | Absent or `llm`: one model turn; `command`: one gated `run` call; `operator`: pause and hand its prompt to a person |
| `state` | The step's name in prose and in the record |
| `summary` | The step's prompt when `prompt` is absent |
| `prompt` | The step's prompt |
| `context` | File paths **inside** the workspace, read like `--context` and named in the step input. A name that is absolute, that resolves outside the workspace, or that is not there fails the step by name — a `warning` with `code: "context-refused"` naming the path and the workspace, no model call — and the walk skips what needed it |
| `scope` | Where **this step** may write: a list of `<glob>=<allowed\|partial-only\|denied>`, the same grammar `--write-scope` takes. See [A step runs under its node's scope](#a-step-runs-under-its-nodes-scope) |

Documents are read as YAML (`.yaml`, `.yml`) or JSON (`.json`), decided by the extension. Any other
extension is refused by name.

The kind vocabulary is closed. An unknown word, a non-string kind, a malformed command, or an
operator step without a non-empty prompt is refused by `workflow plan`; none silently falls
through to a model turn.

## Plan a document without an endpoint

```bash
b10x-harness workflow plan --flow flow.yaml
```

`plan` takes `--flow` and `--max-attempts` and nothing else: no base URL, no credential, no model,
no socket. It answers *does this document validate, and what runs in what order* — one line per
layer, indented per group, with the repeat bound beside the group. Under `--json` it prints the plan
itself. Exit `0` when the document validates, `1` when it does not.

## Run one

```bash
b10x-harness workflow run \
  --flow flow.yaml \
  --input "Add a --since flag to the report command" \
  --base-url https://gateway.example/v1 \
  --model model-alias \
  --api-key-env MY_MODEL_TOKEN \
  --workspace .
```

| Option | Meaning |
|---|---|
| `--flow FILE` | The document. Refused by name when it does not validate, before any session is written |
| `--input TEXT` | The task, given to every step beside its own prompt |
| `--max-attempts N` | Overrides every `repeat.max` in the document, the root's included, for a document that carries none. Absent means the document's own bounds |

`workflow run` takes the same option groups `run` and `chat` take — endpoint, credentials,
workspace, confinement, approvals, budgets, sessions. `--resume` is refused by name, because a flow
names its own sessions (see [Sessions](#sessions-and-ids)).

`--output-schema` is **not a flag of `workflow run`**: the runner derives the schema each step
answers under (see below), so there is nothing for a file to shape. Typing it is an unrecognised
argument, answered by the command line parser before the run starts, not a refusal worded here.

Budgets divide the way they already do elsewhere: `--max-turns`, `--max-output-tokens` and
`--max-output-tokens-per-turn` bound **each step**, while `--max-cost-microunits` and
`--max-duration-ms` bound the **whole flow**: the runner keeps the cumulative spend and the flow's
start instant, and hands each step a budget derived from what remains. A step that would start
with nothing left does not call the model — it fails with a `warning` (`code: "flow-budget"`)
naming the ceiling and the spend, and the walk skips what needed it.

Tools and approvals are the run's, built once for every step; a published toolset per group is not
in this milestone. The **write scope is not** — a step runs under the one its own node declares.

### A step runs under its node's scope

A step's `run.scope` says where that step may write. It is a list of
`<glob>=<allowed|partial-only|denied>` — the grammar `--write-scope` takes, and the one
`aep govern workflow flow --map` emits into the node it projects:

```yaml
- id: implement-1
  run:
    state: implement
    prompt: "Make the change."
    scope:
      - ".internal/**=denied"
      - "**=allowed"
```

The node's rules are laid over the run's for the length of that step and taken off again however
the step ends, so the next step starts under the run's scope alone. A write the node denies is
**refused in the tool, before it happens** — the call comes back as a failed result the model is
told about, the walk carries on, and nothing reaches disk. It is not an audit afterwards.

Three things follow, and each of them is deliberate:

- **A node can only narrow.** Both scopes are asked and the first refusal wins, so a node that says
  `allowed` where `--write-scope` says `denied` changes nothing. The command line is the operator's
  own sentence about where a run may write, and a projected document — generated by another
  component — cannot raise it.
- **A node that declares no `scope` runs under the run's**, exactly as every step did before. A
  document that says nothing does not silently narrow a run.
- **The order is the rule.** First match wins, in the order the list is written; nothing sorts it.
  `[".internal/**=denied", "**=allowed"]` and `["**=allowed", ".internal/**=denied"]` are two
  different scopes.

A step whose node declares a scope is told it beside its prompt, in the same words the standing
instruction uses for the run's own, so no turn is spent discovering the rule by being refused.

A `scope` this build cannot read — not a list, holding something that is not a string, or naming a
word that is not one of the three — refuses the **document**, by node path, before any session is
written; `workflow plan` refuses it for free with the same words. It never falls through to the
run's scope: a document that states a boundary and a walk that quietly ran without it is the
failure this key exists to prevent.

### What one step sees

A step is one turn of the loop. Its input is one user message, composed in this order:

1. the flow's `--input`;
2. *"You are in step `<path>`, attempt `<n>` of section `<scope>`"*;
3. the handoffs available to this scope, as *"Earlier sections established:"* and one `name: value`
   line each;
4. the step's own prompt.

Nothing else crosses. Steps in the same group and the same attempt continue one conversation and
stay warm; a step in another group starts from the handoffs above and nothing else, because a
sibling cannot depend on a step inside a group and therefore must not see that step's transcript
either. A retreat — `Repeat` — re-enters the whole section from the same handoffs, which is what
*the whole scope re-runs* means.

### A `command` step is a call, not a turn

A step whose `run` says `kind: command` names a program the document runs — the projection's
verifiers — and the model is never asked:

```yaml
- id: verify-2
  needs: [verify-1]
  run:
    state: verify
    kind: command
    command: [cargo, test, --workspace]
```

The argv becomes one `run` call made through the same gate a model's call meets, in the same
order: published or withheld, the approver, the operator's `before-call` hook, the tool, the
`after-call` hook. It is recorded as a model's call would be — `tool-requested`, `tool-completed`,
a `warning` naming any refusal between them — and filed into the section's conversation as the
call and its result, so the next step in the scope reads what the suite printed where it would
have read a tool's answer. Exit `0` is a passed step. A non-zero exit, a timeout, a program this
run does not publish, a person's *no* or a hook's block is a failed step, and a `step-command`
warning says which. A `command` step whose `command` is missing, empty or not a list of strings is
an error and not a turn: a document that meant to run a verifier is never quietly asked a model
about it instead.

### An `operator` step is a handoff, not a turn

```yaml
- id: review
  run:
    kind: operator
    prompt: Review the change and record whether it is accepted.
```

Reaching this step emits one terminal `flow-paused` carrying the flow, path, prompt and tallies,
then exits `0`. The operator step is counted as `reached`, not failed; it has no `step-finished`.
The pending tail is not called skipped, and no open group is left, repeated or handed off. The
step returns before scope narrowing or a budget check and reaches no tool, approval, call hook or
provider. It writes no session because there was no conversation. `--resume` still refuses: a flow
cursor that can continue the pending tail is separate work, so the pause is an honest handoff and
not a claim that the workflow completed.

### How a step reports: the derived schema

Every step runs under an output schema the runner derives. The model never sees a schema file, and
`workflow run` has no `--output-schema` for one to compete with it:

```json
{ "type": "object", "required": ["outcome"],
  "properties": {
    "outcome": { "enum": ["passed", "failed"] },
    "note":    { "type": "string" },
    "gives":   { "type": "object", "properties": { "<each name the enclosing group gives>": {} } } } }
```

The step finishes by calling `answer`, exactly as a structured run does — the standing instruction
names the tool, as it does for `run --output-schema`. `gives` is collected per scope, last write
wins, and handed over when the group leaves; a group that promised a name and never answered with
it fails as `handoff-incomplete`, by the notation's own rule, and fails **once**: a broken promise
is not retried whatever `repeat.max` says, because a second attempt buys the same answer again. A
step whose group gives nothing still answers `outcome`.

**Only a section that came out clean hands anything on.** What a failed one produced is in its own
record — `group-left` says what it had — but it is a result nobody accepted, whether its own steps
failed or a governor declined its leave, so the sections after it start without it.

| The loop said | The step is |
|---|---|
| `completed`, with an `answer` of `outcome: passed` | Passed |
| `completed` with `outcome: failed`, or `unstructured` | Failed |
| A budget stop — turns, output tokens, input tokens, cost, deadline, or an incomplete provider turn | Failed, with the reason in the record |
| `cancelled` | The flow stops. What ran is filed, no `flow-finished` is emitted, status 2 |
| A loop error — a wire, credential, or protocol failure | The flow aborts, status 1. A broken wire is nobody's failed step, and a walk that recorded a network blip as a failure would misreport the plan |

## Gate a section boundary: the `transition` hook

`--hooks FILE` learns a fourth point beside `before-call`, `after-call` and `stop`. It is asked
twice per section attempt: before the group is entered, and after it leaves. The rules are the
other three points' rules — declared in the named file, never discovered in the workspace, an argv
and never a shell, the credential removed from the environment, the same timeout and output bound.

```json
{
  "version": 1,
  "hooks": [
    {"on": "transition", "command": ["/opt/policy/protocol-transition"]}
  ]
}
```

The program receives one JSON document on stdin:

```json
{ "hook": "transition", "flow": "adp/default", "path": "root.implement-to-review",
  "moment": "enter" | "leave", "attempt": 2, "of": 3,
  "failed": false,                  // leave only
  "handoff": { "specification_id": "…" },   // leave only
  "workspace": "/abs/path" }
```

Exit `0` proceeds. Exit `2` refuses, with `{"reason":"..."}` on stdout, or stderr if there is no
JSON. Any other status, a startup failure, a timeout or oversized output is a failed hook and is
**read as a refusal at both moments** — a governor that could not answer did not say yes, exactly
as `before-call` fails closed.

| Moment | A refusal means | What the walk does |
|---|---|---|
| `enter` a group | This section may not run now | The group is skipped as failed: `node-skipped` for every step inside it, `group-left` with `failed`, and whatever needed the group is skipped too |
| `leave` a group that came out clean | The governor does not accept the section's result | The attempt is marked failed. With attempts left the group is re-entered — this is how a governor forces a retreat. At the bound, `group-left` reports it exhausted |
| `leave` a group that already failed | Nothing changes | It has failed already. The refusal is recorded and that is all |

`leave` is asked after the handoff is collected, once per attempt, so the governor sees what the
section is handing over. On an attempt that **failed and still has one left**, `handoff` is `{}`:
the section is going round again and is never asked what it hands over, so a governor must not read
that empty object as *the section produced nothing*. The last attempt is asked either way. The root is a group and is gated like one: a refused `enter` of the root
runs nothing and exits 2.

A file naming `transition` is refused by a build that does not know the point, rather than being
ignored — an older binary says so instead of running unguarded.

:::warning Hooks are not confined

A transition program runs on the operator's host, outside substrate confinement, like every other
hook. See [Security boundary](../concepts/security-boundary.md).

:::

## Events

Under `--json`, flow events share the stdout stream with the loop's events, one JSON object per
line, each with a kebab-case `kind`:

| Kind | Emitted when |
|---|---|
| `flow-started` | The walk began; carries the document id and how many steps it holds |
| `group-entered` | A section started; carries its path, its layers, and `attempt` of `of` |
| `layer-ready` | A set of siblings became runnable together |
| `step-started`, `step-finished` | One step; a step's own loop events appear between the two |
| `node-skipped` | A node did not run: something it needs failed, or a governor refused the `enter` of the section holding it — `because` says which |
| `group-repeating` | A section did not come out clean and is being re-entered |
| `handoff-incomplete` | A group promised names in `gives` and did not hand them over |
| `group-left` | A section ended; carries `failed`, what it gave, attempts used, and `exhausted` |
| `transition-refused` | A `transition` hook refused, with `path`, `moment`, `attempt` and `reason`. Emitted before the consequence |
| `flow-finished` | The walk ended; carries `ran`, `failed`, `skipped`, `retreats` and `clean` |
| `flow-paused` | The walk reached an `operator` step; terminal, with `flow`, `path`, `reason`, `reached`, `failed`, `skipped` and `retreats` |

A `hook-ran` event records each transition hook with `point: "transition"`, as it does for the other
points. On stderr the same events render one line each in the run's own style:

```text
flow ▸ root.shape (attempt 1 of 3)
step ✓ root.shape.specify
step ✗ root.implement-to-review.verify
retreat ↺ root.implement-to-review (2 of 3): <reason>
```

`b10x-harness events` does not yet project these into `metaharness.event/1` families; a kind it does
not know crosses as `opaque`.

## Sessions and ids

One session is filed per `(scope, attempt)` — per section, per attempt. The id is the flow-run id,
allocated once for the walk, followed by **every open section on the way down, each with the attempt
it is on**:

```text
18d06ae681443bb4-0019ed82.root.2.implement-to-review.3.verify.1
```

That reads as: the first attempt of `verify`, inside the third attempt of `implement-to-review`,
inside the second attempt of the root. Naming a session after its own attempt alone is not enough —
when an ancestor is re-entered, everything under it runs its attempt 1 again, and two conversations
would be one file. A section name may therefore not contain a `.`; the document is refused by name
if one does, because a path is built out of them.

Each session is saved as its scope closes, with what that scope cost, in the usual directory;
`--session-dir` is honoured and `--no-session` writes nothing at all. A walk leaves as many files as
it ran sections, so `b10x-harness sessions` lists every attempt that happened, retreats included.

Under `--json` a `{"kind":"session", …}` line is emitted as each scope's session is filed — the same
shape `run` prints last — and `flow-finished` is the last line of a finished flow. `flow-paused` is
the last line of a walk awaiting a person.

`--resume` is refused. A crashed flow leaves its per-scope sessions and its record on disk, but
nothing turns them back into a cursor yet.

## Exit status

| Status | Meaning |
|---|---|
| `0` | The flow came out clean, or is awaiting the operator named by `flow-paused` |
| `2` | It finished and did not: a step failed, a section was skipped or exhausted, or the run was cancelled. Inspect `flow-finished` |
| `1` | Refused before it started — a document that does not validate, a refused flag, a credential — or aborted mid-step on a loop error |

`workflow plan` exits `0` when the document validates and `1` when it does not.

## Not yet

The first milestone deliberately leaves these out:

- **Resuming a flow.** The per-scope sessions and the record survive a crash; there is no flow
  cursor to restart from, and `workflow run --resume` is refused rather than approximated.
- **A published toolset per group.** The catalogue's **entries** are built once for the run — the
  write scope is not, and a step runs under the one its own node declares (above). Narrowing which
  *tools* a section may reach is the `before-call` hook's job today.
- **Parallel layers.** `plan` says what may run beside what; the walk is sequential, and the loop
  holds one client. A measurement comes before a client per thread.
- **A published toolset per `command` step.** A command runs under the run's own toolset and
  approvals; there is no per-step `--allow-program`, so a verifier the run does not publish is a
  refused step rather than an admitted one.
- **The `metaharness.event/1` projection** of flow events, and their listing as control-plane events.
- **Live evidence.** Everything here is `provider_emulated`: the walk, the retreat and the refused
  transitions are proved against the local deterministic endpoint, never against a real provider.
