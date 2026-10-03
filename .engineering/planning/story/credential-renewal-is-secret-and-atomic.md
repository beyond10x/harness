---
format: aep.planning-md/3
id: story:credential-renewal-is-secret-and-atomic
kind: story
status: implemented
title: Credential renewal is secret-preserving and rejects lost updates
summary: Authorization failures reveal no body, empty tokens refuse, and concurrent file changes win.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
transitions:
- {from: "draft", to: "proposed", at: "2026-08-31T01:56:28Z", actor: "human:timo", revision: 2, imported: true}
- {from: "proposed", to: "active", at: "2026-08-31T01:56:28Z", actor: "human:timo", revision: 3, imported: true}
- {from: "active", to: "implemented", at: "2026-08-31T03:29:34Z", actor: "human:timo", revision: 4, decided_on: {"recorded":{"test_result":1}}, imported: true}
---
## Defect

The renewal exchange includes failed response bodies in errors, accepts empty access or refresh values, and atomically replaces a credential file based on bytes read before an unbounded network round trip.

## Acceptance

Credential exchange errors expose status and a bounded generic reason but no response body. Empty returned credentials refuse before any edit. Immediately before replacement, renewal re-reads and verifies the expected source bytes and refuses a concurrent change without overwriting it. Tests prove redaction, empty-value refusal, mode preservation, and lost-update protection.
