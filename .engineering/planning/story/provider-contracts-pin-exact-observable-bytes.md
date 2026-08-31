---
format: aep.planning-md/1
id: story:provider-contracts-pin-exact-observable-bytes
kind: story
status: implemented
title: Provider contracts pin exact requests, headers, and accepted event inventory
summary: New immutable wire versions describe every implemented event and the bytes actually sent.
tags:
- remediation
relations:
- derived_from: epic:full-review-remediation
- informed_by: review-result:harness-0-5-0-full-review
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Defect

Current manifests omit accepted terminal, error, and reasoning events. Contract tests compare parsed JSON values instead of serialized transport bytes, do not pin observable headers, and the checker proves only that fixtures named by the manifest exist.

## Acceptance

New dated contract versions leave every released directory untouched and pin exact request bytes, header names and values excluding secrets, terminal sentinel policy, and every accepted event class. Rust tests exercise the production encoder and transport request builder. The contract checker independently validates manifest, fixtures, inventory, and prior-version immutability.
