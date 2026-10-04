---
title: First read-only run
description: Build Harness, inspect what it can do, and make one bounded read-only run.
---

# First read-only run

Start by inspecting a workspace without giving the model write or execution tools. This tutorial
gets a binary, checks its catalogue, then makes one bounded run. Only the final step contacts your
model endpoint.

## Prerequisites

You need Linux and access to an endpoint serving either the OpenAI Responses or Anthropic Messages
API. Choose an endpoint and credential source you are authorized to use.

The [release page](https://github.com/beyond10x/harness/releases) is the place to check for packaged
binaries and checksums. The repository's release workflow targets Linux x86_64 and aarch64; macOS
is not a build target because the embedded confinement host uses Linux namespaces.

To build from source, install Rust 1.97 or newer and arrange Git access to the pinned foundation
dependencies before building. Public source visibility alone does not grant access to every
dependency. From the repository root:

```bash
cargo build --release --locked -p b10x-harness-cli
./target/release/b10x-harness --version
```

Put the installed or built binary on your `PATH`. The remaining commands use `b10x-harness`.
The crates are not published on crates.io.

## 1. Inspect the safe default

```bash
b10x-harness tools --workspace .
```

The JSON answer contains `file_read`, `dir_list`, `search`, and `find`. It contains no write or
execution tool and contacts no model endpoint. Treat this command as the preflight for every new
machine or confinement setup.

## 2. Choose how connection facts are supplied

Harness supports two explicit, inspectable paths.

### Use your own endpoint and named credential

This is the portable path. Name the exact environment variable Harness may read:

```bash
export MY_MODEL_TOKEN='replace-me'
```

Then pass `--base-url`, `--model`, and `--api-key-env MY_MODEL_TOKEN`. A file source is
`--api-key-file /path/to/token`; do not put the credential value itself in argv.

### Select a built-in provider

A provider supplies a tested bundle of endpoint, wire, model, and credential-source facts. Inspect
the bundle before using it:

```bash
b10x-harness providers list
b10x-harness providers show claude
```

Provider-declared credential paths are defaults, not ambient fallback: the selected provider names
the path, `providers show` prints it before a request, and the run records
`credential_source: "provider:<name>"`. The `codex` provider can also renew and atomically rewrite
its own default credential before the first request when it is near expiry. A credential source you
name with a flag is read only and never renewed by Harness.

See [Configure providers and profiles](./guides/profiles.md) before using a built-in provider.

> **Credential handling**
>
> Environment variables can be visible to processes allowed to inspect your environment. Credential
> files and provider stores have their own permissions and rotation rules. Use the source appropriate
> to your machine, and never paste a real token into a command, fixture, issue, or transcript.

## 3. Make the run

With an explicitly named endpoint:

```bash
b10x-harness run \
  --base-url https://gateway.example/v1 \
  --model model-alias \
  --api-key-env MY_MODEL_TOKEN \
  --workspace . \
  --max-turns 12 \
  --max-output-tokens 12000 \
  --max-duration-ms 180000 \
  --input "Map this repository. Name the evidence behind each claim."
```

With a configured default provider, the same task is:

```bash
b10x-harness run --workspace . --input "Map this repository."
```

The raw-endpoint path defaults to `openai-responses`, which sends to
`POST {base-url}/responses`. Add `--wire anthropic-messages` to send to
`POST {base-url}/messages` instead.

## 4. Verify the outcome

For a normal run:

- stdout is the final model answer;
- stderr carries progress, calls, usage, warnings, cost when priced, and the session ID;
- the session is stored outside the workspace under the user's state directory.

Exit status is `0` when the model completed, `2` when work ran and met a named stop, and `1` when
Harness could not start or continue the run. `--json` replaces prose stdout with JSON Lines.
`--no-session` retains no conversation on the machine.

The limits above are deliberately explicit. A cost ceiling additionally needs a dated `--prices`
rate card; Harness refuses an unmeasurable spend limit rather than pretending it binds.

## Next steps

- [First confined change](./tutorials/confined-change.md) adds write capability through substrate.
- [Tools and approvals](./concepts/tools-and-approvals.md) explains the two gates on an effect.
- [Resume and consume events](./guides/sessions-and-events.md) covers sessions and JSONL.
- [Command-line reference](./reference/cli.md) lists the complete public surface.
