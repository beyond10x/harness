---
title: Configuration reference
description: Provider and profile fields, precedence, model aliases, credential defaults, and renewal behavior.
---

# Configuration reference

`run` accepts around fifty options. A useful invocation is twelve of them and a confined one is
sixteen, and most of those never vary: the endpoint, wire, model and credential are the same for
every run against a given provider, and the confinement flags are the same for every run of a given
*kind of work*.

Two mechanisms move them into `~/.config/b10x/harness.toml`, and the line between them is
**permission**.

| | A **provider** | A **profile** |
|---|---|---|
| Says | where to talk, and how | what the run may do |
| Carries | `base-url`, `wire`, `model`, credential source | `write`, approval ceiling, allow-list, write scope |
| Grants the run anything? | no | yes |
| Ships inside the binary? | **yes** | **no** |

That asymmetry is the design. A provider serves a read-only run and a destructive one identically,
so shipping a collection of them costs nobody anything. A profile decides what may happen, so
nothing of that shape is compiled in: every rule a run obeys is in a file you can read, diff and
put under version control.

## The shortest useful configuration

```toml
# ~/.config/b10x/harness.toml
[default]
provider = "claude"
write = false
```

`b10x-harness profiles init` writes a commented starter and prints its path. With that file, a run
needs no connection flags at all:

```bash
b10x-harness run --workspace . --input "Map this repository."
```

## Providers

```bash
b10x-harness providers list          # what this build ships, and what you have overridden
b10x-harness providers show claude   # the effective endpoint, wire, model and credential path
```

Three are built in:

| provider | route | credential |
|---|---|---|
| `claude` | Anthropic Messages | subscription OAuth, `~/.claude/.credentials.json` |
| `openai` | OpenAI Responses | `OPENAI_API_KEY` |
| `codex` | OpenAI Responses, **ChatGPT subscription** | subscription OAuth, `~/.codex/auth.json` |

`openai` and `codex` are two providers because they are two things to be: one bills an API key, the
other bills a person's ChatGPT plan, at a different endpoint. A single entry would make *which am I
billing* unanswerable from the config.

Override any field without restating the rest — a provider is a bag of
independent connection facts, so changing the model must not silently drop the endpoint it is
served from:

```toml
[providers.claude]
model = "sonnet"
```

### Model aliases

A vendor's exact identifiers carry release dates, and the dates move. Write
`claude-haiku-4-5-20251001` into a config and every config in the fleet goes stale on the next
release — as a `404` from the far side, which nothing here can explain.

So a provider ships short names, and they work wherever a model is named — in the config, or as
`--model haiku` on the command line:

| alias | resolves to | |
|---|---|---|
| `opus` | `claude-opus-5` | **`claude`'s default** |
| `sonnet` | `claude-sonnet-5` | |
| `haiku` | `claude-haiku-4-5-20251001` | |
| `fable` | `claude-fable-5` | |

The **default is itself written as an alias**, so one table answers *which one is current* and a
release does not strand every config that never named a model.

`opus` is the default deliberately: the capable model, at materially more per run than `haiku`.
If that is the wrong trade for you, `[providers.claude] model = "haiku"` is one line and
`--model haiku` is none.

An alias is **this build's** answer to *which one is current*, so a run that asked for `haiku` is
pinned by the binary it ran under rather than by whatever a config was last edited. A name the
table does not know passes through untouched, so a model released after your binary is still
reachable by its exact identifier.

Aliases resolve **wherever a model is named** — a typed `--model`, a `[default] model`, a
profile's, an override's, and the provider's own default. All four go through the same table.

Add your own, merged over the shipped set — a same-named one wins, which is how you correct a stale
built-in without waiting for a release:

```toml
[providers.claude.aliases]
cheap = "claude-haiku-4-5-20251001"
```

`session.started.model` always records what the alias resolved to. **An alias is a convenience at
the command line, never in the evidence.**

`openai` and `codex` ship no aliases: that vendor's current identifiers have not been read off a
working account here in a form worth pinning short names to, and an alias pointing at a model that
does not exist is worse than none.

### The credential is defaulted, and the record says so

A built-in provider names where its credential lives, which means the binary looks in a vendor
directory it was not pointed at. That is a real softening of the rule stated in
[the security boundary](../concepts/security-boundary.md), and it is paid for rather than waved
away: a run whose credential came from a provider reports

```json
"credential_source": "provider:claude"
```

instead of a typed source such as `"api-key:file"`, and `providers show` prints the path before a
token is spent. Something is defaulted; nothing is silent. A credential you name yourself still
wins and reports its credential class plus `file` or `environment`; the path, variable name and
secret never enter the record. Naming no credential reports `"none"`.

If the file a provider names is not there, the run refuses at startup rather than failing at its
first request.

### `codex` renews a stale token, and rewrites the file it came from

A subscription token expires. When `codex` supplies the credential and that token is within fifteen
minutes of expiring, the run presents the refresh token beside it to the vendor's authorization
server, takes the new one, and **writes it back into `~/.codex/auth.json`** before the first
request.

That is a bigger step than defaulting a path, so it is bounded the same way — by being readable and
by being said:

