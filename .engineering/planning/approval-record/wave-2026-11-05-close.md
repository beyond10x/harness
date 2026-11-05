---
format: aep.planning-md/3
id: approval-record:wave-2026-11-05-close
kind: approval-record
status: approved
title: Wave 2026-11-05 closes with U2 and U3; U1 moves to the next wave
relations:
- decides: story:harness-spec-runs-on-the-newest-ess
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-05T04:04:04Z", actor: "human:timo", revision: 2}
---
## The question

Whether wave 2026-11-05 closes before its U1 (`story:harness-spec-runs-on-the-newest-ess`), which
waits on ESS 0.87.0 and the ESS triage verdict on the 12 ESS-SYNTH-003 refusals.

## Who decided

Session `b10x-chef-orchestrator`, 2026-11-05, under the owner's grant for class-C items in the
workspace instructions. The answer, verbatim apart from its internal decision number:

> Close wave 2026-11-05 with U2 and U3 on ESS 0.86.0 (the newest released ESS; 0.87.0 is not
> tagged, so the pin rule is met). U1 moves to the next wave, after ESS 0.87.0 is released and the
> SYNTH-003 triage verdict is in. Now: commit the store records in the integration tree, run the
> staged full gate under the standing terms (floor = stage peak + 40G and never under 45G; stop at
> peak + 2G or 2x cargo minutes; BUILD_SLOTS=2 for gate stages; one pre-check per stage including
> trees-budget.sh; discard between stages). Send stage plan and end lines. On green: one bot PR,
> ci-waits row with the plain PR number, end your turn; merge, tag 0.28.0 and release on green CI
> without asking again.

## How it is applied

- U2 and U3 close the wave; `story:harness-spec-runs-on-the-newest-ess` stays active for the next
  one.
- The ESS pin stays where it is, at 0.77.0. Moving it to 0.86.0 is the deferred U1 itself: under
  0.86.0 synthesis refuses 12 generated wrong-state scenarios and the conformance crate's
  complete-coverage check fails (`approval-record:wave-2026-11-04-ess-stays-077`). This wave
  touches neither `spec/` nor the conformance crate, so the newest-release rule is carried by U1.
- 0.28.0 is gated, merged and released on green CI under these terms.
