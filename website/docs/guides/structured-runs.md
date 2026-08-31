---
title: Structured runs, delegates and hooks
description: Opt into machine-readable answers, skills loaded on demand, fresh-context and named delegation, and operator hooks.
---

# Structured runs, delegates and hooks

These features are opt-in because each changes how the loop can finish or what the model can call.
They share one rule: they do not widen the workspace tool catalogue or bypass its approval gate.

## Structured output

`--output-schema FILE` publishes a loop-owned tool named `answer`. The schema must describe a JSON
object. The model finishes by calling that tool, and its arguments become the answer.

Create `answer.schema.json`:

```json
{
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "summary": {"type": "string"},
    "files": {
      "type": "array",
      "items": {"type": "string"}
    }
  },
  "required": ["summary", "files"]
}
```

Then run:

```bash
b10x-harness run \
  --output-schema answer.schema.json \
  --base-url https://gateway.example/v1 \
  --model model-alias \
  --api-key-env MY_MODEL_TOKEN \
  --workspace . \
  --input "Map the repository" | jq .
```

Without `--json`, stdout contains exactly one compact JSON value when the run completes. Model prose
and progress go to stderr. If the model ends in prose, Harness nudges it once to call `answer` —
**and holds the turn that nudge opens to that tool at the provider**, so the model cannot answer
in prose twice by choice. A second prose ending still stops as `unstructured` with status 2, which
is now a route that ignored a constraint it documents rather than a model that preferred prose.

The constraint is sent on that one turn and no other. Holding every turn to `answer` would be a run
that answers before it does any work, and on one route the field sits outside the prompt cache — so
one turn per run, on a run that was otherwise about to report nothing.

The provider receives the schema as the tool's argument schema, and Harness independently validates
the proposed answer against the same schema. Invalid arguments become a failed tool outcome and the
run continues; an invalid schema refuses the run before the first provider request.

## Offer skills on demand

`--skills-dir DIR` offers every `DIR/<name>/SKILL.md` — YAML frontmatter with `name` and
`description`, the document after — as a skill. Only the descriptions reach the model, one line each
in the standing instruction; the body arrives as the result of a `skill` call the model makes by
name. This loop replays the whole conversation every turn, so a body placed in the instruction is
billed on every turn of every run: `--context` is for the files a run needs throughout, and a skill
library is the other case. The `skill` tool's input is a schema `enum` of the offered names, so a
name this run does not have is refused by the provider before it is sent.

`--plugin-dir DIR` reads `DIR/skills/` and `DIR/agents/` the same way and qualifies each name
`<plugin>:<name>` from `DIR/.claude-plugin/plugin.json`, so two plugins that both ship a `planning`
stay distinguishable. The `started` event and `b10x-harness tools` list the offered names — always,
empty included — because a run given skills and a run given none are different records.

## Delegate one task

`--delegate` publishes another loop-owned tool. The model can hand one self-contained task to a
fresh conversation:

```bash
b10x-harness run \
  --delegate \
  --delegate-turns 12 \
  ...
```

The delegate receives:

- the same standing instruction;
- the same tool port, approver, hooks, cancellation token, and machine boundaries;
- the parent's remaining token, time, and cost budget;
- only the delegated task, not the parent's conversation.

It reports `{stop, turns, text}` once to the parent. Delegation is depth one: a delegate cannot
delegate again. Child events are wrapped as `delegated` so a consumer cannot confuse child text
with the parent answer.

Neighbouring delegate calls may run side by side up to `--delegate-parallel N` (default `4`) only
when every reachable tool is non-mutating and needs no approval and no hook is attached. A mutating
surface, approvals, hooks, a port that cannot fork, or a budget that cannot divide takes the same
delegates through the sequential path in model order.

### Name an agent {#name-an-agent}

`--agents-dir DIR`, or the `agents/` half of `--plugin-dir DIR`, describes delegates in advance in
the on-disk shape Claude Code reads — `DIR/<name>.md`:

```markdown
---
name: reviewer
description: Reads a diff and reports what would break, without editing anything.
tools: Read, Grep, Glob
---
You are reviewing, not fixing. Report findings as file:line and stop.
```

The body is that agent's standing instruction. The model calls `delegate(task, agent)`; the `agent`
argument is a schema `enum` of the offered names, so a name this run does not have is refused before
it is sent, and a run with no agents publishes no `agent` key at all.

An agent narrows and never widens. Its `tools:` is mapped to Harness names and intersected with what
the **parent was admitted**, so a child of an already narrowed run cannot climb back out by naming
an agent, and a file arriving from an unaudited place — a plugin, a checked-out dependency — cannot
grant `run` to a run whose catalogue never published it. What the agent asked for and did not get is
a `withheld` record in the child's own session. The narrowing filters what is published **and**
refuses the call, so a hidden tool is not reachable by name. A file with no `tools:` key is
unrestricted, not disarmed; `tools: []` is refused.

## Run operator hooks

`--hooks FILE` loads a versioned JSON document. Hooks run operator-owned programs at three points in
a run:

| Point | Effect |
|---|---|
| `before-call` | Runs after approval; exit 2 blocks the call |
| `after-call` | Adds a note beside the tool outcome for the model |
| `stop` | Exit 2 asks the loop to continue, at most three times |

Example `hooks.json`:

```json
{
  "version": 1,
  "hooks": [
    {
      "on": "before-call",
      "tools": ["file_write", "file_edit"],
      "command": ["/opt/policy/check-write"]
    },
    {
      "on": "stop",
      "command": ["/opt/policy/check-answer"]
    }
  ]
}
```

Commands receive one JSON document on stdin and return a decision through exit status and optional
JSON on stdout. They are started directly as an argv, never through a shell.

- exit `0` proceeds; an `after-call` hook may return `{"note":"..."}`;
- exit `2` blocks or continues, using `{"reason":"..."}` as the explanation;
- another status, startup failure, timeout, or oversized output is recorded as a failed hook.

A `before-call` failure fails closed. An `after-call` failure is reported as a note because the tool
already ran. A `stop` failure fails open because it cannot retroactively erase an otherwise complete
answer.

A fourth point exists under `workflow run` only, because it is asked at a section boundary and a
single run has none. `transition` is asked before a section is entered and again after it leaves:
exit 2 at the entry skips the section as failed, exit 2 on a clean exit sends the section back for
another attempt, and a hook that cannot answer is read as a refusal at both moments. It is declared
in the same file, under the same rules:

```json
{"on": "transition", "command": ["/opt/policy/protocol-transition"]}
```

See [Workflows](./workflows.md) for the document it receives and what each refusal does to the walk.

:::warning Hooks are not confined

Hook programs run on the operator's host, outside substrate confinement. They are named explicitly
and never discovered from the workspace. See [Security boundary](../concepts/security-boundary.md).

:::
