use harness_wire::{CallId, Subject, ToolCall, ToolName, ToolOutcome, ToolPort};
use serde_json::{Value, json};

use super::*;

/// A provider that writes and runs, so the whole catalogue is exercised without substrate.
struct Everything {
    programs: Vec<String>,
    local: LocalOperations,
    written: std::sync::Mutex<Vec<(String, String)>>,
    ran: std::sync::Mutex<Vec<Vec<String>>>,
}

impl Everything {
    fn at(root: &std::path::Path, programs: &[&str]) -> Self {
        Self {
            programs: programs.iter().map(|p| (*p).to_owned()).collect(),
            local: LocalOperations::new(root).expect("opens"),
            written: std::sync::Mutex::new(Vec::new()),
            ran: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Operations for Everything {
    fn file_read(&self, path: &str, max_bytes: Option<u64>) -> Result<Value, String> {
        self.local.file_read(path, max_bytes)
    }
    fn dir_list(&self, path: &str) -> Result<Value, String> {
        self.local.dir_list(path)
    }
    fn search(&self, p: &str, path: &str, max: Option<usize>) -> Result<Value, String> {
        self.local.search(p, path, max)
    }
    fn file_write(&self, path: &str, text: &str) -> Result<Value, String> {
        self.written
            .lock()
            .expect("not poisoned")
            .push((path.to_owned(), text.to_owned()));
        Ok(json!({"path": path, "bytes": text.len()}))
    }
    fn file_edit(&self, path: &str, _old: &str, new: &str) -> Result<Value, String> {
        self.file_write(path, new)
    }
    fn run(&self, argv: &[String]) -> Result<Value, String> {
        if !self.programs.iter().any(|allowed| allowed == &argv[0]) {
            return Err(format!(
                "`{}` is not a program this run may start. Declared: {}.",
                argv[0],
                self.programs.join(", ")
            ));
        }
        self.ran.lock().expect("not poisoned").push(argv.to_vec());
        Ok(json!({"stdout": "", "exit": 0}))
    }
    fn programs(&self) -> &[String] {
        &self.programs
    }
    fn writes(&self) -> bool {
        true
    }
}

fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary tree");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::fs::write(dir.path().join("src/main.rs"), "fn marker() {}\n").expect("a file");
    dir
}

fn names(answer: &Value) -> Vec<String> {
    answer["tools"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|tool| tool["name"].as_str().expect("a name").to_owned())
        .collect()
}

fn call(name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        call_id: CallId::new("c-1").expect("valid"),
        name: ToolName::new(name).expect("valid"),
        arguments,
    }
}

fn output(outcome: &ToolOutcome) -> String {
    outcome.output.as_str().unwrap_or_default().to_owned()
}

// --- the publication gate -----------------------------------------------------------------------

#[test]
fn a_provider_that_cannot_write_contributes_no_writing_entry() {
    // The gate, in one assertion. The model is never told about a tool it cannot have, so it never
    // plans around one and never spends a turn being refused.
    let dir = tree();
    let read_only = Catalogue::of(LocalOperations::new(dir.path()).expect("opens"));
    assert_eq!(
        names(&read_only.search(None, None)),
        vec!["file_read", "dir_list", "search"]
    );

    let everything = Catalogue::of(Everything::at(dir.path(), &["cargo"]));
    assert_eq!(
        names(&everything.search(None, None)),
        vec![
            "file_read",
            "dir_list",
            "search",
            "file_write",
            "file_edit",
            "run"
        ]
    );
}

#[test]
fn a_provider_with_no_declared_programs_publishes_no_run_even_though_it_can_execute() {
    // A workflow that named no commands wants none. A tool that admitted everything because nobody
    // listed anything is the failure this whole design exists to prevent.
    let dir = tree();
    let catalogue = Catalogue::of(Everything::at(dir.path(), &[]));
    assert!(!names(&catalogue.search(None, None)).contains(&"run".to_owned()));
}

// --- the three verbs ----------------------------------------------------------------------------

#[test]
fn the_model_is_offered_exactly_three_tools_whatever_the_catalogue_holds() {
    let dir = tree();
    for programs in [&[][..], &["cargo"][..]] {
        let verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), programs)));
        let offered: Vec<&str> = verbs.specs().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(offered, vec![SEARCH_VERB, DESCRIBE_VERB, INVOKE_VERB]);
    }
}

