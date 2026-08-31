---
format: aep.planning-md/1
id: story:tool-return-is-not-process-success
kind: story
status: implemented
title: A returned tool outcome is not labelled process success
summary: Human progress rendering says returned when a tool completed without a failed outcome, preserving the distinction from an argv exit status.
relations:
- derived_from: epic:tracking-documents-current
- serves: vision:b10x-owns-its-loop
revision: 5
---
## Defect

The human renderer prints `← ok` for every non-failed `ToolCompleted` event. The run tool can successfully return an observation whose child exit status is nonzero, so `ok` visually claims more than the event states.

## Acceptance

Human and delegated progress render `← returned` for a non-failed tool outcome and retain `← failed` for a failed one. JSON events and their `failed` boolean are byte-for-byte unchanged. Unit and shipped-binary E2E assertions pin both paths.
