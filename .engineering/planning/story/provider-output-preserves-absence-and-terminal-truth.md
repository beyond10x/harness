---
format: aep.planning-md/1
id: story:provider-output-preserves-absence-and-terminal-truth
kind: story
status: implemented
title: Provider output is never invented from missing or contradictory fields
summary: Missing tool input remains missing and explicit terminal output wins over streamed drafts.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

Responses can replace explicit empty terminal output with accumulated streamed calls. Both wires default missing tool arguments to an empty object, changing malformed provider bytes into a valid invocation.

## Acceptance

Absent arguments produce a bounded failed model-visible outcome or a named projection refusal, never an invented object. Explicit terminal output, including an empty list, is authoritative and contradictory stream state is diagnosed. Tests cover scalar, missing, empty, duplicate-id, and terminal-versus-stream cases on both wires.
