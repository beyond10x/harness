---
format: aep.planning-md/1
id: story:bridge-interrupt-settlement-makes-progress
kind: story
status: implemented
title: Bridge interrupt settlement reaches the queued interrupt
summary: Unrelated stashed frames cannot be consumed and re-stashed forever.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

The settlement loop reads through a helper that prefers the stash, then places a non-request back into that stash. One unrelated notification can therefore prevent the reader from ever observing the queued interrupt.

## Acceptance

Settlement reads the underlying stream while temporarily preserving unrelated frames in order. A deterministic test begins with a stashed notification and a queued interrupt, proves bounded completion, and proves the notification is still delivered exactly once.
