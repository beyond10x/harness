---
format: aep.planning-md/1
id: story:executable-gate-and-contract-checkers-run-in-rust
kind: story
status: implemented
title: Touched gate and contract checks run from Rust
summary: The next change retires executable Bash and Python sources it touches.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

The CLI checker and gate were materially changed while remaining Python and Bash, contrary to the organisation rule that anything which runs is Rust and an existing executable moves on its next touch. Provider contract hardening would otherwise extend the same debt.

## Acceptance

A Rust xtask or workspace binary owns gate orchestration and the CLI/provider contract checks changed by this wave. CI invokes that source of truth; compatibility wrappers, if retained temporarily, contain no independent logic and have a recorded retirement path. Self-tests still prove planted bad fixtures fail.
