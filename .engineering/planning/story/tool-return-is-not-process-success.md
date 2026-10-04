---
format: aep.planning-md/3
id: story:tool-return-is-not-process-success
kind: story
status: implemented
title: A returned tool outcome is not labelled process success
summary: Human progress rendering says returned when a tool completed without a failed outcome, preserving the distinction from an argv exit status.
relations:
- derived_from: epic:tracking-documents-current
- serves: vision:b10x-owns-its-loop
revision: 5
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T08:08:12Z", actor: "human:timo", revision: 3, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T08:08:12Z", actor: "human:timo", revision: 4, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T08:58:17Z", actor: "human:timo", revision: 5, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Defect

The human renderer prints `← ok` for every non-failed `ToolCompleted` event. The run tool can successfully return an observation whose child exit status is nonzero, so `ok` visually claims more than the event states.

## Acceptance

Human and delegated progress render `← returned` for a non-failed tool outcome and retain `← failed` for a failed one. JSON events and their `failed` boolean are byte-for-byte unchanged. Unit and shipped-binary E2E assertions pin both paths.
