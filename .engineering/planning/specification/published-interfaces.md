---
format: aep.planning-md/1
id: specification:published-interfaces
kind: specification
status: draft
title: The three interfaces this component publishes, each pinned by a dated contract
summary: Provider wires, the app-server profile and the argv surface — what is pinned, where, and how each half is checked.
relations:
- informed_by: vision:b10x-owns-its-loop
revision: 2
---
## Evidence

- `AGENTS.md:81-86` — invariant 13 names all three: `contracts/provider-wires/<wire>/<version>/` pins one model API subset, `contracts/app-server-profile/<profile>/<version>/` pins the JSON-RPC format this harness serves, `contracts/cli/<product>/<version>/` pins the argv surface. "A change cuts a new version directory."
- `AGENTS.md:88-97` — released means reachable on `origin/main`; a second cut on one day takes a `.N` suffix.
- `AGENTS.md:98-104` — invariant 14: both halves must hold — a Python checker verifies the manifest against its fixtures, a Rust test verifies the code produces exactly those bytes or holds exactly those constants.
- `AGENTS.md:209-212` — the three checkers in the gate: `scripts/check-provider-wires.py`, `scripts/check-app-server-profile.py`, `scripts/check-cli-contract.py`.
- `README.md:410-412` — what each path holds, in the layout table.
- `contracts/provider-wires/openai-responses/2026-08-21/` and `2026-08-22/` — the Responses pins; `2026-08-22` adds the optional sampling fields (`.../2026-08-22/README.md:6`).
- `contracts/provider-wires/anthropic-messages/2026-08-29/` and `2026-08-29b/` — the Messages pins; `2026-08-29b` adds the rolling `cache_control` breakpoint on the last block of the last message (`.../2026-08-29b/README.md:10-41`) and is the one in force.
- `contracts/app-server-profile/codex-app-server-stdio-v2-dynamic-operation-tools-experimental/2026-08-21/README.md:43` — the pinned method inventory; `:83` — "What these checks do **not** catch".
- `contracts/cli/b10x-harness/2026-08-29`, `.1`, `.2`, `.3`, `2026-08-30` — the argv pins; `crates/harness-cli/src/contract.rs:34` names `2026-08-30` as the one in force, and `:303` is the failure message when the binary and the document disagree.
- `contracts/cli/b10x-harness/2026-08-30/README.md:41-54` — the fields the argv document pins, generated from `Cli::command()` and never written by hand.
- `STATUS.md:16` — the same three, with the state of each.

## Context

These are the only interfaces this component publishes to somebody else, and each is versioned by
date, immutable once pushed, and checked from both directions. The specification exists so the plan
store knows what is already contracted: work that changes any of them cuts a new dated directory
rather than editing one, and no story below may propose an edit in place.

Two properties are worth stating explicitly because they are easy to assume and false here:

- **A pin is not evidence of a live provider.** Every provider-wire fixture in the tree today was
  produced by this repository's own emulator (`AGENTS.md:116-117`, invariant 18), which is what
  `epic:wire-pins-from-live-bytes` exists to change.
- **The app-server pin only checks this side against itself.** Invariant 15 (`AGENTS.md:105-109`)
  says the method inventory is a copy of the client's, that nothing here compares the two, and that
  only running the real bridge can.

## Required behaviour

1. A change to what is sent, accepted or served cuts a new dated version directory; the previous one
   stays byte-identical (`AGENTS.md:81-97`).
2. Both halves of every pin pass in `scripts/gate.sh` (`AGENTS.md:98-104`, `:209-212`).
3. The argv document is generated from clap's definition, never hand-written (`AGENTS.md:100-104`).
4. Wire-visible identifiers in the fixtures — the `b10x_operation_search` tool name, the
   `b10x-emulated` model name — change only through a coordinated migration with an ADR, by cutting a
   new version (`AGENTS.md:169-175`).
