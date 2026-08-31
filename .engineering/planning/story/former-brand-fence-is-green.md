---
format: aep.planning-md/1
id: story:former-brand-fence-is-green
kind: story
status: implemented
title: The harness tree passes the organisation former-brand fence
summary: No unexempted former-brand token remains in tracked source or prose.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

The atlas organisation fence rejects a comment in the substrate manifest. A local gate that omits the organisation check can therefore be green while the collection gate is red.

## Acceptance

The offending prose is rewritten without changing the pinned dependency or wire-visible identifiers. The atlas brand check passes on the worktree, and a repository gate step invokes or clearly delegates to the authoritative organisation fence so the mismatch cannot recur silently.
