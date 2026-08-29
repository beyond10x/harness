---
format: aep.planning-md/1
id: initiative:live-evidence
kind: initiative
status: draft
title: Every claim rests on a real provider or a real machine, not the emulator
summary: 'Close the STATUS next-evidence column: live pins, an authorized run per route, and the measurements the emulator cannot produce.'
relations:
- serves: vision:b10x-owns-its-loop
revision: 2
---
## Evidence

- `AGENTS.md:116-117` — invariant 18: evidence from the deterministic local endpoint is `provider_emulated`, is never promoted to `vendor_live`, and no prose may imply a real provider was contacted.
- `STATUS.md:11` — Responses wire, next evidence: "characterize one authorized live endpoint and retain the evidence".
- `STATUS.md:12` — loop, next evidence: "measure a compaction summary against a real provider — the trigger, the ratio and the summary prompt are all `provider_emulated`".
- `STATUS.md:13` — loop-owned tools, next evidence: "one live run per feature".
- `STATUS.md:14` — workspace tools, next evidence: "measure what the flat surface costs or saves on a real provider".
- `STATUS.md:19` — Messages wire, next evidence: "**capture this route's bytes live** — `2026-08-29b` is still emulator-derived".
- `STATUS.md:20` — subscription auth, next evidence: "**renewal, and one authorized run on the ChatGPT/Codex route**".
- `STATUS.md:21` — live provider: one live run on 2026-08-23, and "the current pin is still emulator-derived".
- `STATUS.md:23` — substrate: "the embedded driver's exec is still unexercised on this machine".
- `ROADMAP.md:122-128` — a live pin is a **new dated version** cut from captured bytes, not an edit to an emulated one (invariant 18).
- `ROADMAP.md:145-155` — Phase 5 pairs embedding with "one explicitly authorized live run against a real gateway, retained as `vendor_live` evidence distinct from everything above it".
- `docs/design/0003-workflow-runner.md:241-242` — "Everything below is `provider_emulated` until one authorized run walks the projected `adp/default/2` under a real governor (invariant 18)".

## Context

Nine of the eleven rows in `STATUS.md`'s table carry a *next evidence* cell, and eight of those ask
for the same kind of thing: a run against something real, retained. That is not nine unrelated
tasks — it is one programme, because the repository has one rule (invariant 18) that makes emulated
evidence structurally unable to close any of them.

The live runs that have happened are two: `2026-08-23` against a ChatGPT/Codex endpoint
(`STATUS.md:21`) and `2026-08-29` against `api.anthropic.com` (`STATUS.md:20`). The first found a
real defect on turn one that the emulator could not — the whole workspace toolset was named
illegally for that wire — which is the argument for this initiative in one line.

## Done When

Each area whose *next evidence* cell asks for a live measurement either has it, retained as
`vendor_live` and pinned where a pin is what it asked for, or the cell is rewritten to say the
measurement was tried and what it cost.
