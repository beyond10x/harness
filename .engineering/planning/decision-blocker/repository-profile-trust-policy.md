---
format: aep.planning-md/3
id: decision-blocker:repository-profile-trust-policy
kind: decision-blocker
status: open
title: Choose trust, admitted keys and precedence for repository profiles
relations:
- blocks: story:repo-local-profiles
revision: 1
---
The owner must decide whether a repository profile needs explicit trust, which permission/plumbing keys it may carry, and its precedence against operator defaults and -p. story:repo-local-profiles lists these unresolved decisions. No answer was supplied by the docs/store/ESS task. Keep implementation blocked until an explicit decision is recorded; existing caller-owned profile behavior remains authoritative.
