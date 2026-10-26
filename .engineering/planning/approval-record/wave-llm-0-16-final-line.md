---
format: aep.planning-md/3
id: approval-record:wave-llm-0-16-final-line
kind: approval-record
status: approved
title: Wave llm-0-16-final-line approved by session b10x-chef-orchestrator
summary: llm 0.16.0 and the failed-run finished line, and the 0.21.0 release, 2026-10-26
relations:
- decides: story:harness-builds-on-llm-0-16
- decides: story:failed-run-ends-with-a-finished-line
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-10-26T21:05:42Z", actor: "human:timo", revision: 2}
---
## The stop

Wave 2026-10-26 llm-0-16-final-line: `story:harness-builds-on-llm-0-16` and
`story:failed-run-ends-with-a-finished-line`, and the 0.21.0 release after it. Wave page:
`docs/plans/2026-10-26-wave-llm-0-16-final-line.md`.

## Who decided

Session `b10x-chef-orchestrator`, 2026-10-26, under the operator's standing grant for class-C items
in organization repositories. A relayed approval, not a bypass. Verbatim:

> Approved under the grant in ~/beyond10x/CLAUDE.md (class C: wave, dependency upgrade, bot
> commits, PR, merge, release). Run as an aep:implementing wave [...]
> Unit 1, pin: llm 0.15.0 -> 0.16.0 [...] Add a harness-level test or conformance scenario that a
> streamed turn whose usage carries `cost` completes [...] Decide whether the harness surfaces the
> warning (log line, trace event) or drops it; record the decision in the story.
> Unit 2, defect: [...] fix the smallest thing: at minimum the run must end with a final JSON line
> naming the outcome (action or an error naming the cause: compaction, context overflow, provider
> error, child exit) [...] If the real cause is in llm or metaharness, file the story there as a
> draft and still ship the harness-side final line.
> Then release the next minor (0.21.0 unless AGENTS.md says otherwise) [...]

## Decisions the coordinator took inside the item

- Unit 1: the `usage-extension` warning is surfaced as a `warning` event, as every llm stream
  warning already is; not dropped (invariant 7).
- Unit 2: the terminal line is the existing `finished` event with a new stop kind `failed`, emitted
  by the loop itself on every `LoopError` exit, so every embedder gets it and metaharness's existing
  `finished` arm turns it into `session.ended` with `is_error` and `subtype: failed` without a
  metaharness change. The cause travels in `cause`, `message` and `after_compaction`.
- The two units collide only on `CHANGELOG.md`, which the coordinator writes at integration.

## What it authorises

Two unit commits (and their correction commits), the merges into
`wave/2026-10-26-llm016-final-line`, the opening and closing store commits, a release commit, the
pull request and its merge through the bot route, and the annotated tag `0.21.0`. No consumer pin
moves.
