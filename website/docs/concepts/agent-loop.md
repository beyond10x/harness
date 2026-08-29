---
title: The agent loop
description: How Harness assembles turns, performs tool round trips, and decides when a run stops.
---

# The agent loop

Harness owns the cycle between one caller, one model endpoint, and one tool port. The endpoint does
not retain the thread: every turn is assembled from the local conversation and sent as a complete
request.

## One run, turn by turn

```text
input
  │
  ▼
assemble request ──▶ stream model turn
                         │
              ┌──────────┴──────────┐
              │                     │
          final text             tool calls
              │                     │
              ▼                     ▼
        stop hooks             approval gate
              │                     │
              │                 tool outcomes
              │                     │
              └──── finish ◀── next turn
```

For each turn, the loop:

1. combines the standing instruction, local conversation, published tool definitions, sampling
   settings, and remaining budget;
2. streams model events as they arrive;
3. collects complete tool calls;
4. resolves each call to the operation it would perform;
5. asks the approver when that operation is above the unattended risk ceiling;
6. executes admitted calls and adds every outcome—including refusals—to the conversation;
7. starts another turn, or returns a typed stop.

A refusal the model should learn from is a failed tool outcome, not a crashed run. If a write was
denied, the model receives that fact and may complete with the read-only work it can still do.

## Stateless at the provider boundary

Harness does not use provider-managed conversations or threading. The full local conversation is
projected onto the selected wire each turn.

This gives the loop one continuation model across both provider APIs, but it has a cost: prior items
are sent again. `--context-window` bounds request construction and drives compaction. At 80% of the
declared window, the loop first elides old tool-result payloads and can spend a model turn to
summarize older items, targeting 50%.

Opaque reasoning items are stored verbatim and tagged with the wire that produced them. Harness does
not reinterpret those bytes, and refuses to replay them through another wire.

## Events are the observation seam

Every shell reads the same vendor-neutral loop events: turn starts, streamed text, tool requests,
approval decisions, usage, costs, warnings, compaction, delegates, hooks, and the final stop.

The terminal renderer turns those events into human-readable stdout and stderr. `--json` serializes
them as JSON Lines for another program. This keeps observability separate from model-provider event
formats.

See [Sessions and events](../guides/sessions-and-events.md) for the machine-readable contract and
the important rule for retried streams.

## Budgets bind between effects

Harness can enforce:

- total model turns;
- total reported output tokens;
- output tokens offered to any one turn;
- wall-clock duration, including the time offered to an admitted `run` tool;
- a model context window;
- total spend, when a dated rate card prices the selected model.

Provider-reported usage remains absent when the provider reports none. Cost remains absent without a
rate. Harness never turns “unknown” into zero.

When a budget binds, the run returns a `LoopStop`; it is an ordinary outcome of work that did run.
Configuration, transport, and protocol failures return a `LoopError` because the run could not
proceed.

## One core, three shells

The core loop depends on two ports:

- a `ModelPort` that executes one projected model turn;
- a `ToolPort` that describes and executes the available operations.

The command-line shell binds these ports to HTTP and workspace tools. The app-server shell binds
tools supplied by its client over JSON-RPC. A Rust embedder can bind both in-process. The loop does
not need to know which shell it is running under.
