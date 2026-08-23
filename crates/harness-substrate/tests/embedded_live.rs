//! The embedded driver, doing the thing, with no daemon anywhere.
//!
//! Not ignored: it needs no deployment, no socket and no credential — substrate's driver opens a
//! directory this test makes and hands back. That is the whole argument for embedding, and it is
//! checked on every run rather than when somebody remembers.
//!
//! Execution is asked for only where the machine admits it. A host with no delegated cgroup subtree
//! reports no exec facts, publishes no `run`, and this test says so instead of failing — the point
//! is that the toolset follows the machine, not that every machine is the same.

use b10x_harness_substrate::{Backend, ConfinedTools, Embedded, RUN_TOOL, WRITE_TOOL};
use harness_wire::{CallId, ToolCall, ToolName, ToolPort};
use serde_json::json;

fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        call_id: CallId::new("embedded-1").expect("valid"),
        name: ToolName::new(name).expect("valid"),
        arguments,
    }
}

#[test]
fn an_embedded_driver_opens_a_workspace_writes_a_file_and_reads_it_back() {
    let root = tempfile::tempdir().expect("a temporary root");
    let embedded = Embedded::open(root.path(), None).expect("the driver opens");

    let facts = embedded.machine().expect("it says what it can do");
    assert!(
        facts.holds_workspaces(),
        "a driver over a directory it owns serves guarded workspaces: {facts:?}"
    );

    let workspace = embedded
        .workspace_create(600_000)
        .expect("a workspace opens with no daemon in sight");

    embedded
        .file_write(&workspace, "hello.txt", "one\ntwo\n")
        .expect("the write lands");
    assert_eq!(
        embedded.file_read(&workspace, "hello.txt").expect("reads back"),
        "one\ntwo\n",
        "the bytes that went in are the bytes that come out"
    );
}

#[test]
fn the_toolset_follows_the_machine_and_the_tools_do_not_know_which_backend_they_hold() {
    let root = tempfile::tempdir().expect("a temporary root");
    let embedded = Embedded::open(root.path(), None).expect("the driver opens");
    let facts = embedded.machine().expect("facts");
    let workspace = embedded.workspace_create(600_000).expect("a workspace");

    let mut tools = ConfinedTools::new(
        Embedded::open(root.path(), None).expect("a second handle on the same root"),
        &facts,
        &workspace,
        vec!["/bin/echo".to_owned()],
    );

    let names: Vec<String> = tools
        .specs()
        .iter()
        .map(|spec| spec.name.to_string())
        .collect();
    assert!(names.contains(&WRITE_TOOL.to_owned()), "{names:?}");
    assert_eq!(
        names.contains(&RUN_TOOL.to_owned()),
        facts.confines_execution(),
        "`run` is published exactly where the machine can confine one: {names:?}"
    );

    let wrote = tools.call(&call(
        WRITE_TOOL,
        json!({"path": "written.txt", "text": "by a tool\n"}),
    ));
    assert!(!wrote.failed, "{:?}", wrote.output);
    assert_eq!(
        embedded
            .file_read(&workspace, "written.txt")
            .expect("reads back"),
        "by a tool\n"
    );

    // The local refusal holds whichever backend is underneath: nothing was sent anywhere.
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

#[test]
fn a_tree_that_already_exists_is_adopted_rather_than_replaced_by_an_empty_one() {
    // The gap this closes: a run read one tree through the read-only tools and wrote into another
    // through the confined ones, so it was not doing the task it had been given.
    let root = tempfile::tempdir().expect("a temporary root");
    let tree = root.path().join("ws_existing");
    std::fs::create_dir(&tree).expect("the tree");
    std::fs::write(tree.join("already.txt"), "here first\n").expect("what was there before");

    let embedded = Embedded::open(root.path(), None).expect("the driver opens");
    let workspace = embedded.workspace_adopt("ws_existing").expect("adopted");

    // What was there before is readable through the confined backend: same tree, not a copy.
    assert_eq!(
        embedded
            .file_read(&workspace, "already.txt")
            .expect("reads what was there"),
        "here first\n"
    );

    // And a write lands beside it, on disk, where the read-only tools would see it too.
    embedded
        .file_write(&workspace, "added.txt", "and here after\n")
        .expect("writes");
    assert_eq!(
        std::fs::read_to_string(tree.join("added.txt")).expect("on disk"),
        "and here after\n",
        "one tree: what the confined tools wrote is what an ordinary reader finds"
    );
}

#[test]
fn a_directory_the_driver_cannot_represent_is_refused_by_name_and_says_what_to_do() {
    let root = tempfile::tempdir().expect("a temporary root");
    std::fs::create_dir(root.path().join("work-native")).expect("a badly named tree");
    let embedded = Embedded::open(root.path(), None).expect("the driver opens");

    let error = embedded
        .workspace_adopt("work-native")
        .expect_err("refused")
        .to_string();
    assert!(error.contains("must begin `ws_`"), "{error}");
    assert!(error.contains("Rename the directory"), "and what to do: {error}");

    // And a name that is legal but names nothing is a different refusal.
    let error = embedded
        .workspace_adopt("ws_absent")
        .expect_err("refused")
        .to_string();
    assert!(error.contains("is not a directory to adopt"), "{error}");
}