#[test]
fn search_with_no_argument_answers_the_whole_catalogue_because_it_is_short() {
    let dir = tree();
    let mut verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));
    let answer = verbs.call(&call(SEARCH_VERB, json!({})));
    assert!(!answer.failed);
    assert_eq!(names(&answer.output).len(), 6);
}

#[test]
fn search_narrows_by_effect_so_a_reader_can_ask_what_changes_anything() {
    let dir = tree();
    let mut verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));
    let answer = verbs.call(&call(SEARCH_VERB, json!({"effect": "write"})));
    assert_eq!(names(&answer.output), vec!["file_write", "file_edit"]);

    let answer = verbs.call(&call(SEARCH_VERB, json!({"query": "substring"})));
    assert_eq!(names(&answer.output), vec!["search"]);
}

#[test]
fn describe_answers_the_arguments_and_the_envelope_and_never_invents_a_tool() {
    let dir = tree();
    let mut verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));

    let answer = verbs.call(&call(DESCRIBE_VERB, json!({"name": "file_edit"})));
    assert!(!answer.failed);
    assert_eq!(answer.output["operation"], "file.edit");
    assert_eq!(answer.output["envelope"]["idempotency"], "non_idempotent");
    assert!(answer.output["input_schema"]["properties"]["old"].is_object());

    let answer = verbs.call(&call(DESCRIBE_VERB, json!({"name": "Bash"})));
    assert!(answer.failed);
    let said = output(&answer);
    assert!(said.contains("`Bash` is not a tool this run has"), "{said}");
    assert!(
        said.contains("file_read") && said.contains("run"),
        "and it lists what is: {said}"
    );
}

#[test]
fn invoking_a_tool_this_run_does_not_have_is_refused_here_with_nothing_performed() {
    let dir = tree();
    let provider = Everything::at(dir.path(), &["cargo"]);
    let mut verbs = Verbs::new(Catalogue::of(provider));
    let answer = verbs.call(&call(
        INVOKE_VERB,
        json!({"name": "Bash", "arguments": {"command": "rm -rf /"}}),
    ));
    assert!(answer.failed);
    assert!(
        output(&answer).contains("is not a tool this run has"),
        "{}",
        output(&answer)
    );
}

#[test]
fn invoking_reaches_the_provider_and_carries_its_own_words_back() {
    let dir = tree();
    let mut verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));

    let answer = verbs.call(&call(
        INVOKE_VERB,
        json!({"name": "file_read", "arguments": {"path": "src/main.rs"}}),
    ));
    assert!(!answer.failed, "{}", output(&answer));
    assert_eq!(answer.output["text"], "fn marker() {}\n");

    // A program outside the declared set is the provider's refusal, verbatim.
    let answer = verbs.call(&call(
        INVOKE_VERB,
        json!({"name": "run", "arguments": {"argv": ["sh", "-c", "id"]}}),
    ));
    assert!(answer.failed);
    assert!(
        output(&answer).contains("not a program this run may start"),
        "{}",
        output(&answer)
    );
}

#[test]
fn a_missing_required_argument_names_the_field_and_the_tool() {
    let dir = tree();
    let mut verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));
    let answer = verbs.call(&call(
        INVOKE_VERB,
        json!({"name": "file_write", "arguments": {"path": "a.txt"}}),
    ));
    assert!(answer.failed);
    assert!(
        output(&answer).contains("`text` is required by `file_write`"),
        "{}",
        output(&answer)
    );
}

// --- subjects -----------------------------------------------------------------------------------

