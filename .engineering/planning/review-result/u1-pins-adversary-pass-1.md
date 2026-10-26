---
format: aep.planning-md/3
id: review-result:u1-pins-adversary-pass-1
kind: review-result
status: active
title: Adversary pass 1 on the llm 0.15.0 and ESS 0.69.2 pins (U1)
summary: '4 findings, 3 red cases: unreachable bedrock refusal, stale COVERAGE pins, README release date'
relations:
- reviews: story:harness-builds-on-llm-0-15
revision: 1
---
## Pass

Adversary pass 1 on unit U1 (`unit/pins-llm-0-15-ess-0-69-2`, `a8d9e4e`, merged at `62bd3b8`),
2026-10-26, `aep:adversary` (opus). New test file `crates/harness-llm/tests/adversary_u1_llm_0_15.rs`,
6 cases, red 3. Origin: introduced 3, pre-existing 1.

## Findings

| # | file:line | verdict | finding | route |
|---|---|---|---|---|
| 1 | `crates/harness-llm/src/lib.rs:357` | CONFIRMED | the `BedrockConverse` arm was unreachable: `binding()` refused first in `llm`'s words, so the refusal `CHANGELOG.md` quotes never reached an embedder | fixed in the correction: `client()` refuses the wires it does not speak before the binding |
| 2 | `spec/COVERAGE.md:33,37` | NEEDS-CHANGE | still named `b10x-llm-*` at tag `0.8.1` | fixed in the correction |
| 3 | `README.md:25-26` | NEEDS-CHANGE | "At `0.19.0` … llm tag `0.15.0`": release `0.19.0` pins `0.8.1` | the release commit dates the sentence to `0.20.0`; the case compares an untagged workspace version with this tree's `Cargo.toml` |
| 4 | `crates/harness-app-server/src/lib.rs` | INFEASIBLE (pre-existing shape) | app-server builds a client per turn, so an OpenCode Go endpoint sees a new `x-opencode-session` each turn | recorded, not changed in this wave |

The two 429 finality cases (Messages spend cap, Codex usage limit) were green on first run: llm
0.8.2's final 429 classes reach the loop as final.
