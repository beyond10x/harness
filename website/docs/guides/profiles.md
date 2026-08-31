---
title: Configure a provider and profile
description: Save connection facts and run permissions without hiding what will happen.
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

:::warning The `codex` provider may update its credential file

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
write-scope = [".git/**=denied", "**=allowed"]
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

For every field, the complete precedence order, model aliases, and credential-renewal contract,
see [Configuration reference](../reference/configuration.md).