#[test]
fn the_subject_of_an_invocation_is_the_entrys_and_not_the_verbs() {
    // A gate that read `tool_invoke`'s own arguments would see one opaque blob for every call in the
    // run. What it needs is the file or the program underneath.
    let dir = tree();
    let verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));

    assert_eq!(
        verbs.subjects(&call(
            INVOKE_VERB,
            json!({"name": "file_write", "arguments": {"path": "../../etc/passwd", "text": ""}})
        )),
        vec![Subject::file("../../etc/passwd")],
        "and as the caller wrote it: a gate has to see where the call was trying to go"
    );
    assert_eq!(
        verbs.subjects(&call(
            INVOKE_VERB,
            json!({"name": "run", "arguments": {"argv": ["cargo", "test"]}})
        )),
        vec![Subject::process("cargo")],
        "the program, not the whole argv"
    );
    assert!(
        verbs.subjects(&call(SEARCH_VERB, json!({}))).is_empty(),
        "listing tools touches nothing a policy could name"
    );
}

// --- the local provider's containment, carried across unchanged ---------------------------------

#[test]
fn a_path_that_leaves_the_tree_is_refused_by_where_it_lands() {
    let dir = tree();
    let local = LocalOperations::new(dir.path()).expect("opens");
    let refused = local
        .file_read("../../etc/passwd", None)
        .expect_err("refused");
    assert!(
        refused.contains("resolves outside the workspace") || refused.contains("No such"),
        "{refused}"
    );
}

#[test]
fn the_local_provider_offers_nothing_that_outlives_the_call() {
    // Not a policy: there is no boundary here, so an effect that outlives the call has nothing under
    // it. The catalogue never offers these, and a caller who bypassed it is refused rather than
    // served.
    let dir = tree();
    let local = LocalOperations::new(dir.path()).expect("opens");
    assert!(!local.writes());
    assert!(local.programs().is_empty());
    for refused in [
        local.file_write("a.txt", "x"),
        local.file_edit("a.txt", "x", "y"),
        local.run(&["cargo".to_owned()]),
    ] {
        assert!(
            refused
                .expect_err("refused")
                .contains("is not offered by this workspace")
        );
    }
}

// --- the unconfined provider, which has to be asked for by name ----------------------------------

#[test]
fn a_read_only_provider_and_an_unconfined_one_differ_only_in_what_they_admit() {
    let dir = tree();
    assert!(!LocalOperations::new(dir.path()).expect("opens").writes());

    let writing =
        LocalOperations::unconfined(dir.path(), vec!["/bin/echo".to_owned()]).expect("opens");
    assert!(writing.writes());
    assert_eq!(writing.programs(), ["/bin/echo"]);
    assert_eq!(
        names(&Catalogue::of(writing).search(None, None)),
        vec![
            "file_read",
            "dir_list",
            "search",
            "file_write",
            "file_edit",
            "run"
        ]
    );
}

#[test]
fn an_unconfined_write_creates_the_file_and_the_directories_over_it() {
    let dir = tree();
    let local = LocalOperations::unconfined(dir.path(), Vec::new()).expect("opens");

    local
        .file_write("docs/notes/one.md", "written\n")
        .expect("the write lands");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("docs/notes/one.md")).expect("on disk"),
        "written\n"
    );
}

#[test]
fn a_write_to_a_path_that_does_not_exist_yet_is_still_held_inside_the_tree() {
    // `resolve` cannot answer here — the file is not there to canonicalise — so this is the one
    // place a second containment rule lives, and it is the one a caller reaches for to escape.
    let dir = tree();
    let local = LocalOperations::unconfined(dir.path(), Vec::new()).expect("opens");

    for outside in ["../escaped.txt", "src/../../escaped.txt"] {
        let refused = local.file_write(outside, "nope").expect_err("refused");
        assert!(refused.contains("refused"), "{outside}: {refused}");
    }

    let link = dir.path().join("link");
    std::os::unix::fs::symlink("/etc", &link).expect("a link out of the tree");
    let refused = local
        .file_write("link/escaped.txt", "nope")
        .expect_err("refused");
    assert!(
        refused.contains("resolves outside the workspace"),
        "through a symlink, by where it lands: {refused}"
    );
}

