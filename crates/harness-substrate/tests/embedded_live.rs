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

    let names: Vec<&str> = catalogue
        .entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
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

/// `workspace_adopt`'s documentation admits exactly the names its body adopts.
///
/// Its `# Errors` section said the name "must begin `ws_` and hold only alphanumerics and
/// underscores" while the body seventeen lines below adopted `work-native`. Same commit as the
/// help-text defect (`0c31438`), same class, smaller surface: the prefix went at substrate `0.2.2`
/// and the sentence describing it did not.
///
/// The doc comment is read out of this crate's own source, because it is reachable from nowhere
/// else — and a doc comment nothing reads is exactly how this one survived the commit that
/// falsified it.
///
/// # Decoded, not searched for
///
/// Asserting that the sentence does not say `ws_` and does say "one path component" is satisfied by
/// a sentence naming the wrong characters: a rustdoc mutated to "alphanumerics and `_`" passed one
/// line below a case that adopts `work-native`. So the **alphabet the sentence names** is decoded
/// out of it and used to classify probe names, each of which is separately handed to the driver.
/// Sentence and body must agree on every one, and the refusal the body produces is held to the same
/// standard — it is what a caller reads when the name is wrong, and a rule they cannot act on is no
/// better than the one this story was opened about.
#[test]
fn the_documentation_on_workspace_adopt_admits_the_names_its_body_adopts() {
    // A hyphen, an underscore, a digit, a dot, three scripts whose letters are not ASCII, and a
    // name that would read as an option wherever it reaches an argv.
    const PROBES: [&str; 8] = [
        "my-project",
        "my_project",
        "project9",
        "my.project",
        "café",
        "Projekt-Übung",
        "日本語",
        "-rf",
    ];

    let root = tempfile::tempdir().expect("a temporary root");
    let embedded = Embedded::open(root.path(), None).expect("the driver opens");
    let mut adopts: std::collections::BTreeMap<&str, bool> = std::collections::BTreeMap::new();
    let mut a_refusal = String::new();
    for name in PROBES {
        std::fs::create_dir(root.path().join(name)).expect("a directory to offer");
        match embedded.workspace_adopt(name) {
            Ok(_) => {
                adopts.insert(name, true);
            }
            Err(error) => {
                a_refusal = error.to_string();
                adopts.insert(name, false);
            }
        }
    }
    assert!(
        adopts.values().any(|held| *held) && adopts.values().any(|held| !*held),
        "the probes have to fall on both sides of the rule or they measure nothing: {adopts:?}"
    );

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
    let doc = flowed(&doc.join(" "));

    let mut wrong: Vec<String> = Vec::new();
    for (named, text) in [
        ("its documentation", &doc),
        ("its refusal", &flowed(&a_refusal)),
    ] {
        if text.contains("ws_") {
            wrong.push(format!(
                "  {named} still demands a `ws_` prefix, which the body dropped at substrate 0.2.2"
            ));
            continue;
        }
        let Some(rule) = alphabet_stated_by(text) else {
            wrong.push(format!(
                "  {named} states no rule this case can read — it has to say `one path component \
                 of …, and may not …` — so it cannot be compared with what the body does"
            ));
            continue;
        };
        for (name, adopted) in &adopts {
            let says = rule.admits(name);
            if says != *adopted {
                wrong.push(format!(
                    "  {named} says `{name}` is {}, and the body {} it",
                    if says { "a legal name" } else { "illegal" },
                    if *adopted { "adopts" } else { "refuses" }
                ));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "the workspace-name rule `workspace_adopt` states and the one it keeps:\n{}",
        wrong.join("\n")
    );
}

/// The character rule a sentence states, decoded from the sentence's own words.
struct StatedRule {
    /// Whether it says the alphanumerics are `ASCII`, which is what the code checks.
    ascii_only: bool,
    /// The single characters it lists beside them.
    beside: std::collections::BTreeSet<char>,
    /// Whether it refuses a name that would read as an option.
    leading_dash_refused: bool,
}

impl StatedRule {
    /// Whether the rule, as stated, admits this name.
    fn admits(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        if self.leading_dash_refused && name.starts_with('-') {
            return false;
        }
        name.chars().all(|character| {
            (character.is_alphanumeric() && (!self.ascii_only || character.is_ascii()))
                || self.beside.contains(&character)
        })
    }
}

/// The alphabet clause of a stated rule: everything between `path component of` and the
/// `, and may not` that begins the shape rule.
///
/// Scoped to that clause on purpose. `` `-` `` appears again in "may not … begin with `-`", and a
/// reader that took every backticked character in the sentence would count the hyphen as admitted
/// however the alphabet clause was rewritten.
fn alphabet_stated_by(text: &str) -> Option<StatedRule> {
    let clause = text
        .split_once("path component of")?
        .1
        .split_once(", and may not")?
        .0;
    Some(StatedRule {
        ascii_only: clause.contains("ASCII"),
        beside: clause
            .split('`')
            .skip(1)
            .step_by(2)
            .filter_map(|token| {
                let mut characters = token.chars();
                let single = characters.next()?;
                characters.next().is_none().then_some(single)
            })
            .collect(),
        leading_dash_refused: text.contains("begin with `-`"),
    })
}

/// A doc comment's `///` markers stripped and its lines run together, so a rule wrapped across
/// three lines is one sentence to read.
fn flowed(text: &str) -> String {
    text.replace("///", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
    let workspace_access = b10x_harness_substrate::process_workspace_access(&[])
        .expect("an empty process write scope is read-only");

    let observation = embedded
        .exec(
            &workspace,
            &[inside.clone(), "staged".to_owned()],
            &workspace_access,
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
