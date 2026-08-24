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

use b10x_harness_substrate::{Client, ConfinedOperations};
use harness_tools::Operations as _;

fn socket() -> Option<String> {
    std::env::var("B10X_SUBSTRATE_SOCKET")
        .ok()
        .filter(|value| !value.is_empty())
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

    let tools = ConfinedOperations::new(
        Client::at(&socket),
        &facts,
        &workspace,
        vec!["/bin/echo".to_owned()],
    );

    tools
        .file_write("hello.txt", "one\ntwo\n")
        .expect("the write lands");

    let read = Client::at(&socket)
        .file_read(&workspace, "hello.txt")
        .expect("reads back");
    assert_eq!(
        read, "one\ntwo\n",
        "the bytes that went in are the bytes that come out"
    );

    if facts.confines_execution() {
        tools
            .run(&["/bin/echo".to_owned(), "confined".to_owned()])
            .expect("the confined process runs");
    }

    // A program outside the declared set never reaches the daemon at all.
    let refused = tools
        .run(&["sh".to_owned(), "-c".to_owned(), "id".to_owned()])
        .expect_err("refused");
    assert!(refused.contains("not a program"), "{refused}");
}
