//! The embedded driver, doing the thing, with no daemon anywhere.
//!
//! Not ignored: it needs no deployment, no socket and no credential — substrate's driver opens a
//! directory this test makes and hands back. That is the whole argument for embedding, and it is
//! checked on every run rather than when somebody remembers.
//!
//! Execution is asked for only where the machine admits it. A host with no delegated cgroup subtree
//! reports no exec facts, offers no `run` entry, and this test says so instead of failing — the
//! point is that the catalogue follows the machine, not that every machine is the same.

use b10x_harness_substrate::{Backend, ConfinedOperations, Embedded};
use harness_tools::Catalogue;
use serde_json::json;

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
        embedded
            .file_read(&workspace, "hello.txt")
            .expect("reads back"),
        "one\ntwo\n",
        "the bytes that went in are the bytes that come out"
    );
}

#[test]
fn the_catalogue_follows_the_machine_and_the_entries_do_not_know_which_backend_they_hold() {
    let root = tempfile::tempdir().expect("a temporary root");
    let embedded = Embedded::open(root.path(), None).expect("the driver opens");
    let facts = embedded.machine().expect("facts");
    let workspace = embedded.workspace_create(600_000).expect("a workspace");

    let catalogue = Catalogue::of(ConfinedOperations::new(
        Embedded::open(root.path(), None).expect("a second handle on the same root"),
        &facts,
        &workspace,
        vec!["/bin/echo".to_owned()],
    ));

    let names: Vec<&str> = catalogue.entries().iter().map(|entry| entry.name).collect();
    assert!(names.contains(&"file_write"), "{names:?}");
    assert_eq!(
        names.contains(&"run"),
        facts.confines_execution(),
        "`run` is offered exactly where the machine can confine one: {names:?}"
    );

    catalogue
        .invoke(
            "file_write",
            &json!({"path": "written.txt", "text": "by a tool\n"}),
        )
        .expect("the write lands");
    assert_eq!(
        embedded
            .file_read(&workspace, "written.txt")
            .expect("reads back"),
        "by a tool\n"
    );

    // The local refusal holds whichever backend is underneath: nothing was sent anywhere.
    let refused = catalogue
        .invoke("run", &json!({"argv": ["sh", "-c", "id"]}))
        .expect_err("refused");
    assert!(
        refused.message().contains("not a program")
            || refused.message().contains("not a tool this run has"),
        "{refused}"
    );
}

#[test]
fn a_tree_that_already_exists_is_adopted_rather_than_replaced_by_an_empty_one() {
    // The gap this closes: a run read one tree through the reading provider and wrote into another
    // through the confined one, so it was not doing the task it had been given.
    let root = tempfile::tempdir().expect("a temporary root");
    let tree = root.path().join("ws_existing");
    std::fs::create_dir(&tree).expect("the tree");
    std::fs::write(tree.join("already.txt"), "here first\n").expect("what was there before");

    let embedded = Embedded::open(root.path(), None).expect("the driver opens");
    let workspace = embedded.workspace_adopt("ws_existing").expect("adopted");

    assert_eq!(
        embedded
            .file_read(&workspace, "already.txt")
            .expect("reads what was there"),
        "here first\n"
    );

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
fn a_project_directory_is_adopted_under_its_own_name_and_a_bad_one_says_what_to_do() {
    // **This test asserted the opposite until substrate 0.2.2**, and the reversal is the feature:
    // `work-native` was refused for not beginning `ws_`, which meant a run could only ever be
    // pointed at a scratch copy of a project rather than the project. The prefix was substrate's
    // resource-id scheme, never its containment — that is `openat2` beneath the pinned root
    // descriptor with symlinks refused, and it has not moved.
    let root = tempfile::tempdir().expect("a temporary root");
    std::fs::create_dir(root.path().join("work-native")).expect("a project tree");
    let embedded = Embedded::open(root.path(), None).expect("the driver opens");

    assert_eq!(
        embedded
            .workspace_adopt("work-native")
            .expect("a hyphenated project directory is a workspace now"),
        "work-native",
        "the id is the directory's own name, so the two cannot disagree"
    );

    // What still refuses, and it is everything that carries containment: a name that is not one
    // path component, and one that would read as a flag where the name reaches an argv.
    for bad in [".", "..", "a/b", "-rf", ""] {
        let error = embedded
            .workspace_adopt(bad)
            .expect_err("refused")
            .to_string();
        // Either refusal is correct and which one fires is not this test's business: the driver
        // checks the root identity first and answers `workspace.path-escape`, and this crate's own
        // charset check answers with a sentence naming the rule. What matters is that none of
        // these ever becomes a workspace.
        assert!(
            error.contains("one path component")
                || error.contains("not a directory to adopt")
                || error.contains("path-escape"),
            "`{bad}` must be refused: {error}"
        );
    }

    let error = embedded
        .workspace_adopt("absent")
        .expect_err("refused")
        .to_string();
    assert!(error.contains("is not a directory to adopt"), "{error}");
}

/// `workspace_adopt`'s own documentation states the rule the case above measures.
///
/// Its `# Errors` section said the name "must begin `ws_` and hold only alphanumerics and
/// underscores" while the body seventeen lines below adopted `work-native`. Same commit as the
/// help-text defect (`0c31438`), same class, smaller surface: the prefix went at substrate `0.2.2`
/// and the sentence describing it did not.
///
/// Read out of this crate's own source, because a doc comment is reachable from nowhere else — and
/// a doc comment nothing reads is exactly how this one survived the commit that falsified it. What
/// is asserted is the pair the case above proves: the dropped rule is not named, and the rule that
/// is enforced is, in the words the refusal uses.
#[test]
fn the_documentation_on_workspace_adopt_states_the_rule_its_body_enforces() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("embedded.rs");
    let text = std::fs::read_to_string(&source)
        .unwrap_or_else(|error| panic!("reading `{}`: {error}", source.display()));

    let signature = "pub fn workspace_adopt(";
    let lines: Vec<&str> = text.lines().collect();
    let declared = lines
        .iter()
        .position(|line| line.contains(signature))
        .unwrap_or_else(|| panic!("`{signature}` is in `{}`", source.display()));
    let doc: Vec<&str> = lines[..declared]
        .iter()
        .rev()
        .take_while(|line| line.trim_start().starts_with("///"))
        .copied()
        .collect();
    assert!(
        !doc.is_empty(),
        "`workspace_adopt` carries a doc comment, or there is nothing here to check"
    );
    let doc = doc.join("\n");

    assert!(
        !doc.contains("ws_"),
        "the documentation on `workspace_adopt` still demands a `ws_` prefix its body dropped at          substrate 0.2.2 — the case above adopts `work-native`:\n{doc}"
    );
    assert!(
        doc.contains("one path component"),
        "and it has to state the rule the body does enforce, in the words the refusal uses:\n{doc}"
    );
}

