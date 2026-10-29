//! Adversary pass 1 on `story:substrate-refusal-display-escapes-daemon-bytes` (wave d221).
//!
//! The claim under attack: no byte a daemon answered reaches a terminal raw through a
//! `SubstrateError`'s text. `Refused` now escapes; these cases drive the other variant that
//! carries daemon bytes, and the wider Unicode control and format classes through `Refused`.

use std::sync::{Arc, Mutex};

use b10x_harness_substrate::{Client, SubstrateError, Transport};
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

/// The story's set: ESC, C1 CSI, DEL, a line break and the right-to-left override.
const HOSTILE: [char; 5] = ['\u{1b}', '\u{9b}', '\u{7f}', '\n', '\u{202e}'];

/// Control and format characters beyond the story's set: C1 (NEL, OSC, ST, APC), bidi marks and
/// isolates, the Arabic letter mark, zero-width space/joiners, the BOM, line and paragraph
/// separators, and a tag character.
const WIDER: [char; 17] = [
    '\u{85}',
    '\u{9d}',
    '\u{9c}',
    '\u{9f}',
    '\u{200e}',
    '\u{200f}',
    '\u{2066}',
    '\u{2067}',
    '\u{2068}',
    '\u{2069}',
    '\u{61c}',
    '\u{200b}',
    '\u{200c}',
    '\u{200d}',
    '\u{feff}',
    '\u{2028}',
    '\u{e0041}',
];

fn raw(text: &str, set: &[char]) -> Vec<char> {
    text.chars().filter(|c| set.contains(c)).collect()
}

/// `Unreadable` on the file route: a 200 answer without `content.data` is quoted into the reason
/// with `{value}`, `serde_json`'s Display, which escapes only U+0000..U+001F. DEL, C1 and the bidi
/// override reach the reason, and so the error line, raw.
#[test]
fn an_unreadable_file_answer_is_displayed_without_raw_control_or_bidi_characters() {
    let hostile: String = HOSTILE.iter().collect::<String>() + "x";
    let body = serde_json::json!({"result": {"note": hostile}}).to_string();
    let said = match Client::with(Answering::with(200, body)).file_read("ws_a", "README.md") {
        Ok(text) => panic!("answered {text:?}"),
        Err(error @ SubstrateError::Unreadable { .. }) => error.to_string(),
        Err(other) => panic!("answered {other:?}"),
    };
    assert!(
        raw(&said, &HOSTILE).is_empty(),
        "raw {:?} in {said:?}",
        raw(&said, &HOSTILE)
    );
}

/// `Refused` against classes beyond the story's named five.
#[test]
fn a_refusal_escapes_the_wider_control_and_format_classes() {
    let body: String = WIDER.iter().collect::<String>() + "x";
    let said = match Client::with(Answering::with(409, body)).workspace_adopt("project") {
        Ok(id) => panic!("answered {id:?}"),
        Err(error) => error.to_string(),
    };
    assert!(
        raw(&said, &WIDER).is_empty(),
        "raw {:?} in {said:?}",
        raw(&said, &WIDER)
    );
}
