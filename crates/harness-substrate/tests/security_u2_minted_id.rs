//! Security review of `story:daemon-exec-start-adoption` (wave substrate-adopt, unit U2).
//!
//! `Client::workspace_adopt` answers the id the daemon minted, and every later route
//! (`/v1/workspaces/{id}/files/...`, the `exec.start` body) and the run's one stderr notice carry
//! that id verbatim. Substrate ADR 0035 decision 2 keeps the wire `workspace-id` at
//! `^ws_[A-Za-z0-9]+$`, and decision 3 says the result is the row for the given `root_name` with
//! `origin: adopted`. These cases check that an answer outside that contract is not handed on as a
//! workspace to act in.

use std::sync::{Arc, Mutex};

use b10x_harness_substrate::{Client, SubstrateError, Transport};
use serde_json::Value;

#[derive(Clone)]
struct Answering {
    answers: Arc<Mutex<Vec<(u16, String)>>>,
}

impl Answering {
    fn with(body: String) -> Self {
        Self {
            answers: Arc::new(Mutex::new(vec![(200, body)])),
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

fn adopted(id: &str, root_name: &str, origin: &str) -> String {
    serde_json::json!({
        "api_version": "1",
        "request_id": "req_1",
        "result": {"id": id, "state": "ready", "origin": origin, "root_name": root_name, "labels": {}}
    })
    .to_string()
}

#[test]
fn a_minted_id_outside_the_published_workspace_id_pattern_is_not_answered_as_a_workspace() {
    for id in [
        "ws_a/../../execs",
        "ws_a?mode=dir",
        "ws_a HTTP/1.1\r\nX-Injected: 1\r\n\r\nGET /v1/machine",
        "ws_a\nsubstrate adopted `other` as workspace ws_b",
        "ws_",
        "project",
        "",
    ] {
        let client = Client::with(Answering::with(adopted(id, "project", "adopted")));
        if let Ok(answered) = client.workspace_adopt("project") {
            panic!(
                "{id:?} was answered as the workspace to act in ({answered:?}); it would reach \
                 `/v1/workspaces/{answered}/files/...` and the stderr notice unchecked"
            );
        }
    }
}

#[test]
fn an_adopt_answered_for_another_root_name_or_origin_is_not_answered_as_this_directory() {
    for (root_name, origin) in [("other", "adopted"), ("project", "created")] {
        let client = Client::with(Answering::with(adopted("ws_minted7", root_name, origin)));
        if let Ok(answered) = client.workspace_adopt("project") {
            panic!(
                "a result naming root_name {root_name:?} origin {origin:?} was answered as the \
                 adopt of `project` ({answered})"
            );
        }
    }
}
