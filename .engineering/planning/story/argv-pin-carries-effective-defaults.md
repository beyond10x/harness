---
format: aep.planning-md/1
id: story:argv-pin-carries-effective-defaults
kind: story
status: implemented
title: The argv pin says what a flag does when it is left out
relations:
- derived_from: epic:pinned-interfaces-honest
- serves: vision:b10x-owns-its-loop
revision: 6
---
## Evidence

- `contracts/cli/b10x-harness/2026-08-30/README.md:51` — the pinned field's meaning: "`arguments[…][].default` | the value used when the flag is absent, or `null`".
- `contracts/cli/b10x-harness/2026-08-30/README.md:52` — "`arguments[…][].required` | whether omitting it is a parse error".
- `contracts/cli/b10x-harness/2026-08-30/argv.json` — `--wire` on `run`, `chat` and `workflow run` records `"default": null`.
- `crates/harness-cli/src/lib.rs:988-991` — the binary still defaults it, after clap: `fn wire(&self) -> Wire { self.wire.unwrap_or_default() }`, "defaulted last so a provider can set it and a typed flag can beat it".
- `crates/harness-cli/src/lib.rs:203-210` — `#[default] OpenaiResponses`, with the reason at `:201-202`: "The default is the wire this harness shipped with, so every existing invocation means what it did before."
- `contracts/cli/b10x-harness/2026-08-30/argv.json` — `--model` and `--base-url` record `"required": false`; `crates/harness-cli/src/lib.rs:1170-1174` refuses the run by name when neither a flag nor a profile supplies them.
- `AGENTS.md:98-104` — invariant 14: the pinned document is generated from clap's own definition, "because a hand-written one would be a second description of the command line that drifts from the first".

## Context

Before profiles, clap held every default and requirement, so a document generated from clap's
definition was a complete answer. Now the endpoint, the model, the wire and the credential can come
from a provider or a profile, and clap's answer is a partial one: `--wire` reads as having no default
when it has one, and `--model` reads as optional when omitting it fails unless a config supplies it.

A consumer — metaharness's `b10x` adapter is the one named at `README.md:23` — reads this document to
build an invocation. Both fields now mislead it in the direction of building a command line that does
not run.

This is not an argument for hand-writing the document (invariant 14 forbids that). It is a question
about what the generated document should record now that resolution happens in two places.

## Acceptance

For every flag whose effective default or requirement is decided after clap, the pinned document
either records the effective value or states in its *What is not pinned* section that it does not —
and a consumer building an invocation from the document alone gets a command line that runs.

## Scope

Derived 2026-08-30 by `story-scoper`. Every line is **cited** (read from the story or the tree) or
**inferred** (a reading that could be wrong).

- **Primary surface:** the CLI argv pin — `crates/harness-cli/src/contract.rs` and
  `contracts/cli/b10x-harness/` — cited (`AGENTS.md:98-104` names both halves)
- **Files:** `contracts/cli/b10x-harness/<version>/README.md` § *What is not pinned*, plus that
  directory's `argv.json` and `manifest.json` — cited, the acceptance names the section
- **Files:** `crates/harness-cli/src/contract.rs:102-128` (`flags()`, the emitter of `"default"` and
  `"required"`) and `:34` (`ARGV_CONTRACT_VERSION`) — inferred, required only if the story records
  the effective value rather than disclaiming it
- **Symbols:** `ARGV_CONTRACT_VERSION`, `contract::argv`, `flags`, `REQUIRED_ARGUMENT_KEYS` — cited
  for the first, inferred for the rest
- **Also likely:** `scripts/check-cli-contract.py:35-43` — inferred, a new per-flag key would have to
  be learned there and made optional for the six already-pinned versions
- **Also likely:** `CHANGELOG.md` — inferred, invariant 14 requires a re-pin to enter the changelog
- **Read, not written:** `crates/harness-cli/src/lib.rs` (`fn wire` at `:1030-1032`, `#[default]` at
  `:218`, refuse-by-name at `:1229`) — cited as evidence, inferred to be read-only
- **Documents:** the contract version `README.md` is the story's centre of gravity; `AGENTS.md` is
  cited but not amended — cited
- **Confidence:** high on the surface; medium on `contract.rs` specifically, because the acceptance
  offers two branches (record the effective value, or disclaim it in prose) and only the first
  touches the generator
- **Would collide with:** any unit touching the CLI argv pin — `crates/harness-cli/src/contract.rs`,
  `scripts/check-cli-contract.py`, or a `contracts/cli/b10x-harness/` version directory
- **Overlaps `story:cli-contract-dated-ahead`: yes, heavily.** Both cut or edit a
  `contracts/cli/b10x-harness/` version directory and both move `ARGV_CONTRACT_VERSION`
  (`contract.rs:34`). They cannot run in the same wave in either order.

Stale citations found while scoping: `lib.rs:988-991` and `:203-210` are `1030-1032` and `218` at
`c5bb2ed`. Where the generator would read an effective default from is unsolved: no seam was found
between `contract.rs` and `profile.rs`/`provider.rs`, and invariant 14 forbids hand-writing the
document.
