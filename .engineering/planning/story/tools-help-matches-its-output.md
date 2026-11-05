---
format: aep.planning-md/3
id: story:tools-help-matches-its-output
kind: story
status: active
title: The tools help and the tools output agree on the write scope
tags:
- from-finding
relations:
- informed_by: review-result:security-1104-u1-pass-1
- serves: vision:b10x-owns-its-loop
revision: 3
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T02:19:08Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-05T02:19:09Z", actor: "human:timo", revision: 3}
---
## Context

Noted by `review-result:security-1104-u1-pass-1` outside the reviewed diff, pre-existing. The
`--write-scope` help on `tools` (`crates/harness-cli/src/lib.rs:1205-1215` at `58c2d1a`) says "so
`tools` answers with it", but `tools` prints no scope, typed or default: the reviewer ran the
built binary directly, and its case `tools_describes_the_scope_a_typed_confined_run_applies` in
`crates/harness-cli/tests/security_1104_u1.rs` passes only because both outputs carry no scope.

## Outcome

The `tools` help and the `tools` output agree: either `tools` prints the write scope a run with the
same flags would apply, or its help stops saying it does.

## Acceptance

- A test runs `tools --substrate-embedded` with no `--write-scope` and with a typed one, and
  asserts the output names the scope in each case; or the help text no longer promises it, and
  `help_text_names_the_enforced_rule` pins the corrected sentence.
- `tools_describes_the_scope_a_typed_confined_run_applies` is rewritten to assert the scope
  itself rather than equality of two scope-free outputs.
