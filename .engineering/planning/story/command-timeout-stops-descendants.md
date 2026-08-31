---
format: aep.planning-md/1
id: story:command-timeout-stops-descendants
kind: story
status: implemented
title: A timed-out local command leaves no descendant process running
summary: Timeout cleanup reaches the whole spawned process group and drains output boundedly.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

The local run tool kills only its direct child, then joins pipe readers. Descendants can retain the pipes and continue running, causing both an escaped effect and an unbounded join.

## Acceptance

Each invocation owns an isolated process group using safe Rust APIs. Timeout and cancellation terminate the group, wait for it, and bound output-drain completion. A regression command forks a descendant that holds stdout and proves both processes are gone and the tool returns a failed outcome promptly.
