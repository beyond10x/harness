---
format: aep.planning-md/1
id: epic:measured-not-emulated
kind: epic
status: draft
title: The behaviours that only the emulator has ever seen are measured on something real
summary: Compaction, the answer path, the flat surface and the embedded driver's exec.
relations:
- decomposes: initiative:live-evidence
revision: 2
---
## Evidence

- `STATUS.md:12` — loop: "measure a compaction summary against a real provider — the trigger, the ratio and the summary prompt are all `provider_emulated`".
- `STATUS.md:13` — loop-owned tools: "one live run per feature; the first measurement is how often a real model ends in prose under `answer` (design 0002 M2)".
- `STATUS.md:14` — workspace tools: "measure what the flat surface costs or saves on a real provider. The three verbs' 33–44% discovery overhead is measured; the flat surface's schema-validation behaviour is not".
- `STATUS.md:23` — substrate: "the embedded driver's exec is still unexercised on this machine, which needs the same delegated scope".
- `docs/reviews/2026-08-29-code-review-2.md:38` — "Still open: the embedded driver's exec is unexercised on this machine (needs the same delegated scope)".
- `docs/design/0002-sub-agents-structured-output-hooks.md:59-60` — provider-native constrained decoding is milestone M2, "cut as new contract versions when a live run shows the tool path failing to adhere". The measurement decides whether M2 happens at all.
- `docs/reviews/2026-08-29-sota-comparison.md:62` — the 33–44% discovery overhead figure the flat surface was built to remove.

## Outcome

Four behaviours that exist, pass their own tests and have never met a real model or a real kernel
each get one measurement, and the measurement decides what happens next rather than an argument
about it.

## Scope

Compaction under a real context window; the `answer` path's prose rate; flat versus verbs on a real
provider; and one confined exec through the embedded driver in a delegated cgroup scope.

## Out of Scope

Building an evaluation harness. Three of these are one run each with the figures kept; the eval
that compares arms lives in another repository (`README.md:23`).

## Done When

Each of the four has a retained figure, and the design decisions waiting on them — design 0002's
M2, and whether `verbs` stays the default anywhere — are taken against it.
