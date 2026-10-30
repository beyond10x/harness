---
format: aep.planning-md/3
id: approval-record:wave-d288
kind: approval-record
status: draft
title: Wave d288 approved by the orchestrator as D288
relations:
- decides: story:repo-local-profiles
- decides: story:bridge-interrupt-start-window-under-load
- decides: story:bridge-mode-interrupt-cases-are-deterministic
- decides: story:substrate-escape-is-injective
- decides: story:harness-loop-modeled-in-ess
revision: 1
---
## The stop

Wave `d288` (integration branch `wave/2026-10-30-d288`, page `docs/plans/2026-10-30-wave-d288.md`),
based on `main` at `4353586` (tag `0.24.0`, wave d221 landed by fast-forward under the same decision):

- U1 `story:repo-local-profiles`, on the terms of the cleared
  `decision-blocker:repository-profile-trust-policy`, salvaging a copy of the uncommitted work left in
  managed tree `harness-unit-repo-local-profiles` on 2026-10-23 (that tree stays untouched).
- U2 `story:bridge-interrupt-start-window-under-load` and
  `story:bridge-mode-interrupt-cases-are-deterministic`, one unit, because both change
  `crates/harness-cli/tests/bridge_mode.rs` and the same rate-threshold cases.
- U3 `story:substrate-escape-is-injective`.
- U4 `story:harness-loop-modeled-in-ess`.

Left out: `story:direct-provider-adapter` (open `dependency-blocker:consumer-adapter-run-is-the-consumers`)
and `story:real-bridge-drives-this-binary` (open `dependency-blocker:platform-runner-admits-a-non-codex-child`).
ESS stays at 0.77.0, the newest release on 2026-10-30 (`gh release list -R beyond10x/ess`). The llm pin
is not moved.

## Who decided

Session `b10x-chef-orchestrator`, 2026-10-30, as D288, under the owner's grant for class-C items in the
workspace instructions (owner, 2026-10-30: "dispatch next"). The answer, verbatim:

> Approved as D288 (decisions.md): A. Land PR 61 by ff of bot head 4353586, tag 0.24.0,
> verify-release, then the wave U1-U4 from it, release 0.25.0. PR 58 left as it is. U1 salvages a
> copy only and the source tree stays untouched. Record this in your store. Note: llm 0.20.0 (wave
> w24, in progress) adds a StreamEvent variant. If your wave moves the llm pin, add the match arm in
> harness-llm from_llm_event in the same wave. Otherwise leave the pin alone.

## Commits it authorises

One commit per unit and its corrections; the merges into `wave/2026-10-30-d288`; the opening and
closing store commits; the release commit `0.25.0`; one pull request to `main` through the bot,
landed by fast-forward of its green bot-authored head; the annotated tag `0.25.0`.
