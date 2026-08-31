---
title: Confined workspaces
description: Admit writes and process execution through a named substrate boundary.
---

# Confined workspaces

Harness publishes read tools by default. Writes and process execution appear only when a named
substrate boundary says this machine can confine them.

## Choose a boundary

| Mode | Flag | Intended use |
|---|---|---|
| Embedded | `--substrate-embedded` | One operator running locally |
| Daemon | `--substrate /path/to/socket` | A separate substrate service with peer identity |

Neither mode is auto-discovered. The same invocation should not acquire different effects merely
because a daemon happened to be running on one machine.

## Inspect before running

Use `tools` with the same confinement flags you plan to give `run`:

```bash
b10x-harness tools \
  --workspace /work/project_1 \
  --substrate-embedded
```

The output is the actual catalogue admitted on that machine, not an aspirational list. It includes a
`withheld` record when a requested capability could not be admitted.

## Embedded workspace rules

Embedded mode adopts the directory passed as `--workspace`; it does not create or copy it. The
workspace's parent becomes the substrate root, and reads and writes land in the same tree.

The directory name is one non-empty path component containing only ASCII letters, digits, `_` or
`-`. It may not be `.` or `..` or begin with `-`; `/work/project_1` is valid. A name outside that
grammar is refused before the loop starts rather than silently falling back to read-only.

With a guarded workspace but no admitted execution capability, the catalogue has six tools: the
four reads plus `file_write` and `file_edit`.

## Admit process execution

The `run` tool needs all of the following:

- substrate's execution capability probe succeeds;
- the Harness process is inside an appropriate delegated cgroup subtree;
- `--cgroup-root` names the containing delegated root;
- at least one `--allow-program` declaration is present.

The cgroup subtree must be delegated to the current user, have `cpu`, `memory`, and `pids`
controllers, and have no process in its root. A typical outer launch uses a user systemd scope:

```bash
systemd-run --user --scope \
  --property="Delegate=cpu memory pids" \
  -- ./run-harness.sh
```

The script must pass the containing slice that applies on that host as `--cgroup-root`. Inspect with
`b10x-harness tools` inside the scope before starting a paid model run.

Programs are an allowlist of executable names:

```bash
--allow-program cargo --allow-program rustc
```

Harness starts an argv directly. It never invokes a shell to reinterpret one string.
`--allow-program` is matched against `argv[0]`, the root executable Harness starts. Programs that
root starts — such as compilers and linkers — are not matched again. They remain inside the same
sandbox, cgroup limits, no-network namespace and workspace boundary, and whole-tree timeout or
cancellation covers them too.

For an executable outside the sandbox's ordinary `/usr`, `/bin`, `/lib`, `/lib64`, or workspace
mounts, use `--driver /absolute/host/path`. Harness stages exactly that file read-only at
`/toolchain/driver`, admits the mounted path, and reports its digest. The declaration admits the
name and staged file for this run; it is not a general host-path mount.

## Build toolchains

`--toolchain rust` mounts the operator's Rust toolchain read-only inside the sandbox. It points
`CARGO_HOME` at `<workspace>/.cargo`, never at the operator's Cargo home, because that directory may
contain registry credentials.

The confined process has no network. Seed only the package cache the task needs into
`<workspace>/.cargo` before the run, or an offline build that needs an unavailable crate will fail.

`--toolchain go` mounts the installation named by `GOROOT`, or the one containing the first `go`
on `PATH`, read-only at `/toolchain/go`. Go's build cache, module cache and `GOPATH` live inside the
workspace. `GOENV=off`, `GOTOOLCHAIN=local` and `GOSUMDB=off` prevent operator configuration and
toolchain downloads from widening the declaration; the sandbox's unshared network prevents module
lookup from reaching a proxy.

`--toolchain auto` resolves built-in Rust, Go, Taskfile, npm and Yarn provider documents from
root-relative markers; it never runs an unconfined discovery command. Rust and Go contribute typed
lifecycle tools. Taskfile public tasks—including safe, static local includes—and root package
scripts become enum-validated calls through `taskfile_run`, `npm_run` or `yarn_run`. Generic roles
are published only when every active provider implements the role. `--toolchain-spec FILE` loads a
custom provider only when the operator names it. The catalogue is frozen before turn one, and raw
`run` cannot invoke internally admitted programs.

## Approval still applies

Confinement decides what the machine can safely offer. Approval decides whether one offered call may
run now. With the default low risk ceiling, both write tools and `run` ask before execution.

For an unattended run, combine automatic approval with narrow capabilities rather than treating
approval as the only boundary:

```text
confined workspace
  + explicit program list
  + ordered write restrictions
  + duration and token budgets
  + declared unattended approval
```

See [Tools and approvals](../concepts/tools-and-approvals.md) for risk and write-scope semantics and
[Security boundary](../concepts/security-boundary.md) before multi-tenant use.
