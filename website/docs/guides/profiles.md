---
title: Configure a provider and profile
sidebar_position: 2
description: Save connection facts and run permissions without hiding what will happen.
lede: A provider holds connection facts and a profile holds run permissions; Harness ships providers and never ships a permission profile.
source: crates/harness-cli/src/provider.rs; crates/harness-cli/src/profile.rs; crates/harness-cli/src/trust.rs; b10x-harness providers --help; b10x-harness profiles --help
---

# Configure a provider and profile

Use a **provider** for connection facts and a **profile** for run permissions. Providers grant no
tools; profiles can, so Harness ships providers but never ships permission profiles.

## Inspect a built-in provider

Before spending a token, inspect the effective endpoint, model, credential source, and renewal
behavior:

```bash
b10x-harness providers list
b10x-harness providers show claude
b10x-harness providers show codex
```

Built-in providers may name default credential sources. `claude` reads
`~/.claude/.credentials.json`; `codex` reads `~/.codex/auth.json`; `openai` reads
`OPENAI_API_KEY`. This is visible in `providers show` and in the run record as
`credential_source: "provider:<name>"`.

:::caution[The codex provider may update its credential file]

When its default token is near expiry, `codex` renews it before the model call and atomically
rewrites `~/.codex/auth.json`. Harness reports that write even under `--quiet`. Naming your own
OAuth source disables renewal: explicitly named credential files are read-only.

:::

## Create a configuration

Generate the commented starter:

```bash
b10x-harness profiles init
```

Then keep a safe default in the printed configuration file:

```toml
[default]
provider = "claude"
write = false
```

Now a read-only run needs no connection flags:

```bash
b10x-harness run --workspace . --input "Map this repository."
```

## Add a confined change profile

Permission belongs in a named profile you can inspect and select deliberately:

```toml
[[profiles]]
name = "confined-change"
write = true
approve-up-to = "low"
write-scope = [".git/**=denied", ".b10x/**=denied", "**=allowed"]
```

Preview the resolved argv without contacting a model:

```bash
b10x-harness profiles explain -p confined-change
```

Then follow [Make a confined change](../tutorials/confined-change.md) to supply the required
substrate boundary and run it.

## Override one run

A typed flag wins over the configuration:

```bash
b10x-harness run -p confined-change --model haiku --workspace . --input "..."
```

Typing `--base-url` opts out of the provider bundle entirely. Supply the matching wire, model, and
credential source explicitly; Harness will not combine an arbitrary endpoint with half of a
provider definition.

## Let a repository choose its model

A repository can ship the plumbing its own work needs in `.b10x/harness.toml` at the root of the
workspace a run is given (`--workspace`, or the current directory):

```toml
[default]
provider = "claude"
model = "sonnet"
max-turns = 40
```

Harness reads that file only for a workspace you have trusted:

```bash
b10x-harness profiles trust .
b10x-harness profiles list
```

`trust` resolves the path to its canonical form and records it in `harness-trust.toml`, beside your
own configuration file; your `harness.toml` is never edited. A symlink and its target are the same
repository. Another checkout, a parent directory or a subdirectory is not trusted by it.
`b10x-harness profiles untrust .` takes it back. Trust is a decision about the path, not the file:
once a repository is trusted, later edits to its file apply without a new `trust`.

`untrust` also works for a repository that has since been deleted or moved, named the way you
trusted it: `..` and a trailing slash are read as written, a symbolic link on the way is followed
even when its target is gone, and a path that has since become a symbolic link loses both the entry
for where it points and the entry for the path itself.

The trust record must be yours: a `harness-trust.toml` owned by another user, or one its group or
anyone else may write, refuses the run by name instead of being read. `trust` and `untrust` hold a
lock beside it while they change it, so two at once cannot undo each other.

Until you trust it, the file is not read at all. The run goes ahead without it and prints one line
on standard error naming the file and the exact `profiles trust` command.

A repository profile is **plumbing only**. It is one `[default]` table and it may set `provider`,
`model` and `max-turns`, nothing else. Anything that decides what a run may do — `write`,
`approve-up-to`, `allow-program`, `toolchains`, `toolchain-specs`, `write-scope`, `plugin-dir` — and
every other table, including `[providers]` and `[[profiles]]`, refuses the run by name before any
model is called. Permissions, endpoints, credentials and hooks stay in files you wrote.

A trusted file is read only as a regular file of at most 64 KiB at exactly `.b10x/harness.toml`:
a symbolic link there or on the way, a FIFO, a device, a larger file, text that is not UTF-8 or
does not parse, and a value of the wrong type each refuse the run by name, and so do a `provider`
holding a control, bidi or other unprintable character and a `max-turns` below one. No refusal
quotes the file's values; a refused key is named with every such character escaped, so nothing
from the file acts on your terminal, and `profiles explain` shows a repository's `model` the same
way.

A run's default write scope denies `.b10x/**` as well as `.git/**`, so a run that may write cannot
rewrite the profile the next run applies. A `write-scope` you write yourself replaces the default;
keep `.b10x/**=denied` in it.

A repository's `provider` reads that provider's default credential exactly as a typed `--provider`
would, and nothing more: your `profiles trust` is what points Harness at it, and the credential
stays paired with its own provider's endpoint. When the repository's `provider` is what chose the
provider, a credential you typed (`--api-key-env`, `--api-key-file`, `--api-key-secret`,
`--oauth-token-file`, `--oauth-token-env`) refuses the run before any request: you typed it for a
provider you chose, and the repository does not get to decide which endpoint receives it. Choose
the provider yourself with a `-p` profile or your own `[default]`, or type `--base-url` beside the
credential.

For those three keys the repository sits between your defaults and your choices:

1. your `[default]`
2. the trusted repository profile
3. each `-p` in order
4. a typed flag

The run's `session.started.profiles` names it `repository`, with the file it came from and a digest
of its table. `profiles list`, `profiles show` and `profiles explain` read the repository profile of
the current directory, which is the workspace a run gets when `--workspace` is not typed.

For every field, the complete precedence order, model aliases, and credential-renewal contract,
see [Configuration reference](../reference/configuration.md).
