---
title: Use the command line
sidebar_position: 1
description: How the b10x-harness command line is organised, how a run gets its credential, and what its output streams and exit statuses mean.
lede: The CLI reference lists every option. This guide covers what a table cannot show, which is where a credential comes from, what an MCP profile or a skill directory grants, and how a caller reads the result.
source: crates/harness-cli/src/lib.rs; crates/harness-cli/src/provider.rs; crates/harness-cli/src/skills.rs; crates/harness-cli/src/agents.rs; crates/harness-cli/src/memories.rs; crates/harness-mcp/src/lib.rs; b10x-harness --help
---

# Use the command line

The installed binary is `b10x-harness`. `--help` after any command prints its exact argv for the
build you have, and [the CLI reference](../reference/cli.md) is generated from the same definition.
The repository pins that surface as a versioned contract under `contracts/cli/b10x-harness/`. A
version reachable on `main` never changes; a changed surface cuts the next one.

```bash
b10x-harness --help
b10x-harness run --help
```

## Providers and profiles

A **provider** carries the endpoint, wire, model and credential source and grants nothing, so the
collection ships built in. A **profile** carries `write`, the approval ceiling, the allow-list and
the write scope, and nothing of that shape is compiled in. `-p NAME` applies a profile from
`~/.config/b10x/harness.toml`; it is repeatable and a later one wins. A typed flag beats both, and
typing `--base-url` opts out of the provider bundle entirely. The `started` event names every
profile that contributed, with a digest of what it said. See
[Configure a provider and profile](./profiles.md) and the
[Configuration reference](../reference/configuration.md).

## Credentials

API-key and OAuth sources are mutually exclusive.

| Option | Meaning |
|---|---|
| `--api-key-file PATH` | Read a bearer API key from the named file |
| `--api-key-env NAME` | Read it from the named environment variable |
| `--api-key-secret NAMESPACE/NAME` | Read it from the local secrets store on each model call |
| `--oauth-token-file PATH` | Re-read a subscription token file per model call |
| `--oauth-token-env NAME` | Read the subscription token from the named variable |
| `--oauth-token-pointer POINTER` | Select a token inside a JSON OAuth source |

Harness searches no ambient default. A **provider** may default a credential path, which is what
makes `provider = "claude"` enough on its own, and a run that took one reports
`credential_source: "provider:<name>"`, so the default is visible in the record rather than
silent. A credential you name yourself always wins and records its class and source kind, for
example `"api-key:file"`, `"api-key:secrets"` or `"oauth:environment"`, without recording the
path, variable name, secrets-store reference or secret. Naming none records `"none"`.

## MCP servers

`run`, `chat` and `tools` accept the same reviewed outbound surface.

| Option | Meaning |
|---|---|
| `--mcp-profile FILE` | Publish the explicit subset in one Harness authority profile; repeatable |
| `--mcp-registry FILE` | Override the shared `b10x-mcp` registry; requires at least one profile |

The profile pins the registry SHA-256 and the exact frozen `tools/list` snapshot, then supplies
local names, reviewed descriptions, envelopes and subjects. Discovery and server annotations grant
nothing on their own. Preparation refuses digest drift and name collisions before the first model
request, and every admitted call still meets the normal approver, hooks, deadline and result bound.
The standalone `b10x-mcp config check`, `connections check` and `tools snapshot` commands provide
the review inputs.

## Skills, agents and memories

`run` and `tools` take these. Each is repeatable, and a named directory that is not there refuses
the run by name, as `--context` does.

| Option | Meaning |
|---|---|
| `--skills-dir DIR` | Offer every `DIR/<name>/SKILL.md` as a skill: its `description` in the standing instruction, its body only when the model calls the `skill` tool by name |
| `--agents-dir DIR` | Offer every `DIR/<name>.md` as a named agent a `delegate` call may pick; needs `--delegate` |
| `--plugin-dir DIR` | `--skills-dir DIR/skills` plus `--agents-dir DIR/agents`, each name qualified `<plugin>:<name>` from `DIR/.claude-plugin/plugin.json` |
| `--memory-dir DIR` | Offer every `DIR/<id>.md` as a memory: its `summary` in the standing instruction, its body only when the model calls the `recall` tool by id |

A memory record is `<id>.md`, the id being the file's own name, with frontmatter `kind`, `summary`,
`trust`, `status` and an optional `supersedes`, and the body after. `kind` is one of `decision`,
`fact`, `handoff`, `lesson`, `preference`, `runbook`. `trust` has exactly one legal value,
`unreviewed`. `status` is `active`, `rejected` or `superseded`; only `active` records are offered.
[Structured runs, delegates and hooks](./structured-runs.md#offer-memories-on-demand) covers the
rules, and [Security boundary](../concepts/security-boundary.md) why `--memory-dir` is the only
way a memory reaches a run.

The layout is the one Claude Code writes, so a plugin written for it runs here unchanged. The
frontmatter reader takes top-level `key: value` lines and nothing else: a document using a key this
build does not read refuses the run rather than being half-read. An agent's `tools:` list uses the
vendor's names (`Read`, `Grep`, `Glob`, `Bash`, `Write`, `Edit`, `LS`), and a name outside that
table refuses the document, as does `tools: []`. See
[Name an agent](./structured-runs.md#name-an-agent) for what an agent may and may not do.

## Context inspection

`b10x-harness context show` assembles the same typed context layers without starting a model turn.
It prints a body-free provenance manifest by default; `--json` makes that manifest machine-readable,
and `--body` prints the model-facing rendering. Digests, byte counts, timestamps and cache classes
stay out of the model prompt. The prompt carries only semantic kind and trust, plus a source path
where knowing the document's identity can improve the answer.

## Standard streams

| Mode | stdout | stderr |
|---|---|---|
| Default | Streamed answer text | Progress, reasoning summary, calls, usage, warnings, session ID |
| `--output-schema` | One compact JSON object on completed success | Model prose and normal progress |
| `--json` | JSONL event stream | Pre-loop failures and session diagnostics where applicable |

`--output-schema` belongs only to `run`: an open-ended `chat` conversation has no single final
object. Under `--json`, a refusal before the loop starts is one `{"kind":"refused","reason":"..."}`
line and exit status 1.

## Exit status

| Status | Caller action |
|---|---|
| `0` | Consume the completed answer |
| `2` | Inspect the named stop on stderr or in the `finished` event: a budget, deadline, cancellation or a provider that ended a turn early bound the run |
| `1` | Treat it as a configuration, credential, confinement, transport or protocol failure; a structured run whose held turn ended in prose again is one |

Clap parse errors are normalised to status 1 by this command, because status 2 already means a run
that started and stopped.
