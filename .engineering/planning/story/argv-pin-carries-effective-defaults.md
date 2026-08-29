---
format: aep.planning-md/1
id: story:argv-pin-carries-effective-defaults
kind: story
status: draft
title: The argv pin says what a flag does when it is left out
relations:
- derived_from: epic:pinned-interfaces-honest
revision: 2
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
