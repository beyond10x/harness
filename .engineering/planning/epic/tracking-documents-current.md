---
format: aep.planning-md/1
id: epic:tracking-documents-current
kind: epic
status: draft
title: STATUS and CHANGELOG describe the tree they ship with
summary: Five feature commits after the status page was last written; three user-visible changes with no changelog entry.
relations:
- decomposes: initiative:record-matches-the-code
revision: 2
---
## Evidence

- `AGENTS.md:247-255` — § *Where work is tracked*: `STATUS.md` (what is built, with exit evidence), `ROADMAP.md`, `docs/design/`, `contracts/`, `CHANGELOG.md`.
- `AGENTS.md:239-240` — releases: every user-visible behaviour, contract, wire or boundary change enters `Unreleased` "**in the same change that implements it**".
- `STATUS.md:15` — states `ServerConfig` carries no context window; `crates/harness-app-server/src/lib.rs:46-50` and `:410` show it does.
- `STATUS.md:32-35` vs `STATUS.md:13` — the same file says loop-owned `answer`, `delegate` and hooks exist, and that this component claims "no sub-agents, no hooks, … no structured output".
- `STATUS.md:28-29` vs `STATUS.md:23-24` — the same file says "No Substrate confinement" and "Substrate confinement: **working, embedded, including execution**".
- `STATUS.md:20` vs `STATUS.md:21` — "The ChatGPT/Codex route still has not been contacted", beside a live run against `https://chatgpt.com/backend-api/codex` under a ChatGPT subscription credential on 2026-08-23.
- `STATUS.md:15` and `STATUS.md:16` — the command line is listed as six subcommands (`workflow`, `profiles`, `providers` absent) and the argv pin as `2026-08-29`, while `crates/harness-cli/src/contract.rs:34` pins `2026-08-30`.
- Commits `719f6e3`, `0c31438`, `f701e2e` (2026-08-29, 23:08–23:30) — no `CHANGELOG.md` in any of the three diffs; `CHANGELOG.md` was last written at 20:55 by `a405f46`.
- `STATUS.md` last written by `82f4b85` (2026-08-29 18:20); five feature commits landed after it.

## Outcome

A reader who opens `STATUS.md` learns what the binary does. Today the file contradicts the code in
one place, contradicts itself in three, and predates five feature commits.

## Why Now

The repository is twelve days old and the drift is already four documents wide. Every consumer named
in `README.md:21-26` reads these pages before reading the code.

## Scope

Bring `STATUS.md` to the tree, put the missing changes in `CHANGELOG.md`, and decide whether
anything mechanical should stop it recurring — the gate reads none of these files today
(`scripts/gate.sh:6-11`).

## Done When

`STATUS.md` and `CHANGELOG.md` agree with the tree at a named commit, and the three internal
contradictions above are resolved rather than reworded.
