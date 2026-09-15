---
format: aep.planning-md/1
id: vision:b10x-owns-its-loop
kind: vision
status: approved
title: The b10x agent loop is owned here, and nothing above it is embedded
summary: A harness that talks to model APIs directly, depends only on substrate, and is embedded by others rather than embedding them.
revision: 6
---
## Evidence

- `README.md:3-4` — "The b10x agent loop — ours, not a vendor's. It talks to LLM APIs directly and owns the cycle: one turn out, tool calls back, results in, next turn out."
- `README.md:9-12` — the problem it removes: driving a vendor's harness means booting a vendor binary, registering tools through that vendor's mechanism, and living with that vendor's budgets.
- `README.md:14-17` — one dependency in the collection (substrate, pinned by git revision) and nothing that could embed it; "the arrow points inward".
- `README.md:23` — metaharness observes this component by launching `b10x-harness run` and reading its `--json` record; observed, not driven.
- `AGENTS.md:16-18` — the three objectives this repository serves, by id from the collection's roadmap: **O1** governed reach, **O3** any harness observed and compared, **O6** self-improvement from filed sessions.
- `AGENTS.md:25-26` — what it owns: "turn assembly, tool round trips, approvals, budgets. A harness that talks to LLM APIs **directly** rather than driving someone else's."
- `AGENTS.md:34-42` — invariants 1 and 2: no bridges to vendor binaries; no dependency on any sibling that could embed this; substrate is the one dependency below it, pinned by revision, never by path.
- `AGENTS.md:116-117` — invariant 18: evidence from the deterministic local endpoint is `provider_emulated` and is never promoted to `vendor_live`.
- `STATUS.md:3` — observed 2026-08-29 at `0.1.0` plus the substrate pin, the second wire and the SOTA-comparison wave.
- Git history: 77 commits between 2026-08-17 and 2026-08-29, three authors, one tag (`0.1.0`), zero reverts and zero commits hedged with *for now* / *temporarily* / *until we*.

## Context

This repository is twelve days old and has never backed a decision out. What it is for is stated in
three places that agree: the README's opening, `AGENTS.md` § *What this repository owns*, and the
objective ids in `AGENTS.md` § *Serves*. The boundary is stated as invariants rather than as taste —
the component may be embedded, and may embed nothing above itself, which is what keeps the
collection's components separable.

The distinguishing commitment is about **evidence**, not about features: a claim made from the
local emulator may never be worded as a claim about a real provider (`AGENTS.md:116-117`), and each
published interface is pinned by a dated, immutable contract checked from both directions
(`AGENTS.md:81-104`). Almost every open item in the plan below is a consequence of that one rule —
the code exists, and the evidence that would let it be described as working against something real
does not yet.

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

What has changed in the tree since this was drafted, without changing the objective:

- **something outside this repository now holds the loop as a library.** `ROADMAP.md:179-187`
  records Agent Platform as the first embedder, pinning three of these crates at tag `0.10.0`.
  "The arrow points inward" is now a fact with an instance, not only a rule.
- **the two live routes exist.** `ROADMAP.md:174-175` states the credential phase's exit as "Both
  met".
- **the emulator rule has not moved.** Every provider-wire pin in `contracts/` is still
  emulator-derived (`STATUS.md:23`), which is the objective's distinguishing commitment still being
  paid for rather than waived.

Moved to `approved` on that basis. Nothing here was re-measured; the citations above are file reads
in this worktree and statuses read from this store.
