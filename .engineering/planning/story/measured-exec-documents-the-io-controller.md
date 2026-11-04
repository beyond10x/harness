---
format: aep.planning-md/3
id: story:measured-exec-documents-the-io-controller
kind: story
status: active
title: Measured execution documents the io controller it requires
summary: README and both --cgroup-root help texts name cpu, memory and pids but not io, which run requires (GitHub issue 21)
refs:
- provider: github
  reference: beyond10x/harness#21
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: README.md
- confidence: cited
  path: crates/harness-cli/src/lib.rs
- confidence: cited
  path: website/docs/reference/cli.md
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-11-04T07:29:45Z", actor: "human:timo", revision: 3}
- {from: "proposed", to: "active", at: "2026-11-04T07:29:51Z", actor: "human:timo", revision: 4}
---
## Context

Filed from GitHub issue https://github.com/beyond10x/harness/issues/21 (2026-10-03). The client's
execution contract always requests resource usage, block I/O included
(`crates/harness-substrate/src/lib.rs`, the mandatory measurement), and substrate withholds the exec
facts until every required cgroup v2 counter, `io.stat` among them, is available at the configured
root. The documentation names only three controllers:

- `README.md:178-183` (at `577def9`): the delegated subtree "carries `cpu`, `memory` and `pids`",
  and the recipe is `systemd-run --user --scope --property="Delegate=cpu memory pids"`.
- The two `--cgroup-root` help texts in `crates/harness-cli/src/lib.rs` (near `:668-726` and
  `:1127-1181` at `577def9`) do not name `io` either.

Following those prerequisites on a machine whose user manager delegates `cpu memory pids` leaves
`run` and every toolchain operation withheld, with a correct, named refusal. The refusal stays; the
documentation is what is incomplete.

## Outcome

The README and both CLI help texts state the full prerequisite for measured execution: a delegated,
process-free cgroup root carrying `cpu`, `memory`, `pids` and `io`, with `io` enabled for the
execution children. They distinguish what file tools need from what `run` needs, and say that a
user scope cannot add a controller its parent does not delegate.

## Acceptance

- `README.md` and both `--cgroup-root` help texts name `io` as required for `run` and the toolchain
  tools, and the README recipe delegates `io`.
- The README says that a user scope cannot add a controller the user manager does not delegate, and
  names the administrator step (systemd user-manager `DelegateControllers`) as outside the client.
- The generated CLI reference (`website/docs/reference/cli.md`) is regenerated and
  `harness-docs generate --check` passes.
- No behaviour changes: the measurement stays mandatory, no counter is fabricated and no unmeasured
  execution fallback is added.

## Scope

- `README.md`, `crates/harness-cli/src/lib.rs` (help texts only), `website/docs/reference/cli.md`
  (generated).
