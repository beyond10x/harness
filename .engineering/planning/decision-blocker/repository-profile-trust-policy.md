---
format: aep.planning-md/3
id: decision-blocker:repository-profile-trust-policy
kind: decision-blocker
status: cleared
title: Choose trust, admitted keys and precedence for repository profiles
relations:
- blocks: story:repo-local-profiles
revision: 3
transitions:
- {from: "open", to: "cleared", at: "2026-10-23T19:43:20Z", actor: "human:timo", revision: 3}
---
The owner must decide whether a repository profile needs explicit trust, which permission/plumbing keys it may carry, and its precedence against operator defaults and -p. story:repo-local-profiles lists these unresolved decisions. No answer was supplied by the docs/store/ESS task. Keep implementation blocked until an explicit decision is recorded; existing caller-owned profile behavior remains authoritative.

## Resolution

Decided by the orchestrator under the operator's standing grant, 2026-10-23, quoted:

> a repository profile needs explicit operator trust per repository path; it may carry plumbing keys only, never permission keys (--approve, --approve-up-to, the approver, hooks); precedence -p > repository profile > operator defaults for the keys it may carry.

- Trust: a repository profile is read only for a repository path the operator has explicitly trusted. Each path is trusted separately, and there is no ambient trust.
- Admitted keys: a repository profile may set plumbing keys only. It may never set a permission key: `--approve`, `--approve-up-to`, the approver or hooks.
- Precedence, for the keys a repository profile may carry: `-p` overrides the repository profile, and the repository profile overrides the operator defaults.

story:repo-local-profiles is unblocked on these terms.
