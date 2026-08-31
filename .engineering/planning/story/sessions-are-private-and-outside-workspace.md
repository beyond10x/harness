---
format: aep.planning-md/1
id: story:sessions-are-private-and-outside-workspace
kind: story
status: implemented
title: Session transcripts are private and never written in the workspace
summary: Explicit and default session paths enforce placement, absolute roots, and private modes.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

The CLI accepts a session directory inside the workspace, a relative XDG_STATE_HOME can resolve there implicitly, and an existing permissive directory can yield readable transcript files.

## Acceptance

All resolved session directories are absolute and outside the workspace, including symlink and relative-path cases. Existing and new session directories are mode 0700 and every transcript is mode 0600 on Unix. Refusals occur before a provider request and name the offending session option or environment source.
