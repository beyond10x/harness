---
format: aep.planning-md/3
id: story:bounded-data-never-looks-complete
kind: story
status: implemented
title: Bounded tool, skill, search, read, and exchange data never looks complete
summary: Every overflow is refused or explicitly marked before the consumer can trust it.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T01:56:31Z", actor: "human:timo", revision: 2, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T01:56:31Z", actor: "human:timo", revision: 3, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T03:29:33Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Defect

Oversized bridge results remain successful, skill files are loaded without an early limit, search silently omits large files, file_read can exceed max_bytes without marking truncation, and JSON exchange reads exactly the limit without checking for an additional byte.

## Acceptance

Each boundary reads at most limit plus one, then either refuses by the bound's stable name or returns an explicit incomplete marker that the model sees. Bridge overflow is a failed outcome. Skill, search, file_read, and exchange tests cover limit minus one, exact limit, multibyte text, and limit plus one without unbounded allocation.
