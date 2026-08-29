---
format: aep.planning-md/1
id: story:repo-local-profiles
kind: story
status: draft
title: A repository may supply profiles, once a trust decision says how
relations:
- derived_from: epic:adoption-follow-ups
revision: 2
---
## What is missing

Profiles are read from `$XDG_CONFIG_HOME/b10x/harness.toml` only. A repository-local
`./.b10x/harness.toml` would let a project ship the profile its own work needs — the write scope for
its tree, the programs its tests run — instead of every contributor writing the same table by hand.

- `crates/harness-cli/src/profile.rs`, `config_path()` — the single place the location is decided.
- `website/docs/guides/profiles.md`, § *Not yet* — where the absence is already stated to readers.

## Why it was left

**A profile is executable policy.** It carries `write`, an approval ceiling, an allow-list of
programs and a write scope; the whole design rests on nothing of that shape being compiled in, so
that every rule a run obeys is a file somebody can read. A repository you cloned supplying one
silently inverts that: the rules would arrive with the code rather than from the operator.

This is a trust decision before it is an implementation, which is why it was deferred rather than
attempted.

## What has to be decided first

1. Does a repo-local file need explicit acceptance — a `b10x-harness profiles trust .` recorded in
   the operator's own config — or is it read on sight?
2. May it carry permission keys at all, or only plumbing (`provider`, `model`, limits)? A file that
   may only reduce what a run can do is a different and much smaller decision than one that may
   raise it.
3. Precedence against `[default]` and `-p`.

## Acceptance

Whatever is decided, the run's record still names every profile that contributed with a digest —
`session.started.profiles` — because that record is the condition on which a file may carry a
permission at all, and a repo-local file does not get an exemption from it.