#[test]
fn an_edit_that_matches_twice_changes_nothing_and_says_how_many() {
    let dir = tree();
    let local = LocalOperations::unconfined(dir.path(), Vec::new()).expect("opens");
    local
        .file_write("twice.rs", "let x = 1;\nlet x = 1;\n")
        .expect("the file");

    let refused = local
        .file_edit("twice.rs", "let x = 1;", "let x = 2;")
        .expect_err("refused");
    assert!(refused.contains("appears 2 times"), "{refused}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("twice.rs")).expect("on disk"),
        "let x = 1;\nlet x = 1;\n",
        "and the file is exactly as it was"
    );

    let refused = local
        .file_edit("twice.rs", "absent", "x")
        .expect_err("refused");
    assert!(refused.contains("appears nowhere"), "{refused}");

    local
        .file_edit("twice.rs", "let x = 1;\nlet", "let x = 2;\nlet")
        .expect("one place, named by its surroundings");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("twice.rs")).expect("on disk"),
        "let x = 2;\nlet x = 1;\n"
    );
}

#[test]
fn run_starts_a_declared_program_in_the_tree_and_nothing_else() {
    let dir = tree();
    let local = LocalOperations::unconfined(dir.path(), vec!["/bin/sh".to_owned()]).expect("opens");

    let answer = local
        .run(&[
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            "pwd; exit 3".to_owned(),
        ])
        .expect("it runs");
    assert_eq!(answer["exit"], 3, "the child's own code, not a verdict");
    assert_eq!(answer["timed_out"], false);
    assert!(
        answer["stdout"].as_str().expect("stdout").contains(
            &dir.path()
                .canonicalize()
                .expect("real")
                .display()
                .to_string()
        ),
        "the working directory is the workspace: {}",
        answer["stdout"]
    );

    let refused = local.run(&["/bin/echo".to_owned()]).expect_err("refused");
    assert!(
        refused.contains("is not a program this run may start"),
        "{refused}"
    );
}

#[test]
fn a_provider_that_writes_but_declared_no_program_publishes_no_run() {
    // The two are separate questions. Somebody who wanted files changed did not thereby ask for a
    // process, and a set nobody named means nobody wanted one.
    let dir = tree();
    let local = LocalOperations::unconfined(dir.path(), Vec::new()).expect("opens");
    let listed = names(&Catalogue::of(local).search(None, None));
    assert!(listed.contains(&"file_write".to_owned()));
    assert!(!listed.contains(&"run".to_owned()));
}

// --- the vocabulary, without a provider ----------------------------------------------------------

#[test]
fn every_entry_a_catalogue_can_hold_answers_to_exactly_one_operation() {
    // The gap this closes: a run is judged from its record, long after the catalogue that answered
    // it is gone. `tool_invoke {"name": "file_write"}` has to be readable as `file.write` by a
    // consumer that never held a provider — otherwise every consumer keeps its own copy of the
    // table, and the copies drift. That drift is what this tool surface exists to remove.
    let dir = tree();
    let catalogue = Catalogue::of(Everything::at(dir.path(), &["cargo"]));
    for entry in catalogue.entries() {
        assert_eq!(
            operation_of(entry.name),
            Some(entry.operation),
            "`{}` reads as a different operation off the record than in the catalogue",
            entry.name
        );
    }
    assert_eq!(
        catalogue.entries().len(),
        entry_names().len(),
        "and all six"
    );
    assert_eq!(
        operation_of("Bash"),
        None,
        "a vendor name is not in this vocabulary"
    );
}

