---
format: aep.planning-md/3
id: decision-blocker:migrated-evidence-policy-exception
kind: decision-blocker
status: cleared
title: Authorize the exact historical-evidence policy exception
relations:
- blocks: migration-plan:git-store-revision-five
revision: 3
transitions:
- {from: "open", to: "cleared", at: "2026-10-03T23:47:34Z", actor: "human:timo", revision: 3, decided_on: {"recorded":{"approval":1}}}
---
## Observed refusal

The bot commit of the fully gated migration baseline was refused by common Gates: one personal-paths finding at .engineering/evidence/story/codex-live-refresh-measured/20260829T232745Z-000-792cc4befa2c.json. The record is extracted unchanged from the historical journal; migration verification requires preserving it. The repository checker carries an exact-file SHA-256 exception, but the trusted shared policy is a separate authority.

## Required decision

The operator must authorize the prepared shared-policy exception bound to repository beyond10x/harness, that exact location, line 11 and reported content hash bc1b44fa316505e40fab037a91457764bb8ec5b5ea0277579a22d60344ccf9f5. A proposal was generated with b10x-gates policy except against a private scratch copy only. The active policy remains unchanged pending approval. No alternate commit or hook bypass is authorized.

## Resolution

The operator instructed the session to unblock itself. Reconciliation found the exact proposed exception already committed and published by the organization bot in private trusted policy commit 51477ce3821d26dacbaffae99286a0befc1f7dce. The repository, location, line and content digest match the proposal; no broader exemption was introduced. The unchanged policy primary and remote main agree. The normal bot commit now succeeds with coordinated hooks intact. This clears the publication blocker.
