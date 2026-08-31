---
format: aep.planning-md/1
id: story:generic-http-has-no-credential-semantics
kind: story
status: implemented
title: Generic HTTP names no credential or vendor semantics
summary: Authorization exchange policy and fixtures live in the credential crate, not transport.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

The generic HTTP crate contains token-route, refresh-field, and authorization-specific semantics in code or tests, crossing the neutral transport fence from the opposite side.

## Acceptance

The HTTP crate exposes only URL, headers, body, decoder, redirect, size, retry, clock, and cancellation mechanics with neutral vocabulary. Credential-specific request construction, response policy, redaction, and fixtures live in harness-credential. A source guard rejects vendor fields, headers, and endpoint paths in shipped HTTP code.
