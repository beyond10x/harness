---
format: aep.planning-md/3
id: story:conformance-pin-check-follows-the-compiled-ess
kind: story
status: draft
title: The conformance pin check follows the compiled ESS release
tags:
- from-finding
relations:
- informed_by: review-result:adversary-1105b-u1-pass-1
- serves: vision:b10x-owns-its-loop
revision: 1
---
## Context

Noted by `review-result:adversary-1105b-u1-pass-1` (pre-existing, CONFIRMED as a mutant). The
conformance binary checks the specification manifest against a hard-coded string:
`crates/harness-conformance/src/main.rs:191` compares `spec/ess-inputs.yaml`'s `requires` with the
literal `ess 0.86.0`. Nothing ties that literal, or `requires`, to the `ess-*` crates the binary is
compiled against (`crates/harness-conformance/Cargo.toml`, pinned by git revision). A revision
move in `Cargo.toml` without a matching `requires` and `main.rs` change goes unnoticed by the
binary; every ESS move so far has had to change three places by hand.

## Outcome

The conformance binary refuses a manifest whose `requires` names a release other than the one its
compiled `ess-*` crates are, with one place to change when ESS moves.

## Acceptance

- The check reads the compiled crate's version (for example a version constant `ess-compiler`
  exposes, or `CARGO_PKG_VERSION` of a dependency read at build time) rather than a literal, and a
  test with a manifest naming another release is refused with both versions in the message.
- An ESS move changes `Cargo.toml` and `spec/ess-inputs.yaml` only; `main.rs` holds no version.

## Scope

- `crates/harness-conformance/src/main.rs` (`compile`), inferred.
- `crates/harness-conformance/Cargo.toml`, inferred.
