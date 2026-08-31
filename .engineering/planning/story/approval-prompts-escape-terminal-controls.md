---
format: aep.planning-md/1
id: story:approval-prompts-escape-terminal-controls
kind: story
status: implemented
title: Approval prompts render model text as inert terminal text
summary: Paths, edits, and argv cannot inject terminal controls into the operator prompt.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

The terminal approver writes model-controlled invocation details directly to /dev/tty. Newlines, carriage returns, C0 bytes, and escape sequences can forge prompt lines or control the terminal.

## Acceptance

Every untrusted field is bounded and encoded into a single inert visible representation before rendering. Tests cover ESC, CSI, CR, LF, tabs, other C0 controls, truncation, and ordinary Unicode; the operator's answer parsing remains unchanged.
