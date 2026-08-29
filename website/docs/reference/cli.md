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

The repository pins that generated command-line surface as a versioned contract; the current pin is
`contracts/cli/b10x-harness/2026-08-29.3`. A released version — one reachable on `origin/main` —
never changes, and a changed surface cuts the next one. This page groups the options by task rather
than copying every help paragraph.

## Commands

| Command | Purpose |
|---|---|
| `run` | Run one request to completion |
| `chat` | Read one line at a time over the same local session |
| `workflow plan` | Validate a workflow document and print what runs in what order, without an endpoint |
| `workflow run` | Walk a workflow document: one turn per step, one session per section |
| `sessions` | List local sessions newest first |
| `tools` | Print the catalogue, skills and agents this machine would publish, without a model call |
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

## Skills and agents

Both `run` and `tools` take these. Each is repeatable, and a named directory that is not there
refuses the run by name, as `--context` does.

| Option | Meaning |
|---|---|
| `--skills-dir DIR` | Offer every `DIR/<name>/SKILL.md` as a skill: its `description` in the standing instruction, its body only when the model calls the `skill` tool by name |
| `--agents-dir DIR` | Offer every `DIR/<name>.md` as a named agent a `delegate` call may pick; needs `--delegate` |
| `--plugin-dir DIR` | `--skills-dir DIR/skills` plus `--agents-dir DIR/agents`, each name qualified `<plugin>:<name>` from `DIR/.claude-plugin/plugin.json` |

The layout is the one Claude Code writes, so a plugin written for it runs here unchanged. The
frontmatter reader takes top-level `key: value` lines and nothing else: a document using a key this
build does not read refuses the run rather than being half-read. An agent's `tools:` list uses the
vendor's names — `Read`, `Grep`, `Glob`, `Bash`, `Write`, `Edit`, `LS` — and a name outside that
table refuses the document, as does `tools: []`. See
[Structured runs, delegates and hooks](../guides/structured-runs.md#name-an-agent) for what an agent
may and may not do.

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

`--output-schema` belongs only to `run`: an open-ended `chat` conversation has no single final
object, and `workflow run` derives one schema per step for itself.

## Advanced loop features

| Option | Meaning |
|---|---|
| `--delegate` | Publish one fresh-context sub-agent tool; with agents offered, a call may name one |
| `--delegate-turns N` | Cap one delegate; default `20`, minimum `1` |
| `--hooks FILE` | Load explicitly named operator hook programs |

Under `workflow run`, `--hooks` also accepts `on: "transition"`, asked before a section is entered
and after it leaves. See [Workflows](../guides/workflows.md).

## Workflows

| Option | Meaning |
|---|---|
| `--flow FILE` | The workflow document; YAML or JSON, decided by extension |
| `--input TEXT` | The task, given to every step beside its own prompt — the same word `run` uses |
| `--max-attempts N` | Override every `repeat.max` in the document, the root's included; absent means the document's own bounds |

`workflow plan` accepts `--flow` and `--max-attempts` only, and contacts no endpoint. `workflow run`
takes the option groups above — endpoint, credentials, workspace, confinement, approvals, budgets,
sessions — and refuses `--resume` by name, because a flow names its own sessions. `--output-schema`
is not a flag of `workflow run` at all: the runner derives the schema each step answers under, so
typing it is an unrecognised argument rather than a refusal. Step budgets
(`--max-turns`, `--max-output-tokens`, `--max-output-tokens-per-turn`) bound one step;
`--max-cost-microunits` and `--max-duration-ms` bound the whole flow.

## Standard streams

| Mode | stdout | stderr |
|---|---|---|
| Default | Streamed answer text | Progress, reasoning summary, calls, usage, warnings, session ID |
| `--output-schema` | One compact JSON object on completed success | Model prose and normal progress |
| `--json` | JSONL event stream | Pre-loop failures and session diagnostics where applicable |

Under `--json`, a refusal before the loop starts is one `{"kind":"refused","reason":"..."}` line
and exit status 1.

## Exit status

| Status | Caller action | Under `workflow run` |
|---|---|---|
| `0` | Consume the completed answer | The flow came out clean |
| `2` | Inspect the named stop: a budget, cancellation, or unstructured result bound the run | The flow finished and did not come out clean: a failed step, a skipped or exhausted section, or a cancelled run. Inspect `flow-finished` |
| `1` | Treat it as a configuration, credential, confinement, transport, or protocol failure | Refused before the flow started, or aborted mid-step on a loop error |

`workflow plan` exits `0` when the document validates and `1` when it does not.

Clap parse errors are normalized to status 1 by this command because status 2 already means a run
that started and stopped.
