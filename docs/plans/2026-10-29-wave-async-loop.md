# Wave 2026-10-29: LoopTask, the awaitable loop

Status: approved by the operator on 2026-10-29 ("A"). Closed 2026-10-29: three stories and the
epic implemented; `cargo xtask gate` on `6696dfd`: `gate: green`, exit 0, 70 test binaries,
1205 tests passed, Harness ESS 29/29.

## Close

| figure | value |
|---|---|
| adversary passes | loop-task 2 (5 then 1 introduced; 0 carried), real-ports 1 (2 pre-existing) |
| review outcomes | loop-task pass 1 and 2 `fixed`; real-ports pass 1 `no-op` (filed) |
| design amended from findings | epic D4 (a future dropped before its command starts skips it), D5 (`LoopError` returns inside `Ran`/`RanIn` with the sink) |
| filed | `story:mcp-call-stops-when-the-loop-is-cancelled` (under `epic:async-native-loop`), scope added to `story:end-to-end-suite-leaves-no-fixture-binary` |
| implementor spend | loop-task 162,360 + 218,618 + 253,737 tokens; real-ports 130,761; guide 144,485 |
| adversary spend | loop-task 125,136 + 167,457 tokens; real-ports 112,371 |

## Unit records

Trees are managed worktrees under the worktree state root (`b10x/harness/<id>`); build directory
is `target/` inside each tree; scratch is under the coordinator session's scratch root,
`wave/<unit>/scratch`.

| unit | managed id | branch | base | stage |
|---|---|---|---|---|
| integration | `harness-wave-async-loop` | `wave/2026-10-29-async-loop` | `3b62509` | plan merged |
| `story:loop-task-awaits-a-run` | `harness-unit-loop-task` | `impl/loop-task-awaits-a-run` | `3938235` | adversary pass 1 red (5 introduced, `review-result:loop-task-adversary-pass-1`), correction round 1 green (267 cases), adversary pass 2: 1 introduced (`review-result:loop-task-adversary-pass-2`), correction round 2 green (272 cases), coordinator read the round-2 diff, VERIFIED, merged `b87c940`, tree finished |
| `story:real-ports-run-under-an-awaited-loop` | `harness-unit-real-ports` | `impl/real-ports-run-under-an-awaited-loop` | `b87c940` | implementor green (413 cases, no production change); adversary pass 1: 2 pre-existing (`review-result:real-ports-adversary-pass-1`), filed as `story:mcp-call-stops-when-the-loop-is-cancelled` and `story:end-to-end-suite-leaves-no-fixture-binary`; red case re-pinned to today's 30 s wait; VERIFIED, merged `6696dfd`, tree finished |
| `story:embedding-guide-recommends-loop-task` | `harness-unit-embedding-guide` | `impl/embedding-guide-recommends-loop-task` | `b87c940` | acceptance corrected at revision 9 (drop semantics, `Ran`); implementor green, no adversary, VERIFIED, merged `2b3f60a`, tree finished |

## History

- The first plan (`epic:async-native-loop`, seven stories) was not proposed: two adversarial
  reviews found 4 blockers (`review-result:async-loop-adversary-feasibility`,
  `review-result:async-loop-adversary-blast-radius`). The seven stories are archived; the epic is
  kept as the follow-up for native async internals.
- The operator chose option A on 2026-10-29: "A - yes and make async the default and
  recommendation. a sync adapter is a workaround".

## Units

Parent `epic:loop-awaitable-from-async-programs`. All three stories are `draft`; approval of this
page moves each `draft -> proposed -> active` before dispatch.

| wave | story | lands on | subagent |
|---|---|---|---|
| 1 | `story:loop-task-awaits-a-run` | `crates/harness-loop` (new `task.rs`), `crates/harness-conformance/src/boundary.rs`, `CHANGELOG.md` | `aep:implementor`, then `aep:adversary` |
| 2 | `story:real-ports-run-under-an-awaited-loop` | `crates/harness-cli/tests/awaited_ports.rs`, `crates/harness-cli/Cargo.toml` | `aep:implementor`, then `aep:adversary` |
| 2 | `story:embedding-guide-recommends-loop-task` | `website/docs/guides/embedding.md`, `README.md`, `crates/harness-loop/examples/` | `aep:implementor` |

`aep plan artifact waves --kind story --status draft`, verbatim:

```text
wave 1
  story:bridge-interrupt-start-window-under-load
  story:loop-task-awaits-a-run (inferred)
wave 2
  story:embedding-guide-recommends-loop-task (inferred)
  story:real-ports-run-under-an-awaited-loop (inferred)
collision: story:embedding-guide-recommends-loop-task story:loop-task-awaits-a-run crates/harness-loop/src/lib.rs (inferred)
collision: story:loop-task-awaits-a-run story:real-ports-run-under-an-awaited-loop CHANGELOG.md (inferred)
unassessed: story:bridge-mode-interrupt-cases-are-deterministic
unassessed: story:end-to-end-suite-leaves-no-fixture-binary
unassessed: story:harness-loop-modeled-in-ess
unassessed: story:loop-stop-is-a-tagged-union
unassessed: story:substrate-refusal-display-escapes-daemon-bytes
2 wave(s), 2 collision(s), 5 unassessed
```

`story:bridge-interrupt-start-window-under-load` belongs to another epic and is not in this wave.
Both collisions are ordered by `depends_on` edges onto wave 1.

## Plan review

| round | acceptance | design | scope | parallel |
|---|---|---|---|---|
| 1 | needs-revision, 8 findings | needs-revision, 3 | needs-revision, 1 | approve |
| 2 | needs-revision, 3 | needs-revision, 4 | approve | (not rerun) |

Round-2 findings were fixed in the bodies and recorded `fixed`; two rounds is the limit, so the
fixes themselves are unreviewed. The adversary on each unit is the next check.

## Pre-flight (read 2026-10-29)

| figure | value | source |
|---|---|---|
| free disk on `/` | 70G | `df -h /` |
| baseline test build | 81 s, 5.1G `target/`, exit 0 | `cargo test --workspace --locked --no-run`, this tree |
| expected peak | 3 trees, about 15G | 5.1G per tree |
| integration branch | `wave/2026-10-29-async-loop` at `e5ab71d` | `git rev-parse` |

## Commits on approval

1. The plan tree (`plan/async-loop`): one store commit with the new epic, three stories, archived
   stories, review records; merged into the integration branch.
2. One commit per unit, one merge per unit into the integration branch, one closing store commit.
3. `cargo xtask gate` once on the integration branch, then one PR through `b10x-gates` as
   `b10x-bot[bot]`. No release without the operator.
