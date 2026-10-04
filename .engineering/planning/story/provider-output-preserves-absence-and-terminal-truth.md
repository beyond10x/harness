---
format: aep.planning-md/3
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
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T01:56:27Z", actor: "human:timo", revision: 2, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T01:56:27Z", actor: "human:timo", revision: 3, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T03:29:36Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Defect

Responses can replace explicit empty terminal output with accumulated streamed calls. Both wires default missing tool arguments to an empty object, changing malformed provider bytes into a valid invocation.

## Acceptance

Absent arguments produce a bounded failed model-visible outcome or a named projection refusal, never an invented object. Explicit terminal output, including an empty list, is authoritative and contradictory stream state is diagnosed. Tests cover scalar, missing, empty, duplicate-id, and terminal-versus-stream cases on both wires.
