---
title: Plan and run a workflow
description: Validate a multi-step document, inspect its order, and run it under one bounded Harness invocation.
---

# Plan and run a workflow

A workflow turns a YAML or JSON graph into a bounded walk. A step is one turn; a group is one
conversation and can repeat as a unit.

## Write the smallest useful flow

Save this as `flow.yaml`:

```yaml
id: change
root:
  id: root
  nodes:
    - id: inspect
      run:
        prompt: "Inspect the requested area and state the smallest safe change."
    - id: implement
      needs: [inspect]
      run:
        prompt: "Implement and verify the change."
        scope:
          - ".git/**=denied"
          - "**=allowed"
```

Node ids must be unique among siblings and cannot contain `.`. A node scope can only narrow the
scope supplied by the operator.

## Validate without contacting a model

```bash
b10x-harness workflow plan --flow flow.yaml
```

Planning needs no endpoint, credential, or model. It exits `0` for a valid document and `1` for a
refused one.

## Run under explicit bounds

Use the same connection, confinement, approval, and budget options as a normal run:

```bash
b10x-harness workflow run -p confined-change \
  --flow flow.yaml \
  --input "Add a --since flag to the report command" \
  --workspace . \
  --approve prompt \
  --max-turns 12 \
  --max-duration-ms 180000
```

Here `confined-change` is the profile created in
[Configure a provider and profile](./profiles.md): it selects embedded confinement and can inherit
the default provider. Use `b10x-harness providers show claude` first so the endpoint and credential
behavior are not surprises. Replace the provider configuration with explicit endpoint and
credential flags when you do not want a built-in default.

Each section attempt files its own session unless `--no-session` is set. `--json` interleaves flow
events with loop events, ending with `flow-finished` when the walk finishes or `flow-paused` when
an `operator` step hands control to a person.

## Interpret the result

| Status | Meaning |
|---|---|
| `0` | Every required section came out clean, or the walk is awaiting the operator named by `flow-paused` |
| `2` | The flow finished but a step failed, skipped, exhausted, or was cancelled |
| `1` | Configuration was refused or a loop error aborted a step |

For the full document grammar, command steps, handoffs, repeat rules, transition hooks, event
shapes, session ids, and budget division, see [Workflow reference](../reference/workflows.md).
