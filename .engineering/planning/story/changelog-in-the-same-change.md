---
format: aep.planning-md/1
id: story:changelog-in-the-same-change
kind: story
status: active
title: Three shipped changes enter the changelog they skipped
relations:
- derived_from: epic:tracking-documents-current
- serves: vision:b10x-owns-its-loop
revision: 5
---
## Evidence

- `AGENTS.md:239-240` — "Maintain `CHANGELOG.md` in Keep a Changelog form. Every user-visible behaviour, contract, wire or boundary change enters `Unreleased` **in the same change that implements it**."
- `AGENTS.md:255` — `CHANGELOG.md` is where "what shipped" is tracked.
- `CHANGELOG.md` last written by `a405f46`, 2026-08-29 20:55.
- `719f6e3` (2026-08-29 23:08) — providers and profiles: two new subcommands with six verbs, a `--profile` flag, a config file that carries permission keys, and a new released contract version. `CHANGELOG.md` not in the diff.
- `0c31438` (2026-08-29 23:25) — workspace adoption: `--substrate-embedded` accepts an operator-named directory, capsules move to `$XDG_STATE_HOME`, and the substrate pin moves `0.2.1` → `0.2.2`. `CHANGELOG.md` not in the diff.
- `f701e2e` (2026-08-29 23:30) — a default model and alias resolution wherever a model is named. `CHANGELOG.md` not in the diff.
- `contracts/cli/b10x-harness/2026-08-30/README.md:100-104` — the contract's own *Cutting the next version* step 4: "Enter what changed in `CHANGELOG.md`, naming any flag whose `takes_value` moved".

## Context

Three consecutive commits, twenty-two minutes apart, each user-visible, none entering the changelog.
One of them also cut a released contract version whose own procedure says to write the entry.

Two of the three are boundary changes by this repository's own definitions: a config file that may
carry `write`, an approval ceiling and an allow-list of programs, and a relaxation of which
directories confinement will adopt — with the substrate revision moving underneath them
(`AGENTS.md:36-42`, invariant 2). The commit messages carry the reasoning in full; the file a
consumer reads carries none of it.

## Acceptance

`CHANGELOG.md`'s `Unreleased` section describes providers and profiles, workspace adoption with the
substrate pin move, and the default model — each naming the flag or file surface it changed.

## Scope

Derived 2026-08-30 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Primary surface:** `CHANGELOG.md` (repo root) — cited, the acceptance names it and nothing else
- **Files:** `CHANGELOG.md:8` (`## [Unreleased]`, spans to `:119`) — cited
- **Symbols:** none — cited, the story names no type, function or constant
- **Also likely:** `CHANGELOG.md:120` (`## [0.3.0] — 2026-08-30`) — inferred, all three commits predate the cut release, so the entries may belong there rather than under `Unreleased`
- **Documents:** `CHANGELOG.md` only. `AGENTS.md:251-252`, `AGENTS.md:267` and `contracts/cli/b10x-harness/2026-08-30/README.md:100-104` are read as the governing rule, not edited — cited
- **Not in scope:** `scripts/gate.sh` — cited, the mechanical-guard question belongs to `epic:tracking-documents-current`'s scope, not this story's acceptance
- **Confidence:** high — the acceptance is entirely one document, and the three changes are verified absent from both `Unreleased` and `0.3.0`
- **Would collide with:** any unit that edits `CHANGELOG.md` — adding an `Unreleased` entry, or cutting a release section. No crate surface, so it is disjoint from every code unit

Stale citations found while scoping: the story's `AGENTS.md:239-240` / `:255` are ~12 lines off — the
rule text is at `AGENTS.md:251-252` and the tracking table at `:267`. "`CHANGELOG.md` last written by
`a405f46`" is stale; `4f7c633` and `c5bb2ed` have written it since. The premise still holds: `profiles`,
`--profile`, `--substrate-embedded`, `0.2.2`, `default model` and `alias` are all absent from
`CHANGELOG.md:8-254`.
