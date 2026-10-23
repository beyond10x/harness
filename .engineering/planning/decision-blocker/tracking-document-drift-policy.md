---
format: aep.planning-md/3
id: decision-blocker:tracking-document-drift-policy
kind: decision-blocker
status: cleared
title: Choose how tracking-document drift is detected or accepted
relations:
- blocks: initiative:record-matches-the-code
revision: 3
transitions:
- {from: "open", to: "cleared", at: "2026-10-23T19:43:20Z", actor: "human:timo", revision: 3}
---
The initiative's completion clause requires a mechanism that prevents STATUS/ROADMAP/CHANGELOG drift, or an explicit decision accepting the residual drift. Its existing children repair snapshots but own no continuing prevention. The owner must choose a bounded check and assign implementation, or explicitly accept residual drift. This session's reviews do not supply that policy decision.

## Resolution

Decided by the orchestrator under the operator's standing grant, 2026-10-23, quoted:

> task check fails when CHANGELOG has no section for the Cargo version or a STATUS row predates the newest release tag; ROADMAP drift is accepted.

- `task check` fails when `CHANGELOG.md` has no section for the workspace version in `Cargo.toml`.
- `task check` fails when a `STATUS.md` row is dated before the newest release tag.
- Drift in `ROADMAP.md` is accepted as residual and is not checked.

This satisfies the completion clause of initiative:record-matches-the-code: CHANGELOG and STATUS get a bounded check, and the decision accepts the residual ROADMAP drift. The check itself is still to be implemented.
