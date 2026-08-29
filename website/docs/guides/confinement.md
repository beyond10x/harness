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
  --workspace /work/ws_example \
  --substrate-embedded
```

The output is the actual catalogue admitted on that machine, not an aspirational list. It includes a
`withheld` record when a requested capability could not be admitted.

## Embedded workspace rules

Embedded mode adopts the directory passed as `--workspace`; it does not create or copy it. The
workspace's parent becomes the substrate root, and reads and writes land in the same tree.

The directory name must be `ws_` followed by alphanumerics or underscores, for example
`/work/ws_example`. A differently named directory is refused before the loop starts rather than
silently falling back to read-only.

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

## Build toolchains

`--toolchain rust` mounts the operator's Rust toolchain read-only inside the sandbox. It points
`CARGO_HOME` at `<workspace>/.cargo`, never at the operator's Cargo home, because that directory may
contain registry credentials.

The confined process has no network. Seed only the package cache the task needs into
`<workspace>/.cargo` before the run, or an offline build that needs an unavailable crate will fail.

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
