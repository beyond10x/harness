# Wave substrate-adopt — 2026-10-28

Approved by session `b10x-chef-orchestrator`, item D142: `approval-record:wave-substrate-adopt`.
Skill: `aep:implementing` 0.24.1, wave mode, run as a sub-agent of the orchestrator (no operator
turn; the approval is relayed, not bypassed). Base: `main` at `c715947`. Integration branch:
`wave/2026-10-28-substrate-adopt`.

## Units

| # | Artifact | Branch | Tree id | Stage |
|---|---|---|---|---|
| U1 | `story:substrate-pin-0-14` | `wave/2026-10-28-substrate-adopt` (pin-only, coordinator) | `harness-wave-substrate-adopt` | merged |
| U2 | `story:daemon-exec-start-adoption` | `unit/socket-adopt` | `harness-unit-socket-adopt` | merged after two attacks |
| U3 | `task:gate-mints-no-token` | `wave/2026-10-28-substrate-adopt` (CI and prose, coordinator) | `harness-wave-substrate-adopt` | merged |
| U4 | `story:harness-pins-llm-0-16` | `wave/2026-10-28-substrate-adopt` (pin-only, coordinator, added by D142 follow-up) | `harness-wave-substrate-adopt` | merged |

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
corrections; the merges of `unit/socket-adopt`; the closing store commit; one pull request to
`main` through the bot, landed by fast-forward of its green bot-authored head. No tag and no
release at open; the D142 follow-up added the U4 pin commit, the `0.22.0` release commit and the
annotated tag `0.22.0` on that head.

## Added during the wave

`b10x-chef-orchestrator`, 2026-10-28, under the grant (dependency upgrade): pin llm `0.16.0`,
which carries `39346593` (a streamed `message_delta.usage.cost` is carried as a `usage-extension`
warning instead of refusing the turn), and release harness through its own process so metaharness
and takt can follow the tag. U4 is that pin; the release is `0.22.0`. Open pull request
beyond10x/harness#53 (2026-10-26, conflicting with `main`) carries an older, unmerged llm 0.16.0
unit of its own; its test file `crates/harness-llm/tests/messages_usage_cost.rs` was taken from
that branch (`e9ed0623`) with only its story id changed.

## U2: attacks and corrections

| pass | adversary | security reviewer | carried | routed |
|---|---|---|---|---|
| 1 on `f7c02a0` | 4 findings (1 blocker: a `write = true` profile silently replaced a typed `--substrate`/`--substrate-adopt` by the embedded driver), red 2 | NEEDS-CHANGE: the minted id was not checked against `^ws_[A-Za-z0-9]+$` | - | correction 1, `ea12a46` |
| 2 on `e57897b` | 2 findings (a profile toolchain refused as an untyped `--toolchain`), red 1 | NEEDS-CHANGE: the no-id refusal printed daemon bytes raw; `Refused` display unescaped (pre-existing) | 0 | correction 2, `3beb86d`, verified by the coordinator from the diff; the pre-existing finding is `story:substrate-refusal-display-escapes-daemon-bytes` |

The trend is 6 then 4 findings, none carried. The plain `--substrate` override (pass 1, origin
undecided) was fixed in the same correction: it is the same line and the same mechanism.

## Verification

- U2, live: `cargo test -p b10x-harness-substrate --locked --test live -- --ignored` against the
  substrate `0.14.0` release daemon (checksums verified, confined exec in a delegated user cgroup):
  2 passed at `f7c02a0` and again at `e57897b`. Correction 2 changed refusal wording only.
- Full per-step gate at `e57897b`: 15 of 15 steps exit 0, 1144 passed, 0 failed, Harness ESS
  29/29. A run at the release head was stopped at 24G free (b10x-atlas disk request); the
  pull request's Gate is the gate of record for the final head.

## Not done

- The repository secrets `B10X_BOT_APP_ID` and `B10X_BOT_PRIVATE_KEY` that the removed token step
  read are still set; the bot has no secrets permission.
- `AGENTS.md` says two worktrees must not share a `CARGO_TARGET_DIR`; the operator rule of
  2026-10-28 (one shared target per wave) contradicts it, and the line is unchanged.
