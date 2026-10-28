---
format: aep.planning-md/3
id: story:substrate-refusal-display-escapes-daemon-bytes
kind: story
status: draft
title: A substrate refusal's text escapes the daemon's control bytes
summary: SubstrateError::Refused displays the daemon body verbatim; escape control and bidi characters on display
tags:
- d142
relations:
- serves: vision:b10x-owns-its-loop
revision: 1
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

- `SubstrateError::Refused` keeps the daemon's body unchanged in the error value, and its `Display`
  escapes control and bidi characters, so none reaches a terminal raw.
- The case `a_refusal_body_is_printed_without_raw_control_or_bidi_characters` in
  `crates/harness-substrate/tests/security_u2_refusal_text_p2.rs` loses its `#[ignore]` and passes.

## Out of scope

Bounding the length of a daemon answer (the transport's `read_to_string`).
