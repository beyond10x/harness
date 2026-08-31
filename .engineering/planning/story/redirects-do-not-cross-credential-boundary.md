---
format: aep.planning-md/1
id: story:redirects-do-not-cross-credential-boundary
kind: story
status: implemented
title: Provider and renewal requests never follow redirects
summary: A redirect cannot forward a credential header or request body to another origin.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

The generic transport and JSON exchange use the client's default redirect policy. A cross-origin 307 probe forwarded the Messages credential header and model request body; the renewal exchange exposes refresh material to the same class of redirect.

## Acceptance

Both clients refuse every redirect before a second request is made. Two-origin tests cover Authorization, arbitrary wire-built credential headers, request bodies, and renewal form bodies. The refusal names redirect handling without echoing a credential or body.
