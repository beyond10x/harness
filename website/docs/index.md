---
slug: /
title: What is Harness?
description: Harness owns the agent loop between a model API and an explicitly bounded set of tools.
---

# One loop, explicit effects

Harness is the beyond10x agent loop. It talks to model APIs directly, assembles each turn, executes
tool round trips, asks for approvals, enforces budgets, and records what happened.

It is deliberately not a wrapper around a vendor's coding-agent binary. If you need to drive and
compare vendor harnesses, that is [metaharness](https://beyond10x.github.io/metaharness/). Harness
owns the loop itself.

```text
user or application
        │
        ▼
  b10x-harness ───────▶ model API
        ▲                   │
        │                   │ tool calls
        └── approval gate ◀─┘
                 │
                 ▼
       workspace tool catalogue
```

## What it owns

Harness keeps five concerns together because they all decide what one run means:

- **Turns.** A stateless conversation is projected onto either the OpenAI Responses or Anthropic
  Messages API.
- **Tools.** The model sees exactly the catalogue this machine can perform.
- **Approvals.** Calls above the run's unattended risk ceiling are decided before the effect.
- **Budgets.** Turn, token, duration, context-window and—when rates are supplied—cost ceilings bind
  the loop.
- **Records.** Sessions preserve a conversation locally; JSONL events expose the live run to another
  program.

## The capability ladder

The command line starts read-only. More consequential tools appear only when a confinement boundary
can provide them.

| Machine boundary | Published catalogue | What the run can do |
|---|---|---|
| No substrate | `file_read`, `dir_list`, `search`, `find` | Inspect one workspace |
| Confined workspace | the read tools plus `file_write`, `file_edit` | Change admitted paths |
| Confined execution | all of the above plus `run` | Start an explicitly allowed argv |

A missing capability is not hidden. `b10x-harness tools` and the first JSON event report a tool that
was requested but withheld, together with the machine fact that withheld it.

## Three ways to use the same loop

| Surface | Use it when |
|---|---|
| `b10x-harness run`, `chat` and `workflow run` | A person, shell script or evaluator owns the process |
| `harness-loop` library | Another Rust component should bind model and tool ports in-process |
| `b10x-harness app-server` | A compatible client owns tools and talks JSON-RPC over stdio |

The library core has no terminal policy. Its default approver is deny-all; each shell must choose its
own interaction policy explicitly.

## Where to go next

Start with [Getting started](./getting-started.md) for a read-only first run. Then read
[The agent loop](./concepts/agent-loop.md) to understand the execution model or
[Tools and approvals](./concepts/tools-and-approvals.md) before enabling effects.

:::info Pre-v1

Harness is tagged `0.11.1` and changing quickly. The provider wires and command-line surface are
pinned by repository contracts, but live-provider and external-bridge characterization is still
limited. See [Status and limitations](./status.md) before adopting it.

The source is publicly readable under `LicenseRef-B10x-Proprietary`. Public visibility is not an
open-source licence or a stability promise. See the repository
[security policy](https://github.com/beyond10x/harness/security/policy) for private vulnerability
reporting.

:::
