# Wave llm-0-16-final-line — 2026-10-26

Approved by session `b10x-chef-orchestrator`: `approval-record:wave-llm-0-16-final-line`. Skill:
`aep:implementing` 0.24.1, wave mode. Base: `main` at `684c99f`. Integration branch:
`wave/2026-10-26-llm016-final-line`.

## Units

| # | Story | Branch | Tree id | Stage |
|---|---|---|---|---|
| U1 | `story:harness-builds-on-llm-0-16` | `unit/llm-0-16` | `harness-unit-llm-016` | dispatched |
| U2 | `story:failed-run-ends-with-a-finished-line` | `unit/final-line` | `harness-unit-final-line` | dispatched |
| — | integration | `wave/2026-10-26-llm016-final-line` | `harness-wave-final-line` | open |

Every tree is a managed tree under the worktree state root, with its own `target/`. Scratch is the
session's scratch directory, one subdirectory per unit (`u1-llm-016`, `u2-final-line`). Both serve
`vision:b10x-owns-its-loop`.

## What the store said

`aep plan artifact waves --kind story --status draft --format json` (aep 0.77.1), after the scope
entries were written:

```json
{"waves":[{"wave":1,"ids":["story:bridge-interrupt-start-window-under-load","story:failed-run-ends-with-a-finished-line","story:harness-builds-on-llm-0-16"]}],"collisions":[],"unassessed":["story:bridge-mode-interrupt-cases-are-deterministic","story:end-to-end-suite-leaves-no-fixture-binary","story:harness-loop-modeled-in-ess","story:loop-stop-is-a-tagged-union"]}
```

The other drafts are outside the approved item. `CHANGELOG.md` is written by the coordinator at
integration; neither unit edits it.

## Pre-flight

Free disk at dispatch: 90G.

## Commits this wave makes

Opening store commit, U1 and U2 commits with their corrections, merges into the integration
branch, closing store commit; then the release commit, the pull request and its merge, and the tag
`0.21.0` per `AGENTS.md` § Releases.

## Leftover trees found at the start

`harness-unit-repo-local-profiles` and `harness-wave-repo-profiles`, both at `f88cc09`, as the
previous wave page recorded them. Kept.
