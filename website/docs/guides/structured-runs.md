---
title: Structured runs, delegates and hooks
description: Opt into machine-readable answers, fresh-context delegation, and operator hooks.
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
and progress go to stderr. If the model ends in prose, Harness nudges it once to call `answer`; a
second prose ending stops as `unstructured` with status 2.

The provider receives the schema as the tool's argument schema. Harness currently does not perform
an independent JSON Schema validation pass after the call. Treat that as a current limitation when
the result crosses a trust boundary.

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
