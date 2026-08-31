---
format: aep.planning-md/1
id: story:cancellation-aborts-provider-io
kind: story
status: implemented
title: Cancellation actively aborts provider connection and stream I/O
summary: A cancelled run exits promptly even when send or SSE read is silent.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

Cancellation is checked around blocking operations, but the blocking send and read can remain inside the HTTP timeout for minutes. Dropping the body is promised but cannot occur until the call returns.

## Acceptance

Request establishment and streaming reads race the shared cancellation token and actively tear down their connection. Silent-server tests cover cancellation during connect-or-headers and after an SSE response begins, with a small deterministic upper bound and no leaked worker. Timeout and cancellation remain distinguishable.
