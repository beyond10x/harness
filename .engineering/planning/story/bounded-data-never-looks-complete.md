---
format: aep.planning-md/1
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
---
## Defect

Oversized bridge results remain successful, skill files are loaded without an early limit, search silently omits large files, file_read can exceed max_bytes without marking truncation, and JSON exchange reads exactly the limit without checking for an additional byte.

## Acceptance

Each boundary reads at most limit plus one, then either refuses by the bound's stable name or returns an explicit incomplete marker that the model sees. Bridge overflow is a failed outcome. Skill, search, file_read, and exchange tests cover limit minus one, exact limit, multibyte text, and limit plus one without unbounded allocation.
