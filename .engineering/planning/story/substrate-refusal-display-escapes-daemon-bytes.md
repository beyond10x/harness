---
format: aep.planning-md/3
id: story:substrate-refusal-display-escapes-daemon-bytes
kind: story
status: implemented
title: A substrate refusal's text escapes the daemon's control bytes
summary: SubstrateError::Refused displays the daemon body verbatim; escape control and bidi characters on display
tags:
- d142
relations:
- serves: vision:b10x-owns-its-loop
scope:
- confidence: cited
  path: CHANGELOG.md
- confidence: cited
  path: crates/harness-cli/src/lib.rs
- confidence: cited
  path: crates/harness-substrate/src/client.rs
- confidence: cited
  path: crates/harness-substrate/src/lib.rs
- confidence: cited
  path: crates/harness-substrate/src/withheld.rs
- confidence: cited
  path: crates/harness-substrate/tests/security_u2_refusal_text_p2.rs
revision: 11
transitions:
- {from: "draft", to: "proposed", at: "2026-10-29T20:47:58Z", actor: "human:timo", revision: 5}
- {from: "proposed", to: "active", at: "2026-10-29T20:47:59Z", actor: "human:timo", revision: 6}
- {from: "active", to: "implemented", at: "2026-10-29T21:23:14Z", actor: "human:timo", revision: 11, decided_on: {"recorded":{"test_result":1,"review_outcome":2,"verification":1}}}
---
## Context

The security re-check of the socket adopt unit (`review-result:u2-socket-adopt-security-pass-2`,
2026-10-28) measured that `SubstrateError::Refused`'s `Display`
(`crates/harness-substrate/src/lib.rs:302`) prints the daemon's response body verbatim. A body
holding ESC, C1 controls, DEL, a line break or a bidi override reaches the CLI's stderr error line
raw. The behaviour is pre-existing: every socket route's refusal did this at `81e985f`, before the
wave. Only a daemon answering outside its own contract, or another program on the socket the
operator named, can supply such bytes.

## Acceptance

- `SubstrateError::Refused`'s `Display` escapes control and bidi-format characters, at least the
  test's `HOSTILE` set (ESC, C1 CSI U+009B, DEL, LF, U+202E), so none reaches a terminal raw.
- The error value keeps the daemon's body unchanged: a new case shows `Refused { body, .. }` equals
  the hostile input byte for byte.
- The case `a_refusal_body_is_printed_without_raw_control_or_bidi_characters` in
  `crates/harness-substrate/tests/security_u2_refusal_text_p2.rs` loses its `#[ignore]` and passes.

## Out of scope

Bounding the length of a daemon answer (the transport's `read_to_string`).

Revised 2026-10-29 from the acceptance critic's round (wave d221): the unchanged-body half has its own
case, and the escaped set is named.
