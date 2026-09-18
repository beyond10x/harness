---
format: aep.planning-md/1
id: story:caller-owned-memories
kind: story
status: implemented
title: Memories are caller-owned, read-only and refuse a vault they cannot read whole
summary: 'A Memories value beside Skills: typed frontmatter, trust closed to unreviewed, supersession by status flip, and a read-only recall tool with no writing counterpart.'
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: AGENTS.md
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: README.md
- confidence: cited
  path: STATUS.md
- confidence: cited
  path: contracts/cli/b10x-harness/2026-09-18
- confidence: cited
  path: crates/harness-cli/src/contract.rs
- confidence: cited
  path: crates/harness-cli/src/lib.rs
- confidence: cited
  path: crates/harness-cli/src/memories.rs
- confidence: cited
  path: crates/harness-cli/src/render.rs
- confidence: cited
  path: crates/harness-loop/src/event.rs
- confidence: cited
  path: crates/harness-loop/src/lib.rs
- confidence: cited
  path: crates/harness-loop/src/memory.rs
- confidence: cited
  path: crates/harness-loop/src/tests.rs
- confidence: cited
  path: website/docs/concepts/security-boundary.md
- confidence: cited
  path: website/docs/guides/structured-runs.md
- confidence: cited
  path: website/docs/reference/cli.md
- confidence: cited
  path: website/docs/status.md
revision: 5
---
## Context

`Skills` already proves the shape this repository wants for operator-supplied text: a value the
caller constructs before the run, cloned into delegates, with descriptions in the standing
instruction and bodies behind one enumerated tool call. A memory vault is the same shape with a
harder trust question, because a vault is text that accumulates across runs rather than text an
operator wrote once and reviewed.

The loop must therefore walk no directory and read no file. `--memory-dir <DIR>`, repeatable on
`run`, `chat`, `workflow run` and `tools`, is the whole of how a memory reaches a run: no default
vault, no `$XDG` location, no environment variable, no walk up the tree beside the workspace, no
`--plugin-dir` arm and no profile key. Without the flag a run has no memories, publishes no `recall`
tool, and is byte-identical to one built before this existed.

Two differences from `Skills` are the point of the story rather than incidental to it.

`Memories::new` is **fallible** where `Skills::new` is not. A vault silently missing the record that
mattered reads to the model exactly like a complete vault, so a truncated or partially-invalid vault
is refused **by name** and emits **0 bytes** — never a smaller vault.

`Recall` returns four answers rather than `Option`: `Body`, `Superseded { by }`, `Rejected` and
`Absent`. A model that asks for a record the operator retired is told what became of it, which is
a different fact from the record never having existed.

## Acceptance

A run given `--memory-dir` publishes a read-only `recall` tool whose `id` argument is a schema
`enum` over the active records, carries each record's `summary` in the standing instruction, and
returns a body only on a call; a run given no `--memory-dir` publishes no such tool. A vault
containing an unknown frontmatter key, an unreadable record, an empty `summary`, a duplicate id, a
dangling `supersedes`, or a bidirectional override or control codepoint in any field read by a
person refuses the whole vault by name, emitting no records at all. `LoopEvent::Started` carries
`memories` — the ids of every record handed in, whatever its status, always present and empty
rather than absent — and `b10x-harness tools` answers with the same list. A delegate observes its
parent's vault entry for entry. `cargo xtask gate` is green.

## The trust model, and why `trust` has one value

`trust` is an enum whose only legal value is `unreviewed`. A vault record is unreviewed context.
Governed truth is promoted **out** of a vault into an artifact with its own review rather than
marked trusted in place, so nothing a run reads can raise its own standing by asserting it. `status`
is `active`/`rejected`/`superseded`, and flipping it is the only thing that ever happens to a
record: nothing is deleted, and a correction is a new record naming the old one in `supersedes`.
`kind` is a closed six-member enum with no catch-all.

Bidirectional overrides (U+202A–U+202E, U+2066–U+2069) and C0/C1 control codepoints are refused in
every field a person reads, naming the record, the field and the codepoint. A Trojan-Source
reordering would make a record read one way in an auditor's terminal and another to the parser.
Single-line fields additionally refuse the newline and tab a body may carry, because a `summary`
holding a newline and a list marker would forge an extra row in the instruction's memory list. This
refusal existed nowhere in this tree before.

## Deliberately out of scope: the writer

**No memory-writing tool and no writing flag ships here.** The shipped toolset stays read-only, and
every memory a run can see was handed to it before it started. A writer is its own change, with its
own gate, its own threat model and its own `STATUS.md` row — never a flag on the reader. Recording
that absence as a decision, rather than as an oversight, is part of this story's outcome.

Adding a fourth loop-owned tool is itself the kind of change `AGENTS.md` calls a design change; the
reviewed design for the writer is owed before any of it is built.

## Validation and boundaries

28 tests under `cargo xtask gate`: 23 unit refusal tests over the vault reader and 5 loop tests,
including a delegate observing its parent's set entry for entry. Nothing touches `harness-wire`,
no sibling dependency is added, and nothing becomes ambient — `recall` is resolved by the loop
before the tool port sees a call, exactly as `answer`, `delegate` and `skill` are.

The argv surface is **versioned, not edited**: `contracts/cli/b10x-harness/2026-09-18` is cut with
the four `--memory-dir` arrivals, and `2026-09-02` stays byte-identical to `origin/main` (invariant
13). The cut is strictly additive — four arrivals and no field of any surviving flag moved — so a
consumer pinned to `2026-09-02` is correct against this binary and needs to change nothing.

## Evidence class

`provider_emulated`. No real provider has yet been shown a `recall` tool, so nothing here says
anything about whether summaries in the standing instruction are enough to make a vault reachable.
That is the question `--skills-dir` answered by being used, and it is the next evidence this row
owes (invariant 18).
