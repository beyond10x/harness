//! Security re-check of `story:daemon-exec-start-adoption` after correction `ea12a46`.
//!
//! `workspace_adopt`'s refusals and the CLI's error line around them reach the operator's
//! terminal. These cases check that bytes a daemon answered cannot reach that terminal as raw
//! control or bidirectional-format characters, whichever refusal path carries them.

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

/// ESC, C1 CSI, DEL, a line break and the right-to-left override.
const HOSTILE: [char; 5] = ['\u{1b}', '\u{9b}', '\u{7f}', '\n', '\u{202e}'];

fn raw_hostile(text: &str) -> Vec<char> {
    text.chars().filter(|c| HOSTILE.contains(c)).collect()
}

fn hostile_value() -> String {
    HOSTILE.iter().collect::<String>() + "x"
}

fn refusal_of(status: u16, body: String) -> String {
    match Client::with(Answering::with(status, body)).workspace_adopt("project") {
        Ok(id) => panic!("answered {id:?}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn a_hostile_minted_id_is_quoted_without_raw_control_or_bidi_characters() {
    let body = serde_json::json!({"result": {
        "id": format!("ws_a{}", hostile_value()), "origin": "adopted", "root_name": "project"
    }})
    .to_string();
    let said = refusal_of(200, body);
    assert!(
        raw_hostile(&said).is_empty(),
        "raw {:?} in {said:?}",
        raw_hostile(&said)
    );
}

#[test]
fn a_hostile_root_name_in_the_answer_is_quoted_without_raw_control_or_bidi_characters() {
    let body = serde_json::json!({"result": {
        "id": "ws_minted7", "origin": "adopted", "root_name": hostile_value()
    }})
    .to_string();
    let said = refusal_of(200, body);
    assert!(
        raw_hostile(&said).is_empty(),
        "raw {:?} in {said:?}",
        raw_hostile(&said)
    );
}

#[test]
fn an_answer_without_an_id_is_quoted_without_raw_control_or_bidi_characters() {
    let body = serde_json::json!({"result": {
        "origin": "adopted", "root_name": hostile_value()
    }})
    .to_string();
    let said = refusal_of(200, body);
    assert!(
        raw_hostile(&said).is_empty(),
        "raw {:?} in {said:?}",
        raw_hostile(&said)
    );
}

#[test]
#[ignore = "story:substrate-refusal-display-escapes-daemon-bytes: pre-existing on every route"]
fn a_refusal_body_is_printed_without_raw_control_or_bidi_characters() {
    let body = format!(
        r#"{{"error":{{"code":"workspace.already-exists","message":"{}"}}}}"#,
        hostile_value()
    );
    let said = refusal_of(409, body);
    assert!(
        raw_hostile(&said).is_empty(),
        "raw {:?} in {said:?}",
        raw_hostile(&said)
    );
}

#[test]
fn the_minted_id_check_is_the_whole_pattern_not_a_prefix_or_a_contains() {
    let answer = |id: &str| {
        let body =
            serde_json::json!({"result": {"id": id, "origin": "adopted", "root_name": "project"}})
                .to_string();
        Client::with(Answering::with(200, body)).workspace_adopt("project")
    };
    assert_eq!(
        answer("ws_01M61HEFCNSQ1JSCFD6PY1BS2Y").expect("a daemon-shaped id"),
        "ws_01M61HEFCNSQ1JSCFD6PY1BS2Y"
    );
    for id in [
        "xws_a",
        "WS_a",
        "ws_a_b",
        "ws_a-b",
        "ws_a.b",
        " ws_a",
        "ws_a ",
        "ws_a\0",
        "ws_\u{ff41}",
        "ws_a%2F..",
        "ws",
    ] {
        assert!(answer(id).is_err(), "{id:?} was answered as a workspace");
    }
}
