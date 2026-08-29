---
format: aep.planning-md/1
id: story:answer-prose-rate-measured
kind: story
status: implemented
title: How often a real model ends in prose under answer is a number
relations:
- derived_from: epic:measured-not-emulated
- serves: vision:b10x-owns-its-loop
revision: 5
---
## Evidence

- `STATUS.md:13` — "`--output-schema` publishes the schema as a tool the model calls to finish; a model that ends in prose is told once, then the run stops `unstructured` (exit 2)"; next evidence: "one live run per feature; the first measurement is how often a real model ends in prose under `answer` (design 0002 M2)".
- `docs/design/0002-sub-agents-structured-output-hooks.md:55-60` — the shipped mechanism is the schema published as a tool, and "Provider-native constrained decoding behind the *same* `OutputSchema` value is **milestone M2**, cut as new contract versions when a live run shows the tool path failing to adhere".
- `ROADMAP.md:209-211` — "**What is not reached**, and what the next evidence is: one live run per feature — how often a real model ends in prose under `answer` is the measurement that decides whether provider-native constrained decoding (M2) is cut as new contract versions."

## Context

The decision waiting on this number is a contract change: M2 would add provider-native constrained
decoding to both wires, which by invariant 13 means new dated contract versions on both. That is
weeks of work that the tool-call path may make unnecessary, and the only thing that says which is the
rate at which a real model ends its run in prose instead of calling `answer`.

The exit path already exists and is observable without new instrumentation: a run that ends in prose
stops `unstructured` with exit 2, after exactly one nudge.

## Acceptance

A recorded rate — prose endings per run under `--output-schema`, over enough live runs to be worth
quoting — with design 0002's M2 either cut as work or closed against it.