```shell-session
$ b10x-harness providers show codex
...
renews               yes, when the token is within 15 minutes of expiring
token-endpoint       https://auth.openai.com/oauth/token
client-id            app_EMoamEEZ73f0CkXaXp7hrann
refresh-pointer      /tokens/refresh_token
```

and a run that actually renewed says so, on stderr and in the record, **even under `--quiet`**:

```text
renewed [codex] /home/you/.codex/auth.json, valid to 2026-09-18, refresh token rotated
```

Four rules hold that write down:

- **Only a credential a provider defaulted.** Name your own with `--oauth-token-file` or
  `[providers.codex] oauth-token-file = …` and the renewal switches off: this build reads the file
  you named and never writes it.
- **Atomic.** The new document is written beside the original, parsed back to check it says what it
  should, and only then renamed over it. A crash in the middle leaves the old file intact.
- **Byte-preserving.** Only the token values move. Key order, indentation and keys this build has
  never heard of survive exactly — it is another program's file. Where that cannot be proven safe
  the document is re-serialised instead, and the record's `byte_preserving: false` says so.
- **No part of the credential is recorded.** Not a prefix, not a length, not a digest.

`refresh token rotated` in that line is worth reading: it means the refresh token that was on disk
has been retired, so a backup of that file taken before the run no longer holds a working
credential. Recovery is `codex login`, not restoring the copy.

`claude` does **not** renew, and that is deliberate rather than pending: its credential file holds a
refresh token, but the authorization server and client that would accept it have not been read off
a working install here. A guessed token endpoint is the same mistake as a guessed credential path,
with a worse failure — it presents your refresh token to a server nobody verified.

## Profiles

```toml
[[profiles]]
name = "write"
write = true
approve-up-to = "high"
allow-program = ["/usr/bin/git"]
toolchains = ["rust", "taskfile"]
# Paths resolve from this configuration file's directory and are never workspace-discovered.
toolchain-specs = ["toolchains/company.yaml"]
```

```bash
b10x-harness run -p write --workspace . --input "..."
```

### `write` is one key, and it is off

Whether a run may change anything is a single switch, so reading a config never means assembling
that answer from four keys. Absent or `false`, the run gets the four read-only tools and no `run`
tool whatever else the table says. A profile that declares programs without `write` is **told so at
startup** rather than leaving the model to discover it by being refused mid-run.

Turning writing on does not turn off the approval gate. `write = true` with no `approve-up-to`
still meets the default approver, which denies — see [Approvals](../concepts/tools-and-approvals.md).

`write-scope` defaults to denying `.git/**` and allowing the rest. That default is deliberate and
not a convenience: running against a real checkout is what these mechanisms make ordinary, and a
model rewriting history there is the failure they make possible for the first time. It must not
depend on a key somebody remembered to write.

## What wins

1. the built-in provider
2. `[providers.<name>]`, **field by field**
3. `[default]`
4. each `-p` in order, later winning a contested key — **whole keys, not merged**
5. **a typed flag beats everything** — you are speaking now

A profile may set `provider`, so one `-p` can move both the model and the endpoint.

**Typing `--base-url` opts out of the provider entirely.** A provider is a bundle whose parts belong
together; half-applying one over your own endpoint would point one vendor's dialect at a server
that has never heard of it. The profile's permission keys still apply — those are about what the
run may do, not who it is talking to.

## Read it before you run it

```bash
b10x-harness profiles explain -p write
```

prints the argv the configuration expands to, and which profile set each key, without contacting
anything:

```text
profile default <- /home/you/.config/b10x/harness.toml (0d8a9892b1f6)
profile write <- /home/you/.config/b10x/harness.toml (37550c38dd29)
--base-url https://api.anthropic.com/v1
--wire anthropic-messages
--model claude-haiku-4-5-20251001
--oauth-token-file /home/you/.claude/.credentials.json
--substrate-embedded
--write-scope .git/**=denied
--write-scope **=allowed
--approve-up-to high
```

## Why a file is safer than the flags it replaces

The instinct runs the other way, so here is the evidence. A governed run lost all eight of its
sessions to a hand-assembled command line that dropped one flag and ran unenforced while looking
clean. Sixteen options retyped per invocation is where that failure lives.

A profile is the same declaration made once, and `session.started` names every profile that
contributed with a digest of what it said:

```json
"profiles": [
  {"name": "default", "source": "/home/you/.config/b10x/harness.toml", "sha256": "0d8a98…"},
  {"name": "write",   "source": "/home/you/.config/b10x/harness.toml", "sha256": "37550c…"}
]
```

That record is the **condition** on which a file is allowed to carry a permission, not a bonus
feature. The digest is over the profile's own table, so a run stays attributable to the exact rules
it ran under even after somebody edits an unrelated profile beside it.

A key this build does not read refuses the file by name rather than being skipped, for the same
reason: a rule its author wrote and the run would not have applied is worse than a run that will
not start.

## Not yet

A repository-local `./.b10x/harness.toml` is not read. It needs a trust decision first — a profile
is executable policy, and a repository you cloned must not silently supply one.
