---
format: aep.planning-md/3
id: review-result:u2-socket-adopt-security-pass-2
kind: review-result
status: active
title: Security re-check of socket adopt (U2)
summary: 'NEEDS-CHANGE: the no-id adopt refusal prints daemon bytes raw; Refused display unescaped (pre-existing)'
relations:
- reviews: story:daemon-exec-start-adoption
revision: 1
---
## Pass

Security re-check of unit U2 (`unit/socket-adopt` at `e57897b`, correction `ea12a46`),
2026-10-28, `aep:security-reviewer` (opus). Verdict NEEDS-CHANGE. Cases executed 307 to 312,
red 2. Origin: introduced 1, pre-existing 1. New test file
`crates/harness-substrate/tests/security_u2_refusal_text_p2.rs`, 5 cases. Pass-1 findings
carried: 0 (`security_u2_minted_id.rs` 2/2 green).

Red output, the file alone:

```
test an_answer_without_an_id_is_quoted_without_raw_control_or_bidi_characters ... FAILED
test a_refusal_body_is_printed_without_raw_control_or_bidi_characters ... FAILED
```

## Checked and found nothing wrong

- The minted-id check is the whole pattern, not a prefix or a contains (case
  `the_minted_id_check_is_the_whole_pattern_not_a_prefix_or_a_contains`, green).
- The request line is the fixed route; labels are always empty; only a checked id reaches the file
  and exec routes.
- The stderr notice carries only a validated id and root name.
- The profile change opens no confinement gap: write scope only narrows, `allow-program` acts as
  typed, profile toolchains are still refused under a socket.
- Live against 0.14.0: `security-u2-p2-a` adopted as a conforming id; a second adopt refused with
  exit 1 and the hint.

```findings
[
{"file":"crates/harness-substrate/src/client.rs","line":510,"category":"integrity","severity":"warning","verdict":"NEEDS-CHANGE","origin":"introduced","message":"the no-id adopt refusal prints the daemon's answer through serde_json Display, so DEL, C1 controls and bidi overrides reach stderr raw while the two sibling refusals quote with Debug (test an_answer_without_an_id_is_quoted_without_raw_control_or_bidi_characters)"},
{"file":"crates/harness-substrate/src/lib.rs","line":302,"category":"integrity","severity":"note","verdict":"CONFIRMED","origin":"pre-existing","message":"SubstrateError::Refused displays the daemon body verbatim, so the 409/404/422 adopt echo carries raw ESC and line breaks to stderr, as every socket refusal did at base (test a_refusal_body_is_printed_without_raw_control_or_bidi_characters)"}
]
```
