---
title: Command-line reference
description: Commands, common option groups, output streams, and exit statuses.
---

# Command-line reference

The installed binary is `b10x-harness`. Its generated `--help` output is the canonical reference for
the exact argv accepted by a particular build:

```bash
b10x-harness --help
b10x-harness run --help
```

The repository pins that generated command-line surface as a versioned contract. This page groups
the options by task rather than copying every help paragraph.

## Commands

| Command | Purpose |
|---|---|
| `run` | Run one request to completion |
| `chat` | Read one line at a time over the same local session |
| `sessions` | List local sessions newest first |
| `tools` | Print the catalogue this machine would publish, without a model call |
| `app-server` | Serve one Codex app-server-compatible JSON-RPC connection over stdio |
| `events` | Convert a Harness JSONL record into `metaharness.event/1` |

## Endpoint and wire

| Option | Meaning |
|---|---|
| `--base-url URL` | Endpoint origin plus API prefix |
| `--model ID` | Exact identifier served by the endpoint |
| `--wire openai-responses\|anthropic-messages` | Provider API projection; defaults to `openai-responses` |
| `--context-window TOKENS` | Request bound and compaction input; defaults to `128000` |
| `--temperature`, `--top-p`, `--reasoning-effort` | Optional sampling values; omitted from the request when unset |

## Credentials

API-key and OAuth sources are mutually exclusive.

| Option | Meaning |
|---|---|
| `--api-key-file PATH` | Read a bearer API key from the named file |
| `--api-key-env NAME` | Read it from the named environment variable |
| `--oauth-token-file PATH` | Re-read a subscription token file per model call |
| `--oauth-token-env NAME` | Read the subscription token from the named variable |
| `--oauth-token-pointer POINTER` | Select a token inside a JSON OAuth source |

Harness has no ambient credential fallback.

## Workspace and tools

| Option | Meaning |
|---|---|
| `--workspace PATH` | Root visible to workspace tools; defaults to `.` |
| `--surface flat\|verbs` | Publish entries directly or under three catalogue verbs |
| `--context FILE` | Preload a file; repeatable |
| `--no-project-instructions` | Omit `AGENTS.md` or `CLAUDE.md` from the standing instruction |
| `--write-scope GLOB=SCOPE` | Restrict matching paths; repeatable, first match wins |
| `--scope-announce stated\|silent` | Tell the model the write restrictions or test the gate silently |

## Confinement

| Option | Meaning |
|---|---|
| `--substrate SOCKET` | Use a substrate daemon |
| `--substrate-embedded` | Hold a local substrate driver in this process |
| `--workspace-id ID` | Select a daemon workspace |
| `--cgroup-root PATH` | Name a delegated cgroup root for embedded execution |
| `--allow-program NAME` | Admit one executable program; repeatable |
| `--toolchain rust` | Mount the Rust toolchain read-only |

See [Confined workspaces](../guides/confinement.md) for prerequisites.

## Approvals

| Option | Meaning |
|---|---|
| `--approve auto\|prompt\|deny\|all` | Who decides a call above the ceiling |
| `--approve-up-to low\|medium\|high\|destructive` | Highest risk that runs without asking |
| `--yes` | Same as `--approve all`; cannot combine with `--approve-up-to` |

## Budgets and accounting

| Option | Meaning |
|---|---|
| `--max-turns N` | Total model-turn ceiling |
| `--max-output-tokens N` | Total reported output-token ceiling |
| `--max-output-tokens-per-turn N` | Maximum offered to one turn |
| `--max-duration-ms N` | Wall-clock ceiling |
| `--prices FILE` | Dated JSON rate card used for cost reporting |
| `--max-cost-microunits N` | Spend ceiling in millionths of a US dollar; requires `--prices` |

## Sessions and output

| Option | Meaning |
|---|---|
| `--session-dir PATH` | Override the local session directory |
| `--resume ID\|latest` | Continue a stored conversation |
| `--no-session` | Write no session file |
| `--json` | Write one JSON event per stdout line |
| `--quiet` | Hide progress from stderr; warnings remain |
| `--output-schema FILE` | Make `run` finish with one structured object |

`--output-schema` belongs only to `run`; an open-ended `chat` conversation has no single final
object.

## Advanced loop features

| Option | Meaning |
|---|---|
| `--delegate` | Publish one fresh-context sub-agent tool |
| `--delegate-turns N` | Cap one delegate; default `20`, minimum `1` |
| `--hooks FILE` | Load explicitly named operator hook programs |

## Standard streams

| Mode | stdout | stderr |
|---|---|---|
| Default | Streamed answer text | Progress, reasoning summary, calls, usage, warnings, session ID |
| `--output-schema` | One compact JSON object on completed success | Model prose and normal progress |
| `--json` | JSONL event stream | Pre-loop failures and session diagnostics where applicable |

Under `--json`, a refusal before the loop starts is one `{"kind":"refused","reason":"..."}` line
and exit status 1.

## Exit status

| Status | Caller action |
|---|---|
| `0` | Consume the completed answer |
| `2` | Inspect the named stop: a budget, cancellation, or unstructured result bound the run |
| `1` | Treat it as a configuration, credential, confinement, transport, or protocol failure |

Clap parse errors are normalized to status 1 by this command because status 2 already means a run
that started and stopped.
