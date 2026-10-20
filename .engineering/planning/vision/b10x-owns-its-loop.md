---
format: aep.planning-md/3
id: vision:b10x-owns-its-loop
kind: vision
status: approved
title: The b10x agent loop is owned here, and nothing above it is embedded
summary: A harness that owns its loop, makes its own model calls through llm's client crates, sits on four pinned foundations (substrate, mcp, llm, tools), and is embedded by others rather than embedding them.
revision: 8
transitions:
- {from: "draft", to: "in_review", at: "2026-09-15T17:27:43Z", actor: "human:timo", revision: 4, imported: true}
- {from: "in_review", to: "approved", at: "2026-09-15T17:27:43Z", actor: "human:timo", revision: 5, imported: true}
---
## Evidence

Line citations re-read on 2026-10-20 at wave llm-port commit `62f44050`.

- `README.md:3-4` — "The b10x agent loop — ours, not a vendor's. It makes its own model calls, through `llm`'s client crates, and owns the cycle: one turn out, tool calls back, results in, next turn out."
- `README.md:13-16` — the problem it removes: driving a vendor's harness means booting a vendor binary, registering tools through that vendor's mechanism, and living with that vendor's budgets.
- `README.md:18-29` — four foundations below it in the collection (substrate, mcp, llm, tools), each pinned by git tag or exact revision, never by a sibling path, and nothing that could embed it; "the arrow points inward".
- `README.md:35` — metaharness observes this component by launching `b10x-harness run` and reading its `--json` record; observed, not driven.
- `AGENTS.md:15-17` — the three objectives this repository serves, by id from the collection's roadmap: **O1** governed reach, **O3** any harness observed and compared, **O6** self-improvement from filed sessions.
- `AGENTS.md:24-25` — what it owns: "turn assembly, tool round trips, approvals, budgets. A harness that makes its own model calls, through `llm`'s client crates, rather than driving someone else's loop."
- `AGENTS.md:33-51` — invariants 1 and 2: no bridges to vendor binaries; no dependency on any sibling that could embed this; the foundations below it are pinned by git tag or revision, never by path.
- `AGENTS.md:131-132` — invariant 18: evidence from the deterministic local endpoint is `provider_emulated` and is never promoted to `vendor_live`.
- `STATUS.md:3` — observed 2026-09-25 at `0.13.3`, re-read after `0.14.1` (2026-10-20).
- Git history at the first approval (2026-09-15): 77 commits between 2026-08-17 and 2026-08-29, three authors, one tag (`0.1.0`), zero reverts.

## Context

What this repository is for is stated in three places that agree: the README's opening,
`AGENTS.md` § *What this repository owns*, and the objective ids in `AGENTS.md` § *Serves*. The
boundary is stated as invariants rather than as taste — the component may be embedded, and may
embed nothing above itself, which is what keeps the collection's components separable.

Since atlas ADR 0097 (2026-10-20) the loop is still owned here, but the model calls are made
through `llm`'s client crates and the default tool implementations come from `tools`; compaction,
the mid-stream retry, the run ledger and the approvals stay here. "Directly" in the earlier wording
meant no vendor binary and no vendor SDK between the loop and the model API; that still holds, and
the wording now says it that way.

The distinguishing commitment is about **evidence**, not about features: a claim made from the
local emulator may never be worded as a claim about a real provider (`AGENTS.md:131-132`), and each
published interface is pinned by a dated, immutable contract checked from both directions
(`AGENTS.md` § *Invariants*, invariant 13). Almost every open item in the plan below is a
consequence of that one rule.

## Standing — 2026-09-15

Recorded during triage of the draft backlog (ORG-0201). This objective sat in `draft` from the run
that reverse-engineered it until this pass, which made it indistinguishable from a scratch note
while most of the store declared that it `serves` it — 67 artifacts before this pass, 80 after it,
counted from `aep plan artifact list --format json`.

It is not a proposal. It restates commitments the repository already publishes and enforces —
`README.md`, `AGENTS.md`'s invariants, and the fact that the gate refuses a build that breaks them —
and the store has been acting on it: `epic:full-review-remediation` and
`epic:public-site-is-accurate-live-and-governed` are both `implemented` and both carry
`serves: vision:b10x-owns-its-loop`.

What had changed in the tree by then, without changing the objective:

- **something outside this repository holds the loop as a library.** `ROADMAP.md` records Agent
  Platform as the first embedder. "The arrow points inward" is a fact with an instance.
- **the two live routes exist.** `ROADMAP.md` states the credential phase's exit as "Both met".
- **the emulator rule has not moved.** Every provider-wire pin in `contracts/` is still
  emulator-derived (`STATUS.md`).

Moved to `approved` on that basis.

## Revision — 2026-10-20

Revised for 0.15.0 under atlas ADRs 0096–0097: the summary and evidence no longer say harness
"depends only on substrate" or "talks to LLM APIs directly"; the foundations are substrate, mcp,
llm and tools; line citations re-read at `62f44050`. The objective is unchanged.
