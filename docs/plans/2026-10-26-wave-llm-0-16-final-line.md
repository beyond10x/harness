# Wave llm-0-16-final-line — 2026-10-26

Approved by session `b10x-chef-orchestrator`: `approval-record:wave-llm-0-16-final-line`. Skill:
`aep:implementing` 0.24.1, wave mode. Base: `main` at `684c99f`. Integration branch:
`wave/2026-10-26-llm016-final-line`.

## Units

| # | Story | Branch | Tree id | Stage |
|---|---|---|---|---|
| U1 | `story:harness-builds-on-llm-0-16` | `unit/llm-0-16` | `harness-unit-llm-016` | merged `7f7d10f` |
| U2 | `story:failed-run-ends-with-a-finished-line` | `unit/final-line` | `harness-unit-final-line` | merged `ffc81b3` |
| — | integration | `wave/2026-10-26-llm016-final-line` | `harness-wave-final-line` | closed |

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

## Close

| Unit | Commits | Merge | Adversary | Correction |
|---|---|---|---|---|
| U1 | `23a6eb4`, `c8d2f05` | `7f7d10f` | pass 1: red 2 (README and STATUS pin drift) | `c8d2f05` (STATUS); README dated in the release commit |
| U2 | `1012e54`, `ea60f2b` | `ffc81b3` | pass 1: red 3, 3 mutants the unit's suite missed | round 1 `ea60f2b`; pass 2 not run (docs, test rewrites and one pure function, covered by a new case) |

Coordinator decision on U2 finding 1: `after_compaction` follows the `conversation-compacted`
warning, whether or not the summary changed the conversation (story acceptance 2 reworded).

Diagnosis of the trial defect: verified that a started run failing in a `LoopError` emitted no
`finished` (red on `main`: the stream ended on `TurnStarted { turn: 2 }`). Inferred, not observed:
that the trial's seven steps failed on the first request after compaction; no transcript or stderr
was kept. Two probe measurements are filed as drafts:
`story:summary-fold-keeps-reasoning-with-its-call` and
`story:compaction-shrinks-a-reasoning-heavy-conversation`.

Filed elsewhere: metaharness `story:b10x-failed-run-names-its-cause`
(https://github.com/beyond10x/metaharness/pull/60) and llm `story:context-length-refusal-is-typed`
(https://github.com/beyond10x/llm/pull/63), both drafts.

Not taken: `crates/harness-cli/src/lib.rs` has a second `one_line` (for `refused_line`) on
`str::lines`; pre-existing and outside both stories.

| Agent | Tokens | Tool uses | Duration |
|---|---|---|---|
| U1 implementor | 92,817 | 52 | 349 s |
| U1 adversary | 135,938 | 52 | 433 s |
| U2 implementor | 161,322 | 242 | 2,165 s |
| U2 adversary | 226,747 | 100 | 773 s |
| U2 correction round 1 | 76,840 | 55 | 259 s |
