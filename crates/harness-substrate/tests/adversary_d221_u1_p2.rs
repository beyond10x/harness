//! Adversary pass 2 on `story:substrate-refusal-display-escapes-daemon-bytes` (wave d221).
//!
//! Correction `aec6a5d` makes `inert` leave `\` raw. A daemon body holding a raw ESC and one
//! holding the six characters `\u{1b}` now display the same, so the error line no longer tells
//! the operator which of the two the daemon sent; a body can print text that reads as an
//! escaped control character it never contained.

use b10x_harness_substrate::inert;

#[test]
fn a_raw_control_character_and_its_spelled_out_escape_display_differently() {
    let raw = "a\u{1b}[2Jb";
    let spelled = r"a\u{1b}[2Jb";
    assert_ne!(
        inert(raw),
        inert(spelled),
        "a raw ESC and the literal text `\\u{{1b}}` display the same"
    );
}

#[test]
fn a_raw_line_break_and_a_spelled_out_one_display_differently() {
    let raw = "ok\nerror: forged";
    let spelled = r"ok\nerror: forged";
    assert_ne!(inert(raw), inert(spelled));
}
