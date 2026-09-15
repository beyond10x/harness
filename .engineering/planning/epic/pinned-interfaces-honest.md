---
format: aep.planning-md/1
id: epic:pinned-interfaces-honest
kind: epic
status: implemented
title: A pinned interface document says what the binary does
summary: The current CLI pin is dated ahead, diffed against the wrong predecessor, and records defaults the binary no longer holds.
relations:
- decomposes: initiative:record-matches-the-code
- informed_by: specification:published-interfaces
revision: 6
---
## Evidence

- `AGENTS.md:81-97` — invariant 13: a contract version is immutable once reachable on `origin/main`; a second cut on one day takes a `.N` suffix, "and the alternative — dating a directory tomorrow — puts a false date on a pinned artefact".
- `AGENTS.md:98-104` — invariant 14: both halves, and the CLI pin is generated from clap's own definition because a hand-written one drifts.
- `contracts/cli/b10x-harness/2026-08-30/` — created by `719f6e3` on 2026-08-29 23:08 and reachable on `origin/main`: a released directory dated a day ahead of the commit.
- `contracts/cli/b10x-harness/2026-08-30/README.md:8-13` — "What changed since **2026-08-29.1**" and "**Strictly additive** … every flag … keeps its spelling, its `takes_value`, its default, its conflicts and its requirements", while `2026-08-29.2` and `.3` are the versions in between and the bytes changed three flags.
- `contracts/cli/b10x-harness/2026-08-30/argv.json` vs `contracts/cli/b10x-harness/2026-08-29.3/argv.json` — `--model` and `--base-url` moved `required: true → false` on `run`, `chat` and `workflow run`; `--wire` lost its `"default": "openai-responses"`.
- `crates/harness-cli/src/contract.rs:34` — `ARGV_CONTRACT_VERSION = "2026-08-30"`, so this is the pin in force.
- `crates/harness-cli/src/lib.rs:988-991` — `--wire` still defaults, in code (`unwrap_or_default`), which the pinned document has no field for.

## Outcome

The three published pins can be read by a consumer without being wrong about the binary.

## Why Now

The CLI contract exists because a consumer pinned to `0.1.0` was refused by clap when
`--substrate-embedded` changed shape (`AGENTS.md:84-86`). The document that was cut to prevent that
now carries a false date and a false diff.

## Scope

The CLI argv pin: a correct dated version, and a document that can express a default the binary
applies after clap. The provider-wire pins' live capture is `epic:wire-pins-from-live-bytes`.

## Done When

The version in force is dated the day it was cut, its "what changed" section is measured against its
real predecessor, and every flag whose effective default is not clap's is either recorded or
explicitly out of what the pin claims to cover.

## Closed

Closed 2026-09-15 by triage of the draft backlog (ORG-0201).

All eight stories derived from this epic are `implemented`, and the three clauses of *Done When* are
each answered in the tree:

- **dated the day it was cut.** `contracts/cli/b10x-harness/2026-09-02/` was created by commit
  `1d84c6e`, authored 2026-09-02 02:43 +0200 — same day, not a day ahead. The earlier same-day
  recut took the `.N` suffix invariant 13 requires: `contracts/cli/b10x-harness/2026-08-30.2/` was
  created by `6889741`, authored 2026-08-30 14:50 +0200.
- **measured against its real predecessor.** `contracts/cli/b10x-harness/2026-09-02/README.md:8`
  reads "What changed since `2026-09-01`", and `2026-09-01` is the directory immediately before it
  in `contracts/cli/b10x-harness/`.
- **effective defaults recorded.** `contracts/cli/b10x-harness/2026-09-02/README.md:63` states that
  `--wire` and `--session-dir` record `"default": null` while the binary applies one after clap;
  `:136`, `:139` and `:142` carry the per-command correction rows, and `:318`, `:320` and `:323`
  record `openai-responses` as `--wire`'s effective default on `chat`, `run` and `workflow run`.
  `contracts/cli/b10x-harness/2026-09-02/argv.json` agrees: `--wire` carries `"default": null` on
  `run`, `chat` and `workflow run`, and `"default": "openai-responses"` on `app-server`.

The pin in force is `2026-09-02` (`crates/harness-cli/src/contract.rs:34`).

Not re-run here: the contract checkers themselves. This triage read files and commits; it built
nothing (the checkout was under the disk floor for a workspace build).
