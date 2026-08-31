---
format: aep.planning-md/1
id: story:provider-terminal-and-retry-semantics
kind: story
status: implemented
title: Provider streams stop at terminal events and retries honor server delay
summary: Messages does not drain past completion and bounded Retry-After affects backoff.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

The Messages decoder records message_stop but continues reading until EOF, which can hang on an open connection and accept bytes after completion. HTTP discards Retry-After and always uses its fixed schedule.

## Acceptance

message_stop terminates decoding immediately and refuses unfinished content blocks. Error terminal events do likewise. Retry-After delta seconds and valid dates are parsed at the transport boundary, bounded by policy, and combined with local backoff deterministically; malformed or excessive values fall back safely. Tests cover an open post-terminal stream and retry timing.
