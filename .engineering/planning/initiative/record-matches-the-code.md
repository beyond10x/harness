---
format: aep.planning-md/1
id: initiative:record-matches-the-code
kind: initiative
status: proposed
title: The pages this repository tracks work on say what the code does
summary: STATUS, CHANGELOG and the pinned contracts drifted from the tree inside twelve days; the tracking is prose and prose does not fail a gate.
relations:
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Evidence

- `AGENTS.md:247-255` — where work is tracked: `STATUS.md` for what is built, `ROADMAP.md` for what is next, `docs/design/`, `contracts/`, `CHANGELOG.md`. All prose; none of it fails a gate.
- `AGENTS.md:239-240` — "Every user-visible behaviour, contract, wire or boundary change enters `Unreleased` **in the same change that implements it**."
- `AGENTS.md:81-97` — invariant 13: a contract version is immutable once reachable on `origin/main`; a second cut on one day takes a `.N` suffix, because "dating a directory tomorrow — puts a false date on a pinned artefact".
- `STATUS.md:15` — claims `ServerConfig` carries no context window; `crates/harness-app-server/src/lib.rs:46-50` and `:410` say it does, and has since `9f26ad5` (2026-08-29 13:27), five hours before `STATUS.md` was last written (`82f4b85`, 2026-08-29 18:20).
- `STATUS.md:32-35` — "No sub-agents, no hooks, no MCP client, no multimodal input, no structured output" — contradicted by `STATUS.md:13` in the same file, which says `answer`, `delegate` and the hook port exist and shipped on 2026-08-29.
- Commits `719f6e3` (2026-08-29 23:08), `0c31438` (23:25) and `f701e2e` (23:30) — providers, profiles, workspace adoption and a default model, none of them entering `CHANGELOG.md`, which was last touched at 20:55 by `a405f46`.
- `contracts/cli/b10x-harness/2026-08-30/` — a released contract directory dated one day after the commit that created it.

## Context

This repository's plan lives in prose, and the prose is good: `STATUS.md` states exit evidence per
area, `ROADMAP.md` states outcomes, `AGENTS.md` states invariants that can be checked. What none of
them has is a gate. `scripts/gate.sh` runs tests, format, clippy and three contract checkers; no
step reads `STATUS.md`.

The measurable result, inside twelve days: five feature commits landed after the status page was
last written, three of them user-visible with no changelog entry, and a released contract carrying a
false date and a "what changed" section measured against the wrong predecessor. None of these is a
bug in the harness. Each is a bug in the only thing an outside reader has to go on.

## Done When

The four tracking documents agree with the tree at a named commit, and the drift that produced this
initiative is either prevented by a check or recorded as accepted with its reason.

## Where this stands — 2026-09-15

Recorded during triage of the draft backlog (ORG-0201). Two of the three epics under this initiative
are closed; the third is one task wide.

- `epic:tracking-documents-current` — `implemented`. `STATUS.md:3` names the commit it was observed
  at and `CHANGELOG.md:10` carries the release it names; the four contradictions this initiative was
  drafted from are gone from the file rather than reworded.
- `epic:pinned-interfaces-honest` — `implemented`. The pin in force is
  `contracts/cli/b10x-harness/2026-09-02`, cut the day it is dated, diffed against `2026-09-01`, and
  recording the defaults the binary applies after clap.
- `epic:gate-stays-trustworthy` — open, and only `task:gate-token-steps-in-one-action` is left in it.

**The initiative's own *Done When* has a second clause that nothing has answered:** "the drift that
produced this initiative is either prevented by a check or recorded as accepted with its reason."
`gate()` in `crates/harness-xtask/src/main.rs:75-105` runs tests, the conformance target, format,
clippy and the contract checkers; no step reads `STATUS.md`, `ROADMAP.md` or `CHANGELOG.md`. So the
documents were brought to the tree by hand twice and nothing stops a third drift.

That is the open question this initiative carries, and it is not filed as a story anywhere. It needs
a decision before it needs an implementation — whether a tracking document is gateable at all, or
whether the drift is accepted with its reason written down.
