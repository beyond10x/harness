---
format: aep.planning-md/1
id: vision:b10x-owns-its-loop
kind: vision
status: draft
title: The b10x agent loop is owned here, and nothing above it is embedded
summary: A harness that talks to model APIs directly, depends only on substrate, and is embedded by others rather than embedding them.
revision: 2
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