#[test]
fn a_filtered_search_says_what_it_withheld_so_a_filter_is_not_a_ceiling() {
    // Measured, not imagined: a run on 2026-08-24 opened with `tool_search {"effect":"read"}`,
    // got three tools, never learnt that `file_write`, `file_edit` and `run` existed, and reported
    // a task done that it had not done. The filter the model chose became a ceiling it could not
    // see, because the answer looked complete.
    let dir = tree();
    let catalogue = Catalogue::of(Everything::at(dir.path(), &["cargo"]));

    let filtered = catalogue.search(None, Some("read"));
    assert_eq!(names(&filtered), vec!["file_read", "dir_list", "search"]);
    assert_eq!(filtered["total"], 6);
    assert_eq!(filtered["withheld_by_filter"], 3);
    let note = filtered["note"].as_str().expect("a note");
    assert!(note.contains("no arguments"), "and how to see them: {note}");

    // The unfiltered answer hid nothing, so it says nothing — the call the description asks for
    // stays exactly as short as it was.
    let all = catalogue.search(None, None);
    assert_eq!(names(&all).len(), 6);
    assert!(all.get("withheld_by_filter").is_none(), "{all}");
    assert!(all.get("note").is_none());
}

// --- the write scope ----------------------------------------------------------------------------

/// The store's rule, in the shape a step map declares it.
fn store_scope() -> Scope {
    Scope::of(vec![
        ScopeRule::parse(".engineering/planning/**=partial-only").expect("a rule"),
        ScopeRule::parse("**=allowed").expect("a rule"),
    ])
}

#[test]
fn a_whole_file_write_under_a_partial_only_path_is_refused_and_nothing_is_written() {
    let dir = tree();
    let mut verbs =
        Verbs::new(Catalogue::of(Everything::at(dir.path(), &[])).scoped(store_scope()));

    let answer = verbs.call(&call(
        INVOKE_VERB,
        json!({
            "name": "file_write",
            "arguments": {"path": ".engineering/planning/story/a.md", "text": "whole"},
        }),
    ));

    assert!(answer.failed, "the call is refused, not performed");
    // The refusal has to be usable, or the model retries it until the turn budget is gone.
    let said = output(&answer);
    assert!(said.contains(".engineering/planning/story/a.md"), "{said}");
    assert!(said.contains("file_edit"), "names the way in: {said}");
    assert!(
        !dir.path().join(".engineering/planning/story/a.md").exists(),
        "the provider was never reached"
    );
}

#[test]
fn a_partial_edit_under_the_same_path_goes_through_because_that_is_the_distinction() {
    let dir = tree();
    let mut verbs =
        Verbs::new(Catalogue::of(Everything::at(dir.path(), &[])).scoped(store_scope()));

    let answer = verbs.call(&call(
        INVOKE_VERB,
        json!({
            "name": "file_edit",
            "arguments": {"path": ".engineering/planning/story/a.md", "old": "x", "new": "y"},
        }),
    ));

    assert!(
        !answer.failed,
        "an edit is what the scope admits: {}",
        output(&answer)
    );
}

#[test]
fn a_path_no_rule_names_is_unrestricted_because_a_scope_declares_where_writing_is_bounded() {
    let dir = tree();
    let mut verbs = Verbs::new(
        Catalogue::of(Everything::at(dir.path(), &[])).scoped(Scope::of(vec![
            ScopeRule::parse(".engineering/**=denied").expect("a rule"),
        ])),
    );

    let answer = verbs.call(&call(
        INVOKE_VERB,
        json!({"name": "file_write", "arguments": {"path": "src/main.rs", "text": "fn main() {}"}}),
    ));

    assert!(!answer.failed, "{}", output(&answer));
}

#[test]
fn reading_a_denied_path_is_still_reading_because_a_write_scope_bounds_writes() {
    let dir = tree();
    let mut verbs = Verbs::new(
        Catalogue::of(Everything::at(dir.path(), &[]))
            .scoped(Scope::of(vec![ScopeRule::parse("**=denied").expect("a rule")])),
    );

    let answer = verbs.call(&call(
        INVOKE_VERB,
        json!({"name": "file_read", "arguments": {"path": "src/main.rs"}}),
    ));

    assert!(!answer.failed, "{}", output(&answer));
    assert_eq!(answer.output["text"], "fn marker() {}\n");
}
