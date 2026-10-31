---
format: aep.planning-md/3
id: approval-record:wave-live-pins-d200
kind: approval-record
status: draft
title: Wave live-pins approved by the orchestrator as D200
relations:
- decides: story:compaction-measured-live
- decides: story:messages-cache-breakpoint-live-pin
- decides: story:responses-pin-from-live-bytes
- decides: story:flat-surface-measured
revision: 1
---
## The stop

Wave `live-pins` (integration branch `wave/2026-10-29-live-pins`): `story:compaction-measured-live`
(active) together with three proposed stories, `story:messages-cache-breakpoint-live-pin`,
`story:responses-pin-from-live-bytes` and `story:flat-surface-measured`, whose live provider runs
had not been authorised.

## Who decided

Session `b10x-chef-orchestrator`, 2026-10-29, as D200, under the owner's grant for class-C items in
`~/beyond10x/CLAUDE.md`. Verbatim:

> compaction-measured-live, plus the live-pin/measure stories (messages-cache-breakpoint-live-pin,
> responses-pin-from-live-bytes, flat-surface-measured) — I accept those three for this wave;
> record that acceptance in the store as an approval-record citing D200. Live provider runs are
> allowed on a subscription login (Claude or Codex/ChatGPT OAuth, as the repo's tooling supports;
> Timo 2026-10-20: subscription quota is not spending). If a run needs a pay-per-token API key,
> that is spending: do not use one; file a decision-blocker with `Decider: operator` for that
> story and drop it from the wave. Do not take repo-local-profiles (needs a trust decision).

## What it authorises

- The four stories in one wave, one integration branch, one pull request through the bot, one
  release.
- Live provider runs through the built-in `claude` provider (the Claude subscription login,
  `oauth-token-file`) and the `codex` profile (the Codex login). No pay-per-token key is used.
- Not authorised: `story:repo-local-profiles`, any move of the ESS pin (D198), any credential
  acquisition.

## Decision taken inside the item

The two wire-pin stories ask for a new dated directory under `contracts/provider-wires/`. Invariant
14 in `AGENTS.md` says no new provider-wire version is cut in this repository since the wire
crates moved to `llm` (atlas ADR 0097): a change to what is sent or accepted is an `llm` change.
`llm` recorded both live streams under `story:live-provider-captures` (implemented 2026-10-21,
shipped in `llm` 0.16.0, the tag this workspace pins): `crates/llm-messages/tests/live_capture.rs`
replays a cache-write and a cache-read Messages turn, `crates/llm-responses/tests/live_capture.rs`
a Codex Responses turn. The two stories therefore move to `rejected` with that citation, after this
wave records a live run of this harness through each route, so the harness-side claim is observed
and not inferred.
