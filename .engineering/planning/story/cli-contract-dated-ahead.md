---
format: aep.planning-md/1
id: story:cli-contract-dated-ahead
kind: story
status: draft
title: The CLI pin in force is dated when it was cut and diffed against what preceded it
relations:
- derived_from: epic:pinned-interfaces-honest
revision: 3
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

## Scope

Derived 2026-08-30 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Primary surface:** `contracts/cli/b10x-harness` — cited, the acceptance is "a new CLI contract version"
- **Files:** `contracts/cli/b10x-harness/<new-version>/{argv.json,manifest.json,README.md}` — inferred, a new directory; today's cut is `2026-08-30.2`, since `2026-08-30` and `2026-08-30.1` both exist
- **Files:** `crates/harness-cli/src/contract.rs:34` — cited, `ARGV_CONTRACT_VERSION` must point at the new version
- **Files:** `crates/harness-cli/src/contract.rs:335` — cited, the hard-coded version list in `every_released_argv_version_is_still_pinned_beside_the_current_one` names every directory and fails until the new one is added
- **Files:** `CHANGELOG.md` `[Unreleased]` — cited, **required**: `AGENTS.md:250-251`, invariant 14 (`AGENTS.md:98-101`, "re-pins the fixture *and* enters the changelog"), the cutting recipe at `contracts/cli/b10x-harness/2026-08-30.1/README.md:105`, and precedent at `CHANGELOG.md:62-65`
- **Symbols:** `ARGV_CONTRACT_VERSION`, `every_released_argv_version_is_still_pinned_beside_the_current_one`, `the_pinned_argv_contract_is_what_this_binary_defines` — cited
- **Not touched:** `scripts/check-cli-contract.py` — cited, it walks whatever directories exist and holds no version literal, so the Python half goes green without an edit
- **Not touched:** `crates/harness-cli/src/lib.rs:988-991` — inferred, the `--wire` post-clap default is the epic's other half and outside this acceptance
- **Documents:** the new version's `README.md` is the substance of the story — its "what changed" must be measured against the immediately preceding version and name every field that moved — cited
- **Confidence:** high — the story names the const, the file and the two directories, and every surface above was read in the tree
- **Would collide with:** any unit touching `contracts/cli/b10x-harness/`, `crates/harness-cli/src/contract.rs`, or the `[Unreleased]` section of `CHANGELOG.md` — the last is repo-wide and collides with almost any shipping unit

Stale citation found while scoping: the story cites `ARGV_CONTRACT_VERSION = "2026-08-30"`, but
`crates/harness-cli/src/contract.rs:34` reads `"2026-08-30.1"` (cut by `c5bb2ed` for
`--delegate-parallel`). `2026-08-30.1` is **not** on `origin/main`, so by invariant 13 it is
unreleased and may be corrected in place rather than superseded — which of the two the story wants is
not settled by its body.
