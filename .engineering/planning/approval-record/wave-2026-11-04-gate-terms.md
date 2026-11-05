---
format: aep.planning-md/3
id: approval-record:wave-2026-11-04-gate-terms
kind: approval-record
status: approved
title: 'Wave 2026-11-04: staged gate terms and the ESS 0.86.0 pin'
relations:
- decides: approval-record:wave-2026-11-04
revision: 2
transitions:
- {from: "draft", to: "approved", at: "2026-11-05T00:40:41Z", actor: "human:timo", revision: 2}
---
## The terms

Session `b10x-chef-orchestrator`, 2026-11-05, on the staged gate plan for release 0.27.0 of wave
2026-11-04 (stages G0 to G6 in the wave's stage table), verbatim:

> G0-G6 as planned, with your caps as the stop lines (G1 10G/30 ... G6 6/15); minutes count cargo
> time only.
> Start floor per stage: estimated peak + 40G, never under 45G. Stop at / < 40G.
> BUILD_SLOTS=2, discard between stages, one pre-check per stage.
> Every stage's estimate is under 10G, so it may run beside the one big stage allowed machine-wide
> while another session's gate runs. If a stage's target passes 10G, tell me at once.
> If harness's ess pin or its ESS crates are behind 0.86.0, move them in G0 (0.86.0 verified
> 09:49Z).
> Start after both passes are routed. Send start and end lines. Then one bot PR and a ci-waits row.

## What it changes in the wave

The ESS pin moves from 0.77.0 to 0.86.0 in G0, superseding the opening record's "ESS stays at
0.77.0": `spec/ess-inputs.yaml`, the four `ess-*` crates of `b10x-harness-conformance` at rev
`8d26c81f` (the commit the 0.86.0 tag points at), the manifest check in
`crates/harness-conformance/src/main.rs`, `AGENTS.md` and `spec/README.md`.
`ess specify validate --path spec --strict-requires` with ess 0.86.0: "harness v1 — 3 file(s), 44
scenario(s), valid".
