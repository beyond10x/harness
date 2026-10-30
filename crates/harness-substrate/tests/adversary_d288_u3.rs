//! Adversary pass 1 on `story:substrate-escape-is-injective` (wave d288, unit U3).
//!
//! `inert` now escapes `\`, so it must be applied once, to raw text. The unit moved every reason
//! it wrote itself off `{:?}`; these cases look for a reason that still reaches `inert` already
//! quoted by somebody else's `Debug`, and probe `inert` itself over the whole scalar range.

use std::sync::{Arc, Mutex};

use b10x_harness_substrate::{Client, SubstrateError, Transport, inert};
use serde_json::Value;

#[derive(Clone)]
struct Answering {
    answers: Arc<Mutex<Vec<(u16, String)>>>,
}

impl Answering {
    fn with(status: u16, body: String) -> Self {
        Self {
            answers: Arc::new(Mutex::new(vec![(status, body)])),
        }
    }
}

impl Transport for Answering {
    fn request(
        &self,
        _method: &str,
        _path: &str,
        _body: Option<&Value>,
    ) -> Result<(u16, String), SubstrateError> {
        let mut answers = self.answers.lock().expect("not poisoned");
        assert!(!answers.is_empty(), "asked past the script");
        Ok(answers.remove(0))
    }
}

/// A raw ESC and a raw backslash.
const HOSTILE: &str = "a\u{1b}\\b";

/// [`HOSTILE`] escaped once.
const SHOWN: &str = r"a\u{1b}\\b";

/// The machine document's `facts` member answered as a string, not a map.
///
/// `Client::machine` maps the `serde_json` error to `Unreadable { reason: error.to_string() }`
/// (`client.rs`). serde writes an unexpected string as `string {:?}` (`serde_core`'s
/// `Unexpected::Str`), so the reason already holds a `Debug` quote of the daemon's text, and the
/// display escapes it a second time: a raw ESC shows as `\\u{1b}`, which under the new `inert`
/// reads as the six characters `\u{1b}`, a thing the daemon never sent.
///
/// Reached by `b10x-harness --substrate <socket>`: the CLI's first call is `client.machine()` and
/// it prints `no usable substrate daemon at ...: {error}`.
#[test]
fn a_machine_answer_whose_facts_is_a_string_shows_the_daemon_text_escaped_once() {
    let body = serde_json::json!({"result": {"facts": HOSTILE}}).to_string();
    let error = Client::with(Answering::with(200, body))
        .machine()
        .expect_err("a string is not a map of facts");
    let shown = error.to_string();
    let SubstrateError::Unreadable { reason } = &error else {
        panic!("answered {error:?}")
    };
    assert!(
        shown.contains(&format!("\"{SHOWN}\"")) && !shown.contains(r"\\u{1b}"),
        "story:substrate-escape-is-injective: the reason holds serde's `{{:?}}` quote of the \
         daemon's text ({reason:?}), so the display escapes it twice: {shown}"
    );
}

/// Reads [`inert`]'s output back. `None` when the text is not something `inert` can produce.
fn uninert(shown: &str) -> Option<String> {
    let mut text = String::new();
    let mut chars = shown.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\\' {
            text.push(character);
            continue;
        }
        match chars.next()? {
            '\\' => text.push('\\'),
            't' => text.push('\t'),
            'r' => text.push('\r'),
            'n' => text.push('\n'),
            '0' => text.push('\0'),
            'u' => {
                if chars.next()? != '{' {
                    return None;
                }
                let mut hex = String::new();
                loop {
                    match chars.next()? {
                        '}' => break,
                        digit if digit.is_ascii_hexdigit() => hex.push(digit),
                        _ => return None,
                    }
                }
                text.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
            }
            _ => return None,
        }
    }
    Some(text)
}

/// Every scalar value alone: `inert` is decodable back to it, and what it rewrites is ASCII.
#[test]
fn every_scalar_value_reads_back_from_its_display() {
    for code in 0..=0x10_FFFF_u32 {
        let Some(character) = char::from_u32(code) else {
            continue;
        };
        let text = character.to_string();
        let shown = inert(&text);
        assert_eq!(
            uninert(&shown).as_deref(),
            Some(text.as_str()),
            "U+{code:04X} shows as {shown} and does not read back"
        );
        assert!(
            shown == text || (shown.starts_with('\\') && shown.is_ascii()),
            "U+{code:04X} is rewritten to non-ASCII {shown:?}"
        );
    }
}

/// Characters that rewrite or hide a terminal line: C0, DEL, C1, the line and paragraph
/// separators, bidi controls and isolates, zero-width and invisible format characters, the BOM,
/// interlinear annotation, tag characters and variation selectors.
fn hazardous(code: u32) -> bool {
    matches!(
        code,
        0x00..=0x1F
            | 0x7F..=0x9F
            | 0xAD
            | 0x034F
            | 0x061C
            | 0x180E
            | 0x200B..=0x200F
            | 0x2028..=0x202E
            | 0x2060..=0x206F
            | 0xFE00..=0xFE0F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0xE0000..=0xE007F
            | 0xE0100..=0xE01EF
    )
}

#[test]
fn no_hazardous_scalar_value_passes_through_raw() {
    for code in 0..=0x10_FFFF_u32 {
        let Some(character) = char::from_u32(code) else {
            continue;
        };
        if hazardous(code) {
            let shown = inert(&character.to_string());
            assert!(
                shown.is_ascii() && shown.starts_with('\\'),
                "U+{code:04X} passes through as {shown:?}"
            );
        }
    }
}

/// A fixed-seed walk over strings built from the characters whose escapes could collide: the
/// backslash, the letters and digits of `\u{1b}`, braces, both quotes, controls, a C1 and a bidi
/// override, and a combining mark. Two different texts never show alike.
#[test]
fn texts_built_from_escape_material_read_back_from_their_display() {
    const ALPHABET: [char; 18] = [
        '\\', 'u', '{', '}', '1', 'b', '0', 'n', '"', '\'', '\u{1b}', '\n', '\0', '\u{9b}',
        '\u{202e}', '\u{301}', 'e', '\u{7f}',
    ];
    let mut state: u64 = 0x5EED_D288_0003;
    let mut next = || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        state >> 33
    };
    for _ in 0..200_000 {
        let length = next() % 9;
        let text: String = (0..length)
            .map(|_| ALPHABET[usize::try_from(next()).expect("fits") % ALPHABET.len()])
            .collect();
        let shown = inert(&text);
        assert_eq!(
            uninert(&shown).as_deref(),
            Some(text.as_str()),
            "{text:?} shows as {shown} and does not read back"
        );
    }
}

/// The edges of what is rewritten, as literals.
#[test]
fn the_edges_of_the_rewritten_ranges_show_as_written_here() {
    let cases: [(&str, &str); 14] = [
        ("~", "~"),
        ("\u{7f}", r"\u{7f}"),
        ("\u{80}", r"\u{80}"),
        ("\u{9f}", r"\u{9f}"),
        ("\u{a0}", r"\u{a0}"),
        ("\u{a1}", "\u{a1}"),
        ("\u{ad}", r"\u{ad}"),
        ("\u{2069}", r"\u{2069}"),
        ("e\u{301}", r"e\u{301}"),
        ("\\", r"\\"),
        ("\"'", "\"'"),
        ("\0", r"\0"),
        ("\x001", r"\01"),
        ("\r\n\t", r"\r\n\t"),
    ];
    for (text, expected) in cases {
        assert_eq!(inert(text), expected, "{text:?}");
    }
}
