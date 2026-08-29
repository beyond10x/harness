---
title: Sessions and events
description: Resume conversations, use chat, and consume the JSONL run record correctly.
---

# Sessions and events

Sessions preserve the conversation between invocations. Events expose a run while it is happening.
They answer different needs and can be used independently.

## Sessions

Every `run` writes a session unless `--no-session` is set. It is saved after success, a named stop,
or a loop error: a run that fails on turn 20 is the run whose first 19 turns are most useful to keep.

List sessions newest first:

```bash
b10x-harness sessions
```

Continue the newest one:

```bash
b10x-harness run \
  --resume latest \
  --base-url https://gateway.example/v1 \
  --model model-alias \
  --api-key-env MY_MODEL_TOKEN \
  --input "Now identify the riskiest assumption."
```

You can pass an exact session ID instead of `latest`, or select another directory with
`--session-dir` on both commands.

A session records:

- the selected wire, model, base URL, and workspace;
- the conversation items, including opaque provider items;
- reported usage, accumulated turns, and measured cost;
- the most recent structured answer, when there is one.

It records neither credentials nor the standing instruction. The instruction is rebuilt from the
current invocation, catalogue, environment, write scope, and project files.

:::warning Resume must stay on one wire

Opaque provider items are replayed verbatim. A session produced by `openai-responses` is refused by
`anthropic-messages`, and vice versa. Start a new session to change wires.

:::

## Chat

`chat` reads one line at a time and keeps one session:

```bash
b10x-harness chat \
  --base-url https://gateway.example/v1 \
  --model model-alias \
  --api-key-env MY_MODEL_TOKEN \
  --workspace .
```

Each input line is a follow-up. Enter `exit` or close stdin to finish. The command intentionally
provides no line editing or terminal history.

## JSON Lines event stream

Pass `--json` when a process should consume events:

```bash
b10x-harness run \
  --json \
  --base-url https://gateway.example/v1 \
  --model model-alias \
  --api-key-env MY_MODEL_TOKEN \
  --workspace . \
  --input "Summarize the workspace" > run.jsonl
```

Each line is one object with a kebab-case `kind`. The main event groups are:

| Group | Event kinds |
|---|---|
| Lifecycle | `credential-renewed`, `started`, `turn-started`, `finished` |
| Streaming | `text-delta`, `reasoning-delta`, `tool-arguments-delta` |
| Tools | `tool-requested`, `approval-required`, `approval-resolved`, `tool-completed` |
| Accounting | `usage`, `rates`, `cost` |
| Recovery | `turn-retried`, `compacted`, `warning` |
| Advanced | `answered`, `delegate-started`, `delegated`, `delegate-finished`, `hook-ran` |

The `started` event names the model, tools published to it, neutral operations, any requested tool
withheld by the machine, and the `skills` and `agents` offered — the last three written even when
empty, so *this run had none* and *this build does not say* are different records. The `finished` event carries the typed stop and total model turns.

`credential-renewed` arrives **before** `started`, and only on a run that renewed something: the
token had gone stale, it was renewed, and the file its owner keeps it in was rewritten — all before
the first request. It names the file, the provider whose renewal was used, when the new credential
runs out, whether the refresh token on disk was retired, and whether every byte the rewrite did not
have to change survived. It carries **no part of the credential** — not a prefix, not a length, not
a digest — so this stream stays a thing you can forward to explain a run.

```json
{"kind":"credential-renewed","source":"/home/you/.codex/auth.json","provider":"codex",
 "expires_unix":1788871151,"refresh_token_rotated":true,"byte_preserving":true}
```

Unlike the always-written lists on `started`, this has no empty form: it is an **act**, and a run
that renewed nothing emits nothing here.

### Warning codes

A `warning` event carries a `code` and a `message`. The code is the stable half: match on it rather
than on the words, which are written for a person.

| Code | What happened |
|---|---|
| `unpublished-tool` | The model called a tool this run never published. Nothing ran. |
| `unpublished-tool-routed` | Under `--surface verbs`, the model called a catalogue entry by its bare name. The call was routed to that entry under the same gate; the wasted spelling is recorded. |
| `program-refused` | `run` was asked for a program outside the set this run declared. Nothing ran. The message names the program and the declared set. |
| `conversation-compacted` | The conversation passed its bound and old tool results were elided. A `compacted` event beside it carries the figures. |
| `summary-failed` | A compaction's summary turn failed or answered with no text. The conversation keeps its elided form and the run goes on. |
| `answer-nudged` | Under `--output-schema` the model ended in prose, so it was told once more to call the answer tool — and the turn that follows is held to it at the provider. |
| `batch-miscounted` | A tool port answered a batch with the wrong number of outcomes, so the loop re-ran the calls one at a time. |
| `unpriced-model` | The rate card does not price this model, so the run reports no cost at all. The message lists what the card does price. |
| `hook-failed` | The stop hook could not decide, so the run ends rather than being kept alive by a hook that crashed. |
| `stop-hook-exhausted` | A stop hook blocked the end of the run up to its limit; the run ends anyway. |

Provider wires forward their own codes through the same event — `unknown-stream-event` for a stream
item the wire does not model, `turn-retried` for a stream that broke and is being attempted again.
An unmodelled item is warned about and preserved, never dropped.

`program-refused` is what makes *the surface refused what is outside it* countable. The refusal is
also a failed tool result — the model has to learn the effect did not happen — but on its own that
is the same shape as a compile error, so the warning is what tells a rule from a fault.

### Retried streams invalidate earlier deltas

When a turn's stream breaks after output has been observed, Harness can retry the unchanged turn. A
`turn-retried` event means all text and reasoning deltas already emitted for that turn attempt must
be discarded. A terminal cannot erase printed text, so its renderer warns the person explicitly.

### Structured answers under `--json`

With `--output-schema`, JSON mode remains an event stream and does not print a separate bare answer.
A stop hook can withdraw one answer and send the loop back to work, so consumers must take the last
`answered` event before a `finished` event whose stop is `completed`.

## Evaluation record

The `events` command converts a saved Harness JSONL record into the `metaharness.event/1` stream used
to compare evaluation arms:

```bash
b10x-harness events --in run.jsonl --out evaluation.jsonl
```

It converts observations only. It does not add the per-call control seam used to drive vendor
harnesses. An `approval-resolved` denial therefore crosses as a `warning` with code
`approval-denied` rather than as a seam decision: the approver is the run's own gate, inside the
harness, and nothing on the metaharness side decided the call. An approval that was granted emits
nothing — the request and its result already say the call proceeded.
