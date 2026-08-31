---
format: aep.planning-md/1
id: story:answer-schema-validated-locally
kind: story
status: draft
title: Answer arguments are validated locally against the requested schema
summary: Implement design 0002 milestone M3 so a permissive endpoint cannot complete with an answer outside --output-schema.
relations:
- serves: vision:b10x-owns-its-loop
revision: 1
---
## Context

`--output-schema` publishes the supplied object schema as the `answer` tool, but design 0002 §1
explicitly leaves local validation to milestone M3. A deterministic `provider_emulated` E2E probe
supplied a schema requiring only `{"answer": string}` with `additionalProperties: false`; the
endpoint returned `{"bytes":14,"file":"README.md","verdict":"ok"}`, and the loop accepted it and
exited `0` because it trusts arguments the provider accepted.

Provider-side validation is not a sufficient boundary for a permissive, buggy or emulated endpoint.
This story is the existing design milestone made visible in the governed backlog; it is separate
from the JSON refusal-record defect and needs an explicit dependency and bounds decision.

## Acceptance

An `answer` object that violates the supplied schema becomes a bounded failed `ToolOutcome` that the
model can correct, on both wires, while valid structured-output stdout and JSONL contracts remain
unchanged.

## Design constraints

Record the validator dependency and size/cost tradeoff. Validation failures must not echo unbounded
arguments. Cover mismatch and recovery on both provider-emulated wires. Preserve the current rule
that a non-object answer is an outcome rather than a run error.
