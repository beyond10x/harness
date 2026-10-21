---
format: aep.planning-md/3
id: dependency-blocker:platform-runner-admits-a-non-codex-child
kind: dependency-blocker
status: open
title: The platform's runner gates its app-server child on Codex's version and flags
relations:
- blocks: story:real-bridge-drives-this-binary
revision: 1
---
The real bridge cannot drive this binary without a change in the platform repository. Read
on 2026-10-21 by a scoper at platform `fa9de23a`; nothing was built or run.

- The runner checks the child's version against `codex-cli 0.145.0` before any turn
  (platform `runtime/agent/crates/agent-harness-codex-runner/src/lib.rs:213-230, 406-410`).
  Passing it would mean this binary impersonating Codex, which
  `crates/harness-app-server/src/inventory.rs:23-25` rules out.
- The runner passes `app-server --strict-config` and `-c` overrides (same file `:433, :709,
  :778, :1127-1137`), which `b10x-harness app-server` does not accept.
- Its attestation sends `config/read`, `configRequirements/read` and `hooks/list`
  (`agent-harness-app-server/src/lib.rs:1597-1612`), which this binary refuses by name
  (`crates/harness-app-server/src/lib.rs:274-328`). Serving them is a new app-server profile
  version here (AGENTS.md invariants 13-15), decided together with the platform.
- The platform registers `b10x_operation_search|describe|invoke`
  (`agent-harness-app-server/src/lib.rs:62-64`); no fixture here answers with those names.

Clears when the platform's runner admits a non-Codex app-server child (no version gate, no
Codex-only flags) or the two repositories agree which of the three attestation methods this
binary serves. Then the run in the story's acceptance can be made against the emulated endpoint
with no credential.

Filed by the harness session on 2026-10-21 while selecting wave `idle-1021`.
