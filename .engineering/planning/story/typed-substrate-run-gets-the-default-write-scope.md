---
format: aep.planning-md/3
id: story:typed-substrate-run-gets-the-default-write-scope
kind: story
status: draft
title: A typed substrate run starts from the default write scope
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: inferred
  path: crates/harness-cli/src/lib.rs
revision: 2
---
## Context

Filed by the wave d288 coordinator from security review pass 2 on U1
(`review-result:security-d288-u1-pass-2`, needs-coordinator). The default write scope
(`profile::default_write_scope`, which denies `.git/**` and, from wave d288, `.b10x/**`) is applied
only when a profile sets `write = true` (`crates/harness-cli/src/lib.rs`, the `wanted.write ==
Some(true)` branch, line 1474 at `d8af9b7`). A run started with a typed `--substrate-embedded` or
`--substrate` and no such profile key gets no write scope at all, so it can rewrite `.git/` and the
repository profile the next run applies. This predates wave d288.

## Outcome

Every run that may write through substrate starts from the default write scope unless the operator
types `--write-scope`, whether writing was switched on by a profile or by a typed flag.

## Acceptance

- A typed `--substrate-embedded` run with no `--write-scope` and no profile `write` key refuses a
  write to `.b10x/harness.toml` and to `.git/config`, and admits a write to an ordinary file.
- A typed `--write-scope` still replaces the default, as it does for a profile run.
