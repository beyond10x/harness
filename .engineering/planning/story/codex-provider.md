---
format: aep.planning-md/1
id: story:codex-provider
kind: story
status: draft
title: A codex provider, once its credential location has been measured
relations:
- derived_from: epic:adoption-follow-ups
revision: 2
---
## What is missing

`b10x-harness` ships two providers, `claude` and `openai`. A `codex` provider is wanted so a
subscription to that vendor is one word of config, as `provider = "claude"` already is.

- `crates/harness-cli/src/provider.rs`, `built_in()` — where it would go.

## Why it was left

**Its credential location has never been read off a working install.** `claude`'s values are the
ones a live run actually used, taken from the eval that drives it; nothing equivalent exists for
codex. A provider naming a path that turns out to be wrong fails at the far side, where this build
can say nothing useful about why — and inventing a vendor path is the single mistake the provider
table exists to avoid, which its own module doc records.

`metaharness`'s codex adapter knows about `CODEX_HOME` and a config document inside it
(`crates/metaharness-codex/src/launch.rs:58-97`), but that is a scratch home the adapter *writes*,
not where a real installation keeps a subscription token.

## Acceptance

Someone reads the credential location off a working codex installation and records what they read —
the path, the document shape, and the pointer to the token inside it. The provider entry follows
from that in one commit, with a test that the file exists on a machine that has codex and skips
where it does not.

Until that measurement exists this story cannot be implemented, only guessed at.
