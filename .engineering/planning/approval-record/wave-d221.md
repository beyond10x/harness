---
format: aep.planning-md/3
id: approval-record:wave-d221
kind: approval-record
status: draft
title: Wave d221 approved by the orchestrator as D221
relations:
- decides: story:substrate-refusal-display-escapes-daemon-bytes
- decides: story:end-to-end-suite-leaves-no-fixture-binary
- decides: story:loop-stop-is-a-tagged-union
revision: 1
---
## The stop

Wave `d221` (integration branch `wave/2026-10-29-d221`, page `docs/plans/2026-10-29-wave-d221.md`):
`story:substrate-refusal-display-escapes-daemon-bytes`, `story:end-to-end-suite-leaves-no-fixture-binary`
and `story:loop-stop-is-a-tagged-union`, with ESS 0.77.0 and `libz-sys` 1.1.31 riding along.

## Who decided

Session `b10x-chef-orchestrator`, 2026-10-29, as D221, under the owner's grant for class-C items in
the workspace instructions. The decision as recorded in the orchestrator's decision log, verbatim:

> harness wave of stories needing no live credentials/spending + libz-sys 1.1.31. Each moves ESS to
> 0.77.0 (touched anyway).

The dispatch, verbatim in part:

> Take only stories that run without a paid API key, the operator's credentials or a device.
> Subscription logins already on this machine are allowed, and so is the local GPU.
> Accept each story you take through the store with an approval-record citing D221.
> Ride-along: libz-sys 1.1.31 in Cargo.lock [...] and the ESS crates and pin to 0.77.0.
> Release through the repository's own process.

## Decision taken inside the item

The dispatch named six candidates. On `main` at `90422fc` five of them were no longer open:
`story:daemon-exec-start-adoption`, `story:compaction-measured-live` and `story:flat-surface-measured`
are `implemented`; `story:messages-cache-breakpoint-live-pin` and `story:responses-pin-from-live-bytes`
are `rejected` (both under `approval-record:wave-live-pins-d200`). `story:real-bridge-drives-this-binary`
is blocked by the open `dependency-blocker:platform-runner-admits-a-non-codex-child`. The coordinator
took instead the three drafts above, which need no credential, spending or device, matching D221's
own wording, after one plan-critic round (acceptance and parallel-safety) whose findings were
applied to the drafts.

`story:repo-local-profiles` is not taken. The dispatch asked for a decision-blocker on its trust
question; `decision-blocker:repository-profile-trust-policy` already exists and was `cleared` on
2026-10-23 with the orchestrator's answer, so no second one is filed.

## What it authorises

- The three stories in one wave, one integration branch, one pull request through the bot, one
  release (`0.24.0`) through `AGENTS.md` § Releases.
- Not authorised: any live provider run, any credential, `story:repo-local-profiles`,
  `story:real-bridge-drives-this-binary`, `story:direct-provider-adapter`.
