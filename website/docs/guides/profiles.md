---
title: Profiles and providers
description: Put the flags that never vary in a file, and keep the run explainable afterwards.
---

# Profiles and providers

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

Two are built in: `claude` (Anthropic Messages, subscription OAuth) and `openai` (OpenAI Responses,
`OPENAI_API_KEY`). Override any field without restating the rest — a provider is a bag of
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

`openai` ships no aliases: that vendor's current identifiers have not been read off a working
account here, and an alias pointing at a model that does not exist is worse than none.

### The credential is defaulted, and the record says so

A built-in provider names where its credential lives, which means the binary looks in a vendor
directory it was not pointed at. That is a real softening of the rule stated in
[the security boundary](../concepts/security-boundary.md), and it is paid for rather than waved
away: a run whose credential came from a provider reports

```json
"credential_source": "provider:claude"
```

instead of the flat `"named"`, and `providers show` prints the path before a token is spent.
Something is defaulted; nothing is silent. A credential you name yourself still wins, and still
reports `"named"`.

If the file a provider names is not there, the run refuses at startup rather than failing at its
first request.

## Profiles

```toml
[[profiles]]
name = "write"
write = true
approve-up-to = "high"
allow-program = ["/usr/bin/git"]
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

```
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
