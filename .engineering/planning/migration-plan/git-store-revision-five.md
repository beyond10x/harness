---
format: aep.planning-md/3
id: migration-plan:git-store-revision-five
kind: migration-plan
status: draft
title: Migrate Harness planning to the Git-native revision-five store
relations:
- informed_by: initiative:record-matches-the-code
revision: 3
---
## Outcome

The operator requested the current AEP store driver and a session integration branch on 2026-10-03. AEP 0.68.0 is the current release; it migrated aep.project/1 to aep.project/5 with store.git. The repository's stable planning scope is harness, replacing the migration's incidental worktree-directory default. Governing protocols now pin the exact peeled AEP 0.68.0 tag, 6d7a44d3607d2d9a6ffdf0a165993c546c43d0db; the retired engineering-protocols pin did not declare the ESS lifecycle.

## Verification

`aep plan store migrate git --verify` compared all 99 pre-existing artifacts, 241 transitions and 91 evidence records and reported their status, revision, title, relations, body, transitions and evidence equal. Git history retains the removed journal and its creation/body/relation events. `aep plan artifact validate` reports valid with pre-existing scope and historical-review warnings.

## Session boundary

Use integration/harness-20261003 for this session. Preserve the primary checkout. Public documentation overhaul and two planning review passes follow this baseline; an ESS retrofit follows because no specification exists in the baseline tree. No release or main-branch merge is requested.

## Migration compatibility

The source gate passed before staging. The verified extraction moves one existing historical home-path record out of the journal's existing exception into its own evidence file. The Rust privacy checker retains that exact file only at its verified content digest; its content is not rewritten or treated as new evidence. The checker is ported because this is a material change to the legacy Python implementation. The baseline also updates assertion diagnostics for current stable Rust without raising the minimum supported version.

## Review record handling

Two plan-reviewer passes found six substantive issues and then approved their corrections. Public reports explicitly redact the local checkout prefix from validation output; original tool output remains outside the public repository. One unpublished report creation was rolled back before commit and re-recorded from the reviewer's public-safe delivery. No published or historical review was rewritten.
