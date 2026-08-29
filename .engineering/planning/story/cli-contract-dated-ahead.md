---
format: aep.planning-md/1
id: story:cli-contract-dated-ahead
kind: story
status: draft
title: The CLI pin in force is dated when it was cut and diffed against what preceded it
relations:
- derived_from: epic:pinned-interfaces-honest
revision: 2
---
## Evidence

- `AGENTS.md:94-97` — invariant 13: "**A second cut on the same day takes a `.N` suffix**: `2026-08-29`, then `2026-08-29.1`, then `2026-08-29.2`. The scheme is date-based and cannot otherwise express two cuts in one day, and the alternative — dating a directory tomorrow — puts a false date on a pinned artefact."
- `AGENTS.md:88-93` — "**Released means reachable on `origin/main`**", from which moment "what they pinned to must not move under them".
- `contracts/cli/b10x-harness/2026-08-30/` — created by `719f6e3`, committed 2026-08-29 23:08, and reachable on `origin/main`: a directory dated the day after the commit that cut it, on a day when `2026-08-29`, `.1`, `.2` and `.3` had already been cut.
- `contracts/cli/b10x-harness/2026-08-30/README.md:8` — "What changed since **2026-08-29.1**", skipping `.2` (added by `a405f46`) and `.3`, which are its real predecessors.
- `contracts/cli/b10x-harness/2026-08-30/README.md:10-13` — "**Strictly additive.** Nothing in `2026-08-29.1` was renamed, removed, or changed in shape: every flag … keeps its spelling, its `takes_value`, its **default**, its conflicts and its requirements. A consumer pinned to `.1` is correct against this binary and needs to change nothing."
- The bytes disagree: against `2026-08-29.3/argv.json`, `--model` and `--base-url` move `required: true → false` on `run`, `chat` and `workflow run`, and `--wire` loses `"default": "openai-responses"`.
- `crates/harness-cli/src/contract.rs:34` — `ARGV_CONTRACT_VERSION = "2026-08-30"`: this is the document in force.

## Context

The CLI contract exists because a flag changed shape and a consumer pinned to `0.1.0` was refused by
clap before any harness code ran (`AGENTS.md:84-86`). The version now in force carries a date that is
not the day it was cut, and a *what changed* section measured against a version two cuts back —
which is how it comes to claim strict additivity for a diff that moved three flags.

Because the directory is on `origin/main` it is released, and invariant 13 forbids editing it. The
fix is therefore a new dated version, which also gives the date question a clean answer.

## Acceptance

A new CLI contract version dated the day it is cut, whose "what changed" section is measured against
the version immediately before it and names every field that moved, with `ARGV_CONTRACT_VERSION`
pointing at it and both halves of the check green.
