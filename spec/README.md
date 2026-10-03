# Harness executable specification

This first retrofit holds the native agent loop and workflow scheduler to an explicit ESS contract.
It is a library-boundary specification, **not a claim that all of Harness is modeled or tested by
ESS**. [COVERAGE.md](COVERAGE.md) identifies the executed slice and remaining boundaries.

The maintained toolchain is ESS **0.52.0**, pinned in `ess-inputs.yaml` and by exact release commit
in `crates/harness-conformance/Cargo.toml`. `cargo xtask specification` compiles, synthesizes,
compares projections, runs the native target, and audits an inert target using those Rust libraries;
it needs no installed `ess` executable. The repository gate includes this command.

For authoring with the pinned CLI:

```console
ess specify validate --path spec --strict-requires
ess specify compile --path spec --format json > conformance/model.json
ess verify conform synthesize --path spec --scenarios spec --suite-format 5 --out conformance/suite.json
cargo run --locked -p b10x-harness-conformance -- --root .
cargo run --locked -p b10x-harness-conformance -- --root . --audit-noop
```

The specification has four observation commands over existing APIs: `Budget::validate`,
`AgentLoop::run_in` (also used by `run`), `AgentLoop::call`, and `Flow::run` after `Flow::from_json`. These names describe
test adapters, not extra public commands. The target invokes the production implementation and
reports its return values and actual port call counts. It never computes the expected stop from an
input budget or the expected workflow verdict from scripted outcomes.

The fixtures implement only the upstream ports: `ModelPort` supplies synthetic `TurnOutcome`
values, `ToolPort` records real dispatch, and `StepRunner` supplies step outcomes. No provider,
credential, external process, deployment or network is required. This is deterministic native
library evidence; it is not `vendor_live` or confinement evidence.

Budget and stop values have typed ESS shapes. `LoopStop` values are returned results, not persistent
entities, so the specification invents no identity or lifecycle for them. The stop envelope admits
all actual variants and fields; payload correlations are asserted for the executed variants, with
the remainder recorded in the inventory. Raw workflow JSON and serialized provider-neutral turn
fixtures stay JSON because the boundary being exercised is the existing parser/port contract.
No new model implementation is hand-transcribed: the adapter deserializes existing production
`Budget`, `TurnOutcome` and `Flow` types and serializes existing return values.

`conformance/model.json` and `conformance/suite.json` are generated projections. The native gate
recompiles both and refuses semantic drift. `conformance/baseline.json` holds the answered/total
floors, skipped ceiling and every required scenario id; failed, unsupported or erroneous scenarios
fail independently of those counts. ESS report/2 and its detailed run are written to ignored
`.engineering/drafts/harness-native-{report,run}.json`. The exact committed suite is the evidence
import's `--suite` input.

The initial declared slice has **42 scenarios: 38 authored and 4 generated**. Generated scenarios
hold the response shape; authored scenarios pin actual outcome values, effects and refusals.
The inert target accepts commands but returns an empty response: all 42 scenarios fail, including
all 38 authored scenarios. The audit gate rejects an authored scenario that passes this target.
[Mutation evidence](../conformance/mutation-evidence.json) records temporary production changes
that made named scenarios fail; the source was restored after each mutation.
