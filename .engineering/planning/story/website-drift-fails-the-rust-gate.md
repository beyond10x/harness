---
format: aep.planning-md/1
id: story:website-drift-fails-the-rust-gate
kind: story
status: implemented
title: Website drift fails the Rust gate
summary: A Rust checker binds public CLI coverage, release versions, and the no-internal-links projection.
tags:
- docs
- gate
relations:
- derived_from: epic:public-site-is-accurate-live-and-governed
- serves: vision:b10x-owns-its-loop
revision: 5
---
# Story: Website drift fails the Rust gate

## Outcome

A CLI, release, or public-projection change cannot merge while its public documentation is stale.

## Context

Docusaurus catches broken links but cannot compare prose with clap, workspace version, or the public/private boundary.

## Acceptance

- `cargo xtask website-contract` checks generated subcommands/long flags, workspace and CLI contract versions, and forbidden internal links.
- Planted tests prove missing flags, stale versions, and internal links fail.
- `cargo xtask gate` runs the checker and remains green.
- The checker is Rust and adds no runtime or product interface.

## Out of Scope

No browser automation or prose truth inference.

## Open Questions

None.
