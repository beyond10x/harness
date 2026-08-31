---
title: Make a confined change
description: Move from read-only inspection to a writing run whose paths and commands are bounded.
---

# Make a confined change

This tutorial adds effects deliberately. You will inspect the exact catalogue, enable an embedded
substrate boundary, admit one program, and keep `.git` denied.

## Before you start

Complete [First read-only run](../getting-started.md). Work in a disposable checkout with a clean
`git status`, and inspect the provider you plan to use:

```bash
b10x-harness providers show claude
```

The built-in provider may select a default credential source. `providers show` tells you which one
before any model request.

## Inspect the effect boundary

Without substrate, the catalogue is read-only:

```bash
b10x-harness tools --workspace .
```

Now ask what the embedded boundary would publish:

```bash
b10x-harness tools \
  --substrate-embedded \
  --workspace . \
  --allow-program cargo
```

Expect `file_write` and `file_edit`, plus `run` for the admitted program. If the host cannot provide
the requested boundary, Harness refuses the capability instead of publishing a pretend tool.

## Run the change

```bash
b10x-harness run -p confined-change \
  --workspace . \
  --write-scope '.git/**=denied' \
  --write-scope '**=allowed' \
  --allow-program cargo \
  --approve prompt \
  --max-turns 12 \
  --max-duration-ms 180000 \
  --input "Make the smallest change requested, then run the relevant Cargo test."
```

`--approve prompt` requires a terminal and asks about calls above the unattended risk ceiling. Use
`--approve deny` for a non-interactive dry boundary. Use `--approve all` only when the invocation
itself is your explicit declaration that nobody is watching.

The first matching scope rule wins. Keeping `.git/**=denied` before `**=allowed` prevents the run
from changing repository metadata while permitting files elsewhere in the workspace.

## Verify outside the model

After the run:

```bash
git status --short
git diff --check
cargo test --workspace --locked
```

Review every change yourself. The substrate boundary limits effects; it does not establish that a
change is correct.

Harness prints the session id unless sessions are disabled. Inspect it with:

```bash
b10x-harness sessions
```

Continue with [Confined workspaces](../guides/confinement.md) for daemon mode and host
prerequisites, or [Tools and approvals](../concepts/tools-and-approvals.md) for the risk model.
