---
format: aep.planning-md/3
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
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T01:56:30Z", actor: "human:timo", revision: 2, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T01:56:30Z", actor: "human:timo", revision: 3, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T03:29:35Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Defect

The generic HTTP crate contains token-route, refresh-field, and authorization-specific semantics in code or tests, crossing the neutral transport fence from the opposite side.

## Acceptance

The HTTP crate exposes only URL, headers, body, decoder, redirect, size, retry, clock, and cancellation mechanics with neutral vocabulary. Credential-specific request construction, response policy, redaction, and fixtures live in harness-credential. A source guard rejects vendor fields, headers, and endpoint paths in shipped HTTP code.
