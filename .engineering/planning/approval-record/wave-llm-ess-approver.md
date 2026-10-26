---
format: aep.planning-md/3
id: approval-record:wave-llm-ess-approver
kind: approval-record
status: approved
title: Wave llm-ess-approver approved by session b10x-chef-orchestrator
summary: llm 0.15.0, ESS 0.69.2 and the ADR 0104 caller approver, and the release, 2026-10-26
relations:
- decides: story:harness-builds-on-llm-0-15
- decides: story:ess-pin-0-69-2
- decides: story:approver-delegates-to-caller
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-10-26T11:43:38Z", actor: "human:timo", revision: 2}
---
## The stop

Wave 2026-10-26 llm-ess-approver: `story:harness-builds-on-llm-0-15`, `story:ess-pin-0-69-2` and
`story:approver-delegates-to-caller`, and the release after it. Wave page:
`docs/plans/2026-10-26-wave-llm-ess-approver.md`.

## Who decided

Session `b10x-chef-orchestrator`, 2026-10-26, dispatch-queue item 7, under the operator's standing
grant for class-C items in organization repositories. A relayed approval, not a bypass. Verbatim:

> (1) harness pins llm 0.15.0 [...] Move the pin, regenerate what depends on it, full gate.
> (2) ADR 0104 point 2 [...]: file the story in the store, implement the approver callback as the
> ADR states (if the point needs a Loom-side decision first, file the story as blocked on it and
> say so; do not invent the contract). (3) ESS pin: move to the newest ESS release (0.69.2 today
> [...]) per the workspace rule. (4) Release per AGENTS.md § Releases [...].

## Decisions the coordinator took inside the item

- ADR 0104 point 2 needs no Loom-side decision: point 3 names the seam ("harness's approver seam
  calls back over the steering channel"), the seam is harness's own command line, and point 4
  (Loom's policy) is Loom's and shipped there. The story is built, not blocked.
- The channel is the process's own stdout (request, the existing `approval-required` event) and
  stdin (decision, one JSON line), under a new value `--approve caller` that requires `--json` and
  is refused on `chat`. Decision words copy metaharness's `allow` / `deny`; `replace` stays refused
  because ADR 0104 § Open leaves how a rewrite reaches L1 undecided.
- llm and ESS pins are one unit: both rewrite `Cargo.lock`.

## What it authorises

Two unit commits (and their correction commits), the merges into
`wave/2026-10-26-llm-ess-approver`, the opening and closing store commits, a release commit, the
fast-forward of `main` through the bot route, and the annotated tag the release check names. No
consumer pin moves.
