//! What a confined workspace actually does, against a daemon that is running.
//!
//! # Ignored by default, and pointed at a socket rather than starting one
//!
//! `cargo test -p b10x-harness-substrate --test live -- --ignored` with `B10X_SUBSTRATE_SOCKET`
//! set. Starting a daemon here would mean this test held a deployment: substrate needs a delegated
//! cgroup subtree to serve execution at all, and a test that arranged one would be testing its own
//! arrangement.
//!
//! Every other test in this crate answers from a scripted transport, which proves the client agrees
//! with *itself*. This is the one that can disagree with a daemon — and on both occasions this
//! component has spoken to something real, it did.

use b10x_harness_substrate::{Client, ConfinedTools, RUN_TOOL, WRITE_TOOL};
use harness_wire::{CallId, ToolCall, ToolName, ToolPort};
use serde_json::json;

fn socket() -> Option<String> {
    std::env::var("B10X_SUBSTRATE_SOCKET")
        .ok()
        .filter(|value| !value.is_empty())
}

fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        call_id: CallId::new("live-1").expect("valid"),
        name: ToolName::new(name).expect("valid"),
        arguments,
    }
}

#[test]
#[ignore = "needs a running substrate daemon; set B10X_SUBSTRATE_SOCKET"]
fn a_confined_workspace_takes_a_write_reads_it_back_and_runs_a_declared_program() {
    let Some(socket) = socket() else {
        panic!("set B10X_SUBSTRATE_SOCKET to the daemon's socket");
    };

    let facts = Client::at(&socket).machine().expect("the daemon answers");
    assert!(
        facts.holds_workspaces(),
        "this daemon serves no guarded workspace, so there is nothing to exercise"
    );

    let workspace = Client::at(&socket)
        .workspace_create(600_000)
        .expect("a workspace opens");
    assert!(!workspace.is_empty(), "it has an id");

    let mut tools = ConfinedTools::new(
        Client::at(&socket),
        &facts,
        &workspace,
        vec!["/bin/echo".to_owned()],
    );

    let wrote = tools.call(&call(
        WRITE_TOOL,
        json!({"path": "hello.txt", "text": "one\ntwo\n"}),
    ));
    assert!(!wrote.failed, "write: {:?}", wrote.output);

    let read = Client::at(&socket)
        .file_read(&workspace, "hello.txt")
        .expect("reads back");
    assert_eq!(
        read, "one\ntwo\n",
        "the bytes that went in are the bytes that come out"
    );

    if facts.confines_execution() {
        let ran = tools.call(&call(RUN_TOOL, json!({"argv": ["/bin/echo", "confined"]})));
        assert!(!ran.failed, "run: {:?}", ran.output);
    }

    // A program outside the declared set never reaches the daemon at all.
    let refused = tools.call(&call(RUN_TOOL, json!({"argv": ["sh", "-c", "id"]})));
    assert!(refused.failed);
    assert!(
        refused
            .output
            .as_str()
            .unwrap_or_default()
            .contains("not a program"),
        "{:?}",
        refused.output
    );
}
