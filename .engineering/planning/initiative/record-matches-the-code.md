---
format: aep.planning-md/1
id: initiative:record-matches-the-code
kind: initiative
status: draft
title: The pages this repository tracks work on say what the code does
summary: STATUS, CHANGELOG and the pinned contracts drifted from the tree inside twelve days; the tracking is prose and prose does not fail a gate.
relations:
- serves: vision:b10x-owns-its-loop
revision: 2
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
