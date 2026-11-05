---
format: aep.planning-md/3
id: story:harness-spec-runs-on-the-newest-ess
kind: story
status: active
title: The harness specification runs on the newest ESS release with its full coverage
tags:
- from-finding
relations:
- serves: vision:b10x-owns-its-loop
revision: 3
transitions:
- {from: "draft", to: "proposed", at: "2026-11-05T01:35:31Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-11-05T01:35:31Z", actor: "human:timo", revision: 3}
---
## Context

The 0.27.0 gate moved `spec/ess-inputs.yaml` and the `ess-*` crates of
`b10x-harness-conformance` from ESS 0.77.0 to 0.86.0 (rev `8d26c81f`, the 0.86.0 tag commit) and
ran `cargo test -p b10x-harness-conformance` on 2026-11-05. Three cases went red, all from the move:

- `adversary_d221_u3::the_rewritten_projections_equal_the_checked_in_ones` fails with
  `ESS coverage inventory incomplete` (`crates/harness-conformance/src/main.rs`, `compile`), and
  `a_stale_suite_projection_is_refused_without_the_flag` fails because that error comes before the
  drift message it looks for.
- `ess verify conform synthesize --path spec --scenarios spec --suite-format 5` with ess 0.86.0
  selects 127 generated scenarios where the checked-in suite holds 139, and lists 12 in-scope
  refusals, all `ESS-SYNTH-003` on `harness.loop.Loop`: the wrong-state scenarios
  `harness.loop.Loop/state/{AwaitingModel,AwaitingSummary,Failed,Stopped}/refuses/harness.loop.{CompleteTool,Deny,ResumeDenied}`,
  each with "no candidate of the 1 tried satisfies `a harness.loop.Loop row in this state holding
  next == write, on a harness.loop.Loop row in this state holding a value in every field read by:
  next == read`". The ESS changelog entry that sends a wrong-state scenario of a command reading
  stored fields to a row holding a value in every field a sibling's stored guard reads, and refuses
  it as ESS-SYNTH-003 where no reachable row serves, matches the wording (inferred, not traced).
- `stop_union::a_completed_stop_carrying_limit_ms_is_refused_by_ess` pins the refusal text
  `response literal stop: invalid_input`; 0.86.0 adds the field: `... invalid_input at limit_ms`.

The gate kept ESS 0.77.0 for 0.27.0 rather than release a suite with 12 fewer scenarios.

## Outcome

The harness specification and its conformance crate run on the newest ESS release with a complete
coverage inventory and no fewer generated scenarios than at 0.77.0.

## Acceptance

- `spec/ess-inputs.yaml` requires the newest ESS release and the `ess-*` crates of
  `b10x-harness-conformance` pin its tag commit.
- The synthesized coverage suite lists no in-scope refusal, and the wrong-state scenarios of
  `CompleteTool`, `Deny` and `ResumeDenied` in the four states above are generated again, whether
  through a specification change, a synthesis seed, or an ESS fix filed upstream.
- `cargo test -p b10x-harness-conformance` is green, with `stop_union` asserting the refusal text
  the pinned release writes.
