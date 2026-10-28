# Wave substrate-adopt — 2026-10-28

Approved by session `b10x-chef-orchestrator`, item D142: `approval-record:wave-substrate-adopt`.
Skill: `aep:implementing` 0.24.1, wave mode, run as a sub-agent of the orchestrator (no operator
turn; the approval is relayed, not bypassed). Base: `main` at `c715947`. Integration branch:
`wave/2026-10-28-substrate-adopt`.

## Units

| # | Artifact | Branch | Tree id | Stage |
|---|---|---|---|---|
| U1 | `story:substrate-pin-0-14` | `wave/2026-10-28-substrate-adopt` (pin-only, coordinator) | `harness-wave-substrate-adopt` | planned |
| U2 | `story:daemon-exec-start-adoption` | `unit/socket-adopt` | `harness-unit-socket-adopt` | planned |
| U3 | `task:gate-mints-no-token` | `wave/2026-10-28-substrate-adopt` (CI and prose, coordinator) | `harness-wave-substrate-adopt` | planned |

Every tree is a managed tree under the worktree state root. Builds share one `CARGO_TARGET_DIR`
for the wave (operator rule, 2026-10-28), under the session's scratch directory, with line-table
debug info and no incremental build. U2 is dispatched to `aep:implementor`, then `aep:adversary`
and `aep:security-reviewer` (it chooses which workspace a confined exec runs in). U1 is pin-only
and gets no adversary. All serve `vision:b10x-owns-its-loop`.

## Store moves at open

- `dependency-blocker:substrate-daemon-admits-operator-named-workspaces` cleared: substrate
  `0.7.17` (`00fa634c`) shipped `workspace.adopt` (ADR 0035); harness pins `0.13.0`, which has it.
- `story:substrate-pin-0-12` implemented: Gate run 40509014289 on `ef6df52` (tag `0.21.0`) green,
  `Harness ESS: 29 total, 29 passed`.
- `story:daemon-exec-start-adoption` re-scoped to ADR 0035 and active.
- `task:gate-token-steps-in-one-action` rejected; `task:gate-mints-no-token` replaces it.

## What the store said

`aep plan artifact waves --kind story --status active --format json` (aep 0.78.0) puts
`story:daemon-exec-start-adoption` and `story:substrate-pin-0-14` in one wave. Their only
shared surface is `CHANGELOG.md` (one line each), which the coordinator merges.

## Commits this wave makes

The opening store commit; the U1 pin commit; the U3 commit; the U2 commit or commits and their
corrections; the merge of `unit/socket-adopt`; the closing store commit; one pull request to
`main` through the bot, merged by the App when the required checks are green. No tag and no
release.
