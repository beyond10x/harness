---
format: aep.planning-md/3
id: approval-record:wave-llm017-d204
kind: approval-record
status: approved
title: Wave llm017 approved by the orchestrator as D204
relations:
- decides: story:responses-replays-no-stream-metadata
- decides: story:verbs-survives-a-bare-entry-call
- decides: story:failed-run-ends-with-a-finished-line
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-10-29T12:48:10Z", actor: "human:timo", revision: 2}
---
## The stop

Two draft stories filed from the live runs of 2026-10-29 (D200),
`story:responses-replays-no-stream-metadata` and `story:verbs-survives-a-bare-entry-call`, whose
fix is in `llm`, and the final-line fix `1012e54` (`story:failed-run-ends-with-a-finished-line`),
which sat on wave branch `wave/2026-10-26-llm016-final-line` and never reached main.

## Who decided

Session `b10x-chef-orchestrator`, 2026-10-29, as D204 (option A of the D200 report, taken as
D202), under the owner's grant for class-C items in `~/beyond10x/CLAUDE.md`. Verbatim:

> one harness wave: (1) pin llm 0.17.0 and set return_unpublished_calls so the loop answers an
> undeclared call as a refusal (invariant 9); close harness drafts
> responses-replays-no-stream-metadata and verbs-survives-a-bare-entry-call by it; (2) land the
> final-line fix 1012e54 from wave/2026-10-26-llm016-final-line (cherry-pick or merge onto the
> integration branch; that wave's tree stays as is otherwise); (3) re-run the Codex tool-using and
> the verbs-surface measurements on subscription logins and record them. Up to 4 units. One local
> full gate, one PR via b10x-gates bot, auto-merge, one release, verify-release.sh, announcement
> file. Same rules as D200 (D198: ESS pin stays; Rust only; own target/ per tree; no personal
> tokens; no fixed sleeps). Report the tag.

## What it authorises

- The three stories in one wave on `wave/2026-10-29-llm017`, one pull request through the bot,
  one release.
- The `llm` pin moves from 0.16.0 to 0.17.0 (a dependency upgrade).
- Live provider runs through the built-in `claude` provider (the Claude subscription login) and
  the `codex` profile (the Codex login). No pay-per-token key is used.
- Not authorised: any move of the ESS pin (D198).

## Decision taken inside the item

The final-line fix lands by a bot merge of `ea60f2b` (its adversary tests), whose only commits over
main are `1012e54` and `ea60f2b`; the bot refuses `cherry-pick`. The old wave's store records for
`story:failed-run-ends-with-a-finished-line` (story, evidence, review-result and
`approval-record:wave-llm-0-16-final-line`) are brought over from `e9ed062` unchanged.
