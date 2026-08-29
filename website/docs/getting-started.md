---
title: Getting started
description: Build Harness and make a first read-only run against a model endpoint.
---

# Getting started

This path builds Harness from a source checkout and makes one read-only run. It changes no file and
starts no process.

## Before you begin

You need:

- Rust 1.97 or newer;
- a model endpoint that serves the OpenAI Responses or Anthropic Messages API;
- the exact model identifier that endpoint expects;
- a bearer credential, if the endpoint requires one.

There is no published crate or prebuilt binary yet. From the repository root, build the command:

```bash
cargo build --release --locked -p b10x-harness-cli
./target/release/b10x-harness --version
```

The rest of this guide uses `b10x-harness` as shorthand for
`./target/release/b10x-harness`.

## Inspect the toolset first

Ask the command what a default run would publish:

```bash
b10x-harness tools --workspace .
```

Without a named substrate boundary, the answer contains four read-only entries: `file_read`,
`dir_list`, `search`, and `find`. This command contacts no model endpoint.

## Name the credential source

Harness does not search ambient default variables or vendor configuration directories. Point it at
the source you intend it to read:

```bash
export MY_MODEL_TOKEN='replace-me'
```

Then pass `--api-key-env MY_MODEL_TOKEN`. A file is equally explicit:
`--api-key-file /path/to/token`.

:::warning Shell history and environment

Do not put a real token directly in the command line. An environment variable avoids the shell's
argument history, but it may still be visible to processes with permission to inspect your
environment. Use the credential mechanism appropriate to your machine.

:::

## Make the first run

```bash
b10x-harness run \
  --base-url https://gateway.example/v1 \
  --model model-alias \
  --api-key-env MY_MODEL_TOKEN \
  --workspace . \
  --input "Map this repository. Name the evidence behind each claim."
```

`--base-url` is the endpoint origin plus API prefix. With the default `openai-responses` wire,
Harness sends turns to `POST {base-url}/responses`.

To use the other wire:

```bash
b10x-harness run \
  --wire anthropic-messages \
  --base-url https://gateway.example/v1 \
  --model model-alias \
  --api-key-env MY_MODEL_TOKEN \
  --workspace . \
  --input "Which files define the public command line?"
```

That wire sends turns to `POST {base-url}/messages`. A session cannot be resumed on a different
wire because it may contain opaque provider items.

## Read the output

For a normal run:

- stdout is the model's answer;
- stderr carries progress, tool calls, approvals, usage, cost, warnings, and the session ID;
- the default session directory is `$XDG_STATE_HOME/b10x-harness/sessions`, falling back to
  `$HOME/.local/state/b10x-harness/sessions`.

Exit status distinguishes three outcomes:

| Status | Meaning |
|---|---|
| `0` | The model completed the run |
| `2` | A run happened, then stopped for a named bound or outcome |
| `1` | The harness could not start or continue the run |

Use `--no-session` when the run must leave no transcript on the machine. Use `--json` when another
program should consume the event stream rather than prose.

## Put a ceiling on the run

Start automation with explicit bounds:

```bash
b10x-harness run \
  --base-url https://gateway.example/v1 \
  --model model-alias \
  --api-key-env MY_MODEL_TOKEN \
  --workspace . \
  --max-turns 12 \
  --max-output-tokens 12000 \
  --max-duration-ms 180000 \
  --input "Explain the build and test path."
```

A cost ceiling also needs a dated `--prices` rate card. Harness refuses an unmeasurable spend limit
instead of accepting one it cannot enforce.

## Continue from here

- [Tools and approvals](./concepts/tools-and-approvals.md) explains what changes when effects are
  enabled.
- [Sessions and events](./guides/sessions-and-events.md) covers resume, chat, JSONL, and retry
  semantics.
- [Confined workspaces](./guides/confinement.md) is the prerequisite for file writes and process
  execution.
- [Command-line reference](./reference/cli.md) maps the commands and option groups.
