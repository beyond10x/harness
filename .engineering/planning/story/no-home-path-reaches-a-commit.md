---
format: aep.planning-md/1
id: story:no-home-path-reaches-a-commit
kind: story
status: active
title: A tracked file carrying an absolute home directory fails the gate
summary: The audit that found twenty leaked home directories across five repositories was run once, by hand, after publication; a gate step makes the twenty-first impossible.
relations:
- derived_from: epic:adoption-follow-ups
- informed_by: story:history-carries-a-home-directory
- serves: vision:b10x-owns-its-loop
revision: 4
---
## What is missing

Nothing stops an absolute home directory being committed again. The leak that
`story:history-carries-a-home-directory` records was found by an audit run once, by hand, after the
repository was already public.

## Evidence

- `story:history-carries-a-home-directory` § *Prevention*: the audit across five repositories found
  **twenty tracked files** publishing the operator's home directory. A check for absolute home
  directories in tracked files would have caught **nineteen** of them.
- The twentieth was a committed `.pyc` in `entity-runtime` whose `co_filename` embedded the path, and
  which **no text grep would ever have found**. That is why the check has to start from `git
  ls-files` and decide what to do about bytes it cannot read as text, rather than grepping the tree.
- The gate already has this shape: `scripts/gate.sh:9-11` runs three `scripts/check-*.py` steps that
  each refuse by name. A fourth is the same kind of thing and costs the gate nothing.

## Context

This repository went public on 2026-08-30 and four siblings were already public. The exposure that
prompted the audit was judged acceptable against a force-push that would invalidate `metaharness`'s
pinned harness commits — a decision recorded in `story:history-carries-a-home-directory` and not
reopened here.

What that decision does not do is stop the next one. The cost of a second occurrence is higher than
the first, because the argument that retired the first was *it is already public elsewhere*, and that
argument does not survive being used twice.

The second defect is worth keeping in view: those tests passed on exactly one machine, so the
absolute path was a **portability bug wearing a privacy bug's clothes**. A check that refuses the
path refuses both, and the portability half is the one a contributor feels.

## Acceptance

- `scripts/check-no-home-paths.py` runs in `scripts/gate.sh` and exits non-zero when a tracked file
  contains an absolute home directory, naming the file and the line.
- It enumerates through `git ls-files`, not a filesystem walk, so nothing untracked is judged and
  nothing tracked is skipped.
- A file it cannot read as UTF-8 is **not silently passed**: it is searched as bytes, or named as
  unreadable and refused. The `.pyc` case is the one the check exists for, and a check that skips
  what it cannot decode would have missed it.
- The pattern is the shape of a home directory (`/home/<name>/`, `/Users/<name>/`), not one
  operator's name, so it protects a contributor the same way.
- A planted absolute path in a tracked file is caught by a test, and the check passes the tree
  unmodified.
- `AGENTS.md` names the check where it names the other three.

## Out of Scope

- Rewriting history. That is `story:history-carries-a-home-directory`'s decision and it stands.
- Credential scanning. `git log -S` over the published history already found no credential pattern
  (`sk-ant-`, `ghp_`, `github_pat_`, `xoxb-`, `AKIA`, PEM headers); a live secret scanner is a
  separate thing with a separate failure mode.
- The four sibling repositories. Each needs its own copy; this story buys the one that can be tested
  here, and a second story can carry it outward once the shape is settled.

## Scope

Derived 2026-08-30 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Primary surface:** `scripts/` — cited, the acceptance names the script and the gate it joins
- **Files:** `scripts/check-no-home-paths.py` (new file) — cited, named verbatim in the acceptance
- **Files:** `scripts/gate.sh:9-11` — cited, the story cites these lines and they hold the three
  existing `check-*.py` steps verbatim; a fourth line lands here
- **Symbols:** `git ls-files`, the `/home/<name>/` and `/Users/<name>/` shapes — cited
- **Documents:** `AGENTS.md:218-224` § *The gate* — cited, the acceptance requires the new check be
  named where the other three are; that sentence is at `:223-224`
- **Documents:** `README.md:52-61` — inferred, the same gate step table lists all three checks and
  would go stale; the story does not name it
- **Also likely:** the check's own exemption list — inferred. Four tracked files carry `/home/you/`
  placeholders that match the specified shape (`crates/harness-cli/src/render.rs:550`,
  `crates/harness-loop/src/event.rs:464`, `website/docs/guides/profiles.md:158`,
  `website/docs/guides/sessions-and-events.md:105`), and two tracked planning-store files carry a
  literal `/home/timo` (`.engineering/planning/journal.jsonl:16,153`,
  `.engineering/planning/story/history-carries-a-home-directory.md:17`). The acceptance clause
  "the check passes the tree unmodified" cannot hold today without either an exemption in the
  script or edits to those six files.
- **Confidence:** high — the story names the script path, the gate file and the line range, and all
  three were read in the tree
- **Would collide with:** any unit adding or reordering a step in `scripts/gate.sh`; any unit
  editing `AGENTS.md` § *The gate* or the `README.md` build-and-test table. It does **not** touch
  any crate's source, so it is disjoint from all Rust work unless the exemption is taken as edits
  to `harness-cli` / `harness-loop` rather than as an allowlist in the script.

Open when scoped: where "a planted absolute path is caught by a test" lands — no existing
`scripts/check-*.py` has a self-test, no pytest tree exists, and no Rust test invokes a check script.
Whether the six tracked home-shaped paths get an allowlist or an edit is unstated, and the two
choices have different collision surfaces.
