---
format: aep.planning-md/3
id: story:substrate-escape-is-injective
kind: story
status: active
title: A raw control character and its spelled-out escape display differently
relations:
- serves: vision:b10x-owns-its-loop
- informed_by: review-result:adversary-d221-u1-pass-2
revision: 3
transitions:
- {from: "draft", to: "proposed", at: "2026-10-30T17:39:19Z", actor: "human:timo", revision: 2}
- {from: "proposed", to: "active", at: "2026-10-30T17:39:19Z", actor: "human:timo", revision: 3}
---
## Context

Adversary pass 2 on `story:substrate-refusal-display-escapes-daemon-bytes`
(`review-result:adversary-d221-u1-pass-2`, wave d221) found that correction `aec6a5d` made
`harness_substrate::inert` (`crates/harness-substrate/src/lib.rs:326`) leave `\` unescaped, so
that reasons which already hold `{:?}`-quoted text are not escaped twice. Since then a raw ESC and
the six characters `\u{1b}`, and a raw LF and the two characters `\n`, display identically: a
daemon body can print text that reads as an escaped control character it never sent. No raw byte
reaches the terminal; the display is ambiguous. The unit's first commit, `4850c06`, escaped `\`
and kept the two apart.

## Acceptance

- `inert` is injective on the characters it rewrites: a raw control or format character and its
  spelled-out escape text display differently.
- The two cases in `crates/harness-substrate/tests/adversary_d221_u1_p2.rs`
  (`a_raw_control_character_and_its_spelled_out_escape_display_differently`,
  `a_raw_line_break_and_a_spelled_out_one_display_differently`) lose their `#[ignore]` and pass.
- No refusal text is escaped twice: the reasons that `{:?}`-quote a value before they reach
  `inert` pass it the raw value instead, and the existing refusal-text cases
  (`security_u2_refusal_text_p2`, `security_u2_minted_id`, `adversary_d221_u1`) still pass.

## Scope

- `crates/harness-substrate/src/lib.rs` (`inert`), `crates/harness-substrate/src/client.rs`
  (the `Unreadable` reasons that quote with `{:?}`).
