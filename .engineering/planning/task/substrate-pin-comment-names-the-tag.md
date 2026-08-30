---
format: aep.planning-md/1
id: task:substrate-pin-comment-names-the-tag
kind: task
status: active
title: The substrate pin's comment names the tag the pin actually holds
relations:
- decomposes: epic:tracking-documents-current
- serves: vision:b10x-owns-its-loop
revision: 4
---
## Evidence

- `crates/harness-substrate/Cargo.toml:25-28` — the comment above the pin: "The tag is substrate `0.2.1` (2026-08-29): the first tag after the brand sweep … and identical to the previously pinned revision `f1cfc1c`".
- `crates/harness-substrate/Cargo.toml:29-30` — the pin itself: `substrate-host` and `substrate-wire` at `tag = "0.2.2", version = "0.2.2"`.
- `0c31438` (2026-08-29 23:25) — the commit that moved the pin `0.2.1` → `0.2.2` and left the comment describing `0.2.1`.
- `AGENTS.md:36-42` — invariant 2: substrate is the one dependency below this component, "pinned by git revision in `crates/harness-substrate/Cargo.toml`, never by `path`". The comment is the only place the reason for the pin's value is written down.
- `STATUS.md:8` — the next evidence for the source row is "swap the revision for a `tag` when substrate tags past `0.2.0`", which has already happened twice.

## What to do

Rewrite the comment to describe `0.2.2` and why the pin moved — workspace adoption dropping the
`ws_` root-name requirement, which `0c31438`'s message states in full. One file, five lines, no
behaviour.

## Done When

The comment and the two `tag =` values name the same version.
