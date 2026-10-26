---
format: aep.planning-md/3
id: story:approver-delegates-to-caller
kind: story
status: implemented
title: A run under metaharness hands each approval to its caller
summary: 'ADR 0104 points 2-3: --approve caller asks the spawning process over stdout/stdin; standalone modes unchanged'
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: AGENTS.md
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: README.md
- confidence: inferred
  path: contracts/cli/b10x-harness
- confidence: cited
  path: crates/harness-cli/src/approve.rs
- confidence: cited
  path: crates/harness-cli/src/lib.rs
- confidence: cited
  path: crates/harness-cli/tests
- confidence: inferred
  path: website/docs/reference/cli.md
revision: 12
transitions:
- {from: "draft", to: "proposed", at: "2026-10-26T10:45:04Z", actor: "human:timo", revision: 10}
- {from: "proposed", to: "active", at: "2026-10-26T10:45:05Z", actor: "human:timo", revision: 11}
- {from: "active", to: "implemented", at: "2026-10-26T11:43:38Z", actor: "human:timo", revision: 12, decided_on: {"recorded":{"test_result":1,"review_outcome":1}}}
---
## Context

Atlas ADR 0104 (accepted 2026-10-23, option A) point 2: "metaharness (L2) is the one layer that
decides per-call tool permission." Point 3: "L1 enforces and does not decide. A harness running
under metaharness hands each approval to metaharness: harness's approver seam calls back over the
steering channel [...] harness's own `--approve` modes stay for a standalone run, where no L2
exists." Options table, row A: "harness's approver gains a delegated mode".

Today the command line's approvers are `auto`, `prompt` (a person over `/dev/tty`,
`crates/harness-cli/src/approve.rs`), `deny` and `all` (`crates/harness-cli/src/lib.rs`,
`approver`). metaharness spawns `b10x-harness run --json` with no terminal, so `auto` resolves to
`DenyAll` and nothing above harness can approve a call (metaharness
`crates/metaharness-b10x/src/launch.rs`, the `--approve-up-to` comment). The atlas alignment audit
of 2026-10-26 § C lists this gap.

No Loom-side decision is needed: point 4 (Loom sets the policy) is Loom's and already shipped there
(`story:tool-decisions-read-back`); the seam between L1 and L2 is harness's command line, which this
repository owns and pins (`contracts/cli/`).

## Outcome

`b10x-harness run --json --approve caller` hands every approval to the process that spawned it:
the `approval-required` event on stdout is the request, and one JSON line on stdin is the
decision. The loop still asks before the effect (invariant 12 and the safety envelope stay), and
the library default stays `DenyAll`.

## Contract

- Request: the existing `approval-required` event (`call_id`, `name` = the catalogue entry),
  flushed before harness blocks; the `tool-requested` event with the same `call_id` carries the
  arguments. No new event kind.
- Decision: one line on stdin, `{"call_id":"<id>","decision":"allow"}` or
  `{"call_id":"<id>","decision":"deny","reason":"<non-empty>"}`. The vocabulary is a copy of
  metaharness's `Decision` (`allow`, `deny`), never an import (invariant 15's rule).
- Fails closed, each by name in the refusal the model sees: a line whose `call_id` is not the call
  asked about, an unparseable line, an unknown `decision` (`replace` and `abstain` included: how a
  rewrite reaches L1 is ADR 0104's open question), a `deny` without a reason, a line over the bound.
- End of stdin: this call and every later one is denied without reading, said once on stderr.
- Refused before the run, by name: `--approve caller` without `--json`; `--approve caller` on `chat`
  (stdin carries the questions there); `--yes` together with `--approve caller`.

## Acceptance

Tests in `crates/harness-cli` that drive the shipped binary against the deterministic local
endpoint, under `--approve caller --json`: an `allow` line lets a medium-risk write happen; a
`deny` line refuses it and the model receives the reason; a mismatched `call_id`, a malformed line
and a closed stdin each refuse the call; `--approve caller` without `--json` and on `chat` exit with
the refusal before any turn. Plus unit tests of the decision parser. A new
`contracts/cli/b10x-harness/<date>/` version generated from clap pins the new value;
`cargo xtask gate` exits 0.

The ESS specification's slice is the library boundary; CLI approvers are in
`spec/COVERAGE.md` § UNMAPPED "CLI profiles and contracts", so this story's acceptance is Rust tests
and the CLI contract, not ESS scenarios.

## Out of scope

- Rewrite (`replace`) of a call's input: ADR 0104 § Open.
- metaharness's side (launching with `--approve caller`, answering from the caller's policy): a
  metaharness story.
- A deadline on the decision: the deciding layer owns its window.
