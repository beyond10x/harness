---
format: aep.planning-md/3
id: approval-record:wave-2026-11-04-ess-stays-077
kind: approval-record
status: approved
title: 'Wave 2026-11-04: harness 0.27.0 keeps ESS 0.77.0'
relations:
- decides: story:harness-spec-runs-on-the-newest-ess
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-05T01:35:31Z", actor: "human:timo", revision: 2}
---
## The question

The 0.27.0 gate moved ESS to 0.86.0 in G0, as its terms asked where the pin was behind. Under
0.86.0, synthesis refuses 12 of the specification's generated wrong-state scenarios as
ESS-SYNTH-003 (`harness.loop.Loop/state/{AwaitingModel,AwaitingSummary,Failed,Stopped}/refuses/harness.loop.{CompleteTool,Deny,ResumeDenied}`),
so the conformance crate's complete-coverage check fails. The lead kept ESS 0.77.0 and asked
whether that stands.

## Who decided

Session `b10x-chef-orchestrator`, 2026-11-05, under the owner's grant for class-C items in the
workspace instructions. The answer, verbatim apart from its internal decision number:

> Keep ESS 0.77.0 for harness 0.27.0, as you did. Moving to 0.86.0 drops 12 generated scenarios
> (ESS-SYNTH-003, the wrong-state CompleteTool/Deny/ResumeDenied cases), and restoring them needs a
> specification change outside this wave. I accept story:harness-spec-runs-on-the-newest-ess: move
> it to active for the next harness wave. Record it in your store and commit the revert with G1b.
> Then run G2–G6 under the same terms. I am sending the 12 refusals to the ESS wave to triage, as a
> possible ESS defect.

## What follows

- 0.27.0 ships with `spec/ess-inputs.yaml` requiring ess 0.77.0 and the `ess-*` crates at the
  0.77.0 rev.
- `story:harness-spec-runs-on-the-newest-ess` is active, for the next harness wave.
