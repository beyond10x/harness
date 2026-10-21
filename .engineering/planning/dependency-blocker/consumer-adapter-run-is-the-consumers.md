---
format: aep.planning-md/3
id: dependency-blocker:consumer-adapter-run-is-the-consumers
kind: dependency-blocker
status: open
title: The direct-provider adapter and its conformance run belong to the consumer's repository
relations:
- blocks: story:direct-provider-adapter
revision: 1
---
The work is the consumer's. The story's acceptance says "A consumer binds
TurnEnvironmentProvider to its operations, completes a direct-provider run through the library
… The consumer owns its adapter; this repository changes only proven gaps in its public library
ports" (story body, *Acceptance*). By AGENTS.md invariant 2 this repository may not depend on the
consumer, so the adapter and its lifecycle conformance run are in the consumer's repository
(`ROADMAP.md` Phase 5 names Agent Platform).

No gap in this repository's public ports has been reported by a consumer, so there is nothing to
change here until one is.

Clears when the consumer's repository records a direct-provider run through the library with its
own lifecycle conformance, or reports a named gap in `AgentLoop::run_in`, `ToolPort`,
`ApprovalPort`, `HookPort` or `RunLedger` for this repository to close.

Filed by the harness session on 2026-10-21 while selecting wave `idle-1021`.