#[test]
fn a_staged_driver_is_a_program_the_confined_run_can_actually_start() {
    // The whole point, end to end. A driven run allow-listed its own CLI by absolute host path,
    // the sandbox had no such file, `run` died at `ENOENT`, and the model — told to reach the
    // planning store only through that CLI — wrote the store's files directly instead. The
    // allow-list admits the name; only a mount admits the file.
    let Some(cgroup_root) = std::env::var_os("B10X_CGROUP_ROOT").map(std::path::PathBuf::from)
    else {
        // No delegated subtree named, so this machine admits no exec and the question does not
        // arise. Same shape as the catalogue test above: the machine decides, not the test.
        return;
    };

    let host = tempfile::tempdir().expect("a directory to build in");
    let program = host.path().join("driver-under-test");
    std::fs::write(
        &program,
        "#!/bin/sh\nprintf 'the driver ran as %s\\n' \"$1\"\n",
    )
    .expect("a program");
    std::fs::set_permissions(
        &program,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .expect("it is executable");

    let stage = tempfile::tempdir().expect("a stage");
    let toolchain = b10x_harness_substrate::Toolchain::default()
        .with_driver(&program, stage.path())
        .expect("the driver stages");
    let inside = toolchain
        .driver()
        .expect("a driver was declared")
        .program()
        .to_owned();

    let root = tempfile::tempdir().expect("a temporary root");
    let embedded =
        Embedded::open_with(root.path(), Some(cgroup_root), toolchain).expect("the driver opens");
    let facts = embedded.machine().expect("facts");
    if !facts.confines_execution() {
        // The subtree named is not one this process sits inside, so substrate reports no exec
        // facts. Nothing to assert about a tool the machine will not publish.
        return;
    }
    let workspace = embedded.workspace_create(600_000).expect("a workspace");

    let observation = embedded
        .exec(
            &workspace,
            &[inside.clone(), "staged".to_owned()],
            Some(std::time::Duration::from_secs(30)),
        )
        .expect("the exec starts");

    assert_eq!(
        observation["exit"]["exit"]["code"],
        json!(0),
        "the staged program ran: {observation:#?}"
    );
    // And substrate says which host directory it admitted to make that possible — the stage, never
    // the directory the program was built in.
    let mounted = observation["exit"].to_string();
    assert!(
        mounted.contains(r#""mount":"/toolchain/driver""#),
        "substrate reports the host directory it admitted, and this is where a reader sees that it \
         was the stage and not the directory the program was built in: {observation:#?}"
    );
    assert!(
        observation["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("the driver ran as staged"),
        "and it is the program that was staged, not something else on the path: {observation:#?}"
    );
}
