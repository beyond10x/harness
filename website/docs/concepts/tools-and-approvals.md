---
title: Tools and approvals
description: How Harness publishes capabilities, derives risk, and asks before an effect.
---

# Tools and approvals

The published toolset is the policy: if the model cannot be allowed to perform an operation on this
machine, that operation is not published. Approval is a second, per-call gate for capabilities that
do exist.

## The workspace catalogue

| Entry | Neutral operation | Risk | Availability |
|---|---|---:|---|
| `file_read` | `file.read` | low | Always |
| `dir_list` | `dir.list` | low | Always |
| `search` | `search` | low | Always |
| `find` | `find` | low | Always |
| `file_write` | `file.write` | medium | Confined workspace |
| `file_edit` | `file.edit` | medium | Confined workspace |
| `run` | `shell` | high | Confined execution and an allowed program |

Reads are bounded to the workspace root. Paths are re-checked after canonicalization, including the
paths traversed by a search, so a symlink inside the tree cannot be used to read outside it.

`run` accepts an argv, not a shell string. Its program must appear in a repeated `--allow-program`
declaration. The match applies to `argv[0]`, the root executable Harness starts; descendants are
not matched again, but remain inside the same confinement and whole-tree lifetime limits. An empty
program set publishes no `run` tool.

## Flat and verb surfaces

The same catalogue can reach the model in two shapes:

| `--surface` | Model sees | Use |
|---|---|---|
| `flat` | One provider-validated tool per entry | Default; lowest discovery overhead |
| `verbs` | `tool_search`, `tool_describe`, `tool_invoke` | Comparative evaluations and metaharness interoperability |

The operation being approved is always the resolved catalogue entry. A `run` invoked through
`tool_invoke` is reviewed as `run`, not as the generic verb.

Inspect either shape without contacting a provider:

```bash
b10x-harness tools --surface flat --workspace .
b10x-harness tools --surface verbs --workspace .
```

## Risk ceiling and approver

Two settings answer different questions:

- `--approve-up-to` is the highest risk that runs unattended;
- `--approve` chooses who decides calls above that ceiling.

The default ceiling is `low`, so reads proceed and writes or execution ask. The command-line
approver defaults to `auto`:

| Mode | Behaviour above the ceiling |
|---|---|
| `auto` | Ask over `/dev/tty` when interactive; otherwise state the fallback and deny |
| `prompt` | Require an interactive terminal or refuse the run before it starts |
| `deny` | Return a failed tool outcome to the model |
| `all` | Approve every call; `--yes` is the same declaration |

The library default remains deny-all. A shell that wants another policy has to provide it.

:::warning Unattended effects

`--yes` is not a convenience alias for “safe.” It declares that no person is watching and approves
every call that asks. Prefer a narrow tool catalogue, write scope, program list, budget, and
confinement boundary even when approval is automatic.

:::

## Restrict writes by path

`--write-scope` can narrow where admitted write tools act. Rules are ordered and the first match
wins:

```bash
b10x-harness run \
  --write-scope 'src/**=allowed' \
  --write-scope 'config/*.yaml=partial-only' \
  --write-scope '**/*.pem=denied' \
  ...
```

The values mean:

- `allowed`: whole-file writes and exact edits are admitted;
- `partial-only`: an exact edit is admitted, but whole-file replacement is refused;
- `denied`: no write tool may change the matching path.

A path no rule mentions is unrestricted. These flags declare restrictions, not an allowlist. The
default `--scope-announce stated` also tells the model the rules so it does not spend a turn
discovering them through a refusal. `silent` keeps the gate but withholds the instruction, primarily
for experiments.

## Calls that never happen

An unpublished tool, rejected approval, blocked hook, invalid argument, oversized result, or denied
path becomes an explicit failed tool outcome. It is never reported to the model as success, and the
run record carries enough information to distinguish the refusal from an executed call.

## Narrowing for a named agent

A named agent (`--agents-dir`, or a plugin's `agents/`) may declare `tools:`. The declaration is
intersected with what the parent run was admitted — never with the port's whole list — so it can
only remove entries. The narrowing is enforced at one chokepoint: it filters what the child is
published and refuses a call to anything outside it, by the same rule that refuses a tool the run
never published. Under the `verbs` surface the entry is an argument of `tool_invoke` and is decided
inside the port, so this narrowing is a `flat`-surface feature for now.

Continue with [Confined workspaces](../guides/confinement.md) before enabling writes or execution.
