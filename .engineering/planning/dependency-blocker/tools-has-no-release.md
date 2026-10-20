---
format: aep.planning-md/3
id: dependency-blocker:tools-has-no-release
kind: dependency-blocker
status: open
title: The tools repository has no released crate to pin
relations:
- blocks: story:harness-consumes-tools-crates
revision: 3
---
## What is missing

The `tools` repository has released nothing: main `dabd034` (2026-10-20) has no workspace members
in `Cargo.toml`, `git tag` prints nothing, and `gh release list --repo beyond10x/tools` prints
nothing. `story:harness-consumes-tools-crates` cannot pin a crate that does not exist.

## Cleared when

`tools` carries a tag, and `story:harness-consumes-tools-crates` § Open names that tag and the
crates in it that replace `crates/harness-tools` and `crates/harness-toolchain` (session `tools` is
moving both; ADR 0095 phase T1, ADR 0097 decision 3). Both, because `harness-tools` depends on
`harness-toolchain` (`crates/harness-tools/Cargo.toml:16`): a `tools` crate that still needed the
toolchain crate from here would depend on a harness crate.
