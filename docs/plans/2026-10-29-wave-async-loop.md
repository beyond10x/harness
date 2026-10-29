# Wave 2026-10-29: LoopTask, the awaitable loop

Status: approved by the operator on 2026-10-29 ("A"). Stage 2 running.

## Unit records

Trees are managed worktrees under the worktree state root (`b10x/harness/<id>`); build directory
is `target/` inside each tree; scratch is under the coordinator session's scratch root,
`wave/<unit>/scratch`.

| unit | managed id | branch | base | stage |
|---|---|---|---|---|
| integration | `harness-wave-async-loop` | `wave/2026-10-29-async-loop` | `3b62509` | plan merged |
| `story:loop-task-awaits-a-run` | `harness-unit-loop-task` | `impl/loop-task-awaits-a-run` | `3b62509` | implementor dispatched |
| `story:real-ports-run-under-an-awaited-loop` | — | — | — | waits for wave 1 |
| `story:embedding-guide-recommends-loop-task` | — | — | — | waits for wave 1 |

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
