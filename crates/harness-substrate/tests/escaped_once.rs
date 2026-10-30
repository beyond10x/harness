//! `story:substrate-escape-is-injective`: a refusal's text is escaped once, by `inert`, and only
//! when it is displayed.
//!
//! `inert` escapes `\`, so a reason that already quoted a value with `{:?}` would show every
//! backslash of that quoting doubled: a raw ESC as `\\u{1b}`, which reads as the six characters a
//! daemon could have sent. The reasons hold the raw value and the display escapes it, so each
//! value below shows a raw ESC as `\u{1b}` and a raw `\` as `\\`, exactly once.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex};
use std::thread;

use b10x_harness_substrate::{Client, SubstrateError, Transport, UnixTransport};
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

/// A raw ESC and a raw backslash: the two characters whose escapes collide when text is escaped
/// twice.
const HOSTILE: &str = "a\u{1b}\\b";

/// [`HOSTILE`] as the display shows it, escaped once.
const SHOWN: &str = r"a\u{1b}\\b";

fn adopted(id: &str, root_name: &str, origin: &str) -> String {
    serde_json::json!({"result": {"id": id, "origin": origin, "root_name": root_name}}).to_string()
}

fn unreadable(body: String) -> (String, String) {
    match Client::with(Answering::with(200, body)).workspace_adopt("project") {
        Err(error @ SubstrateError::Unreadable { .. }) => {
            let shown = error.to_string();
            let SubstrateError::Unreadable { reason } = error else {
                unreachable!()
            };
            (reason, shown)
        }
        other => panic!("answered {other:?}"),
    }
}

#[test]
fn a_minted_id_outside_the_pattern_is_kept_raw_and_displayed_escaped_once() {
    let id = format!("ws_{HOSTILE}");
    let (reason, shown) = unreadable(adopted(&id, "project", "adopted"));
    assert!(
        reason.contains(&format!("workspace id \"{id}\"")),
        "story:substrate-escape-is-injective: the reason holds the id as answered: {reason:?}"
    );
    assert!(
        shown.contains(&format!("workspace id \"ws_{SHOWN}\"")),
        "story:substrate-escape-is-injective: the display escapes the id once: {shown}"
    );
}

#[test]
fn an_answered_root_name_is_kept_raw_and_displayed_escaped_once() {
    let (reason, shown) = unreadable(adopted("ws_minted7", HOSTILE, "adopted"));
    assert!(
        reason.contains(&format!("root name \"{HOSTILE}\"")),
        "story:substrate-escape-is-injective: the reason holds the root name as answered: {reason:?}"
    );
    assert!(
        shown.contains(&format!("root name \"{SHOWN}\"")),
        "story:substrate-escape-is-injective: the display escapes the root name once: {shown}"
    );
}

#[test]
fn an_answered_origin_is_kept_raw_and_displayed_escaped_once() {
    let (reason, shown) = unreadable(adopted("ws_minted7", "project", HOSTILE));
    assert!(
        reason.contains(&format!("origin is \"{HOSTILE}\"")),
        "story:substrate-escape-is-injective: the reason holds the origin as answered: {reason:?}"
    );
    assert!(
        shown.contains(&format!("origin is \"{SHOWN}\"")),
        "story:substrate-escape-is-injective: the display escapes the origin once: {shown}"
    );
}

/// The answer's JSON text is the document as `serde_json` writes it, not a `{:?}` quote of it.
#[test]
fn an_answer_without_an_id_is_quoted_as_its_json_text_not_a_debug_quote_of_it() {
    let body = serde_json::json!({"result": {"origin": "adopted", "root_name": "project"}});
    let (reason, _) = unreadable(body.to_string());
    assert_eq!(
        reason,
        format!("no workspace id in {body}"),
        "story:substrate-escape-is-injective: the reason holds the answer's JSON text"
    );
}

#[test]
fn a_root_name_refused_before_sending_is_kept_raw_and_displayed_escaped_once() {
    match Client::with(Answering::with(200, String::new())).workspace_adopt(HOSTILE) {
        Err(error @ SubstrateError::Refused { status: 0, .. }) => {
            let shown = error.to_string();
            let SubstrateError::Refused { body, .. } = error else {
                unreachable!()
            };
            assert!(
                body.starts_with(&format!("\"{HOSTILE}\" is not a substrate root name")),
                "story:substrate-escape-is-injective: the body holds the name as given: {body:?}"
            );
            assert!(
                shown.contains(&format!("\"{SHOWN}\" is not a substrate root name")),
                "story:substrate-escape-is-injective: the display escapes the name once: {shown}"
            );
        }
        other => panic!("answered {other:?}"),
    }
}

#[test]
fn a_status_line_that_is_not_one_is_kept_raw_and_displayed_escaped_once() {
    let dir = tempfile::tempdir().expect("a directory");
    let socket = dir.path().join("substrate.sock");
    let listener = UnixListener::bind(&socket).expect("binds");
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0_u8; 2048];
            let _ = stream.read(&mut buffer);
            let _ = stream.write_all(format!("HTTP/1.1 {HOSTILE}\r\n\r\n").as_bytes());
        }
    });
    let error = Client::with(UnixTransport::new(&socket))
        .machine()
        .expect_err("not a status line");
    let shown = error.to_string();
    let SubstrateError::Unreadable { reason } = error else {
        panic!("answered {error:?}")
    };
    assert!(
        reason.contains(&format!("\"HTTP/1.1 {HOSTILE}\r\n\"")),
        "story:substrate-escape-is-injective: the reason holds the line as read: {reason:?}"
    );
    assert!(
        shown.contains(&format!(r#""HTTP/1.1 {SHOWN}\r\n""#)),
        "story:substrate-escape-is-injective: the display escapes the line once: {shown}"
    );
}
