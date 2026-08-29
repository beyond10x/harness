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
    fn file_read(&self, path: &str, window: ReadWindow) -> Result<Value, String> {
        self.local.file_read(path, window)
    }
    fn dir_list(&self, path: &str) -> Result<Value, String> {
        self.local.dir_list(path)
    }
    fn search(&self, p: &str, path: &str, options: &SearchOptions) -> Result<Value, String> {
        self.local.search(p, path, options)
    }
    fn find(&self, glob: &str, path: &str, max: Option<usize>) -> Result<Value, String> {
        self.local.find(glob, path, max)
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
    fn run(&self, argv: &[String]) -> Result<Value, Refused> {
        if !self.programs.iter().any(|allowed| allowed == &argv[0]) {
            return Err(Refusal::ProgramNotDeclared {
                program: argv[0].clone(),
                declared: self.programs.clone(),
            }
            .into());
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
        vec!["file_read", "dir_list", "search", "find"]
    );

    let everything = Catalogue::of(Everything::at(dir.path(), &["cargo"]));
    assert_eq!(
        names(&everything.search(None, None)),
        vec![
            "file_read",
            "dir_list",
            "search",
            "find",
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
    assert_eq!(names(&answer.output).len(), 7);
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
    assert_eq!(answer.output["text"], "     1\tfn marker() {}\n");

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
    // And it is on the answer as a **value**, not only in the sentence. Downstream, the sentence is
    // all that distinguished this from a compile error, so counting refusals meant matching prose.
    assert_eq!(
        answer.refusal,
        Some(Refusal::ProgramNotDeclared {
            program: "sh".to_owned(),
            declared: vec!["cargo".to_owned()],
        })
    );
    // One author for the words: what the model reads is what the name renders.
    assert_eq!(
        output(&answer),
        answer.refusal.expect("named just above").message()
    );
}

#[test]
fn a_failure_that_is_not_a_refusal_carries_no_name() {
    // The other half of the claim, and the one that keeps the first worth reading: if every failed
    // call were named, *the run would not do this* would be as unreadable as it was before.
    let dir = tree();
    let mut verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));
    let answer = verbs.call(&call(
        INVOKE_VERB,
        json!({"name": "file_read", "arguments": {"path": "nowhere.rs"}}),
    ));
    assert!(answer.failed, "{}", output(&answer));
    assert_eq!(answer.refusal, None, "a missing file is not a refusal");
}

#[test]
fn a_declared_program_runs_and_is_never_named_as_a_refusal() {
    let dir = tree();
    let mut verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));
    let answer = verbs.call(&call(
        INVOKE_VERB,
        json!({"name": "run", "arguments": {"argv": ["cargo", "test"]}}),
    ));
    assert!(!answer.failed, "{}", output(&answer));
    assert_eq!(answer.refusal, None);
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
        .file_read("../../etc/passwd", ReadWindow::whole())
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
    ] {
        assert!(
            refused
                .expect_err("refused")
                .contains("is not offered by this workspace")
        );
    }
    // Not offered at all is a different answer from *offered and this program is outside the set*,
    // and only the second is a refusal this run made by rule.
    let refused = local.run(&["cargo".to_owned()]).expect_err("refused");
    assert!(
        refused
            .message()
            .contains("is not offered by this workspace"),
        "{refused}"
    );
    assert_eq!(refused.refusal(), None, "a missing tool is not a refusal");
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
            "find",
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
        refused
            .message()
            .contains("is not a program this run may start"),
        "{refused}"
    );
    // The words are what the model reads; the name beside them is what a reader of the record
    // counts, and neither is derived from the other.
    assert_eq!(
        refused.refusal(),
        Some(&Refusal::ProgramNotDeclared {
            program: "/bin/echo".to_owned(),
            declared: vec!["/bin/sh".to_owned()],
        }),
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
        "and all seven"
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
    assert_eq!(
        names(&filtered),
        vec!["file_read", "dir_list", "search", "find"]
    );
    assert_eq!(filtered["total"], 7);
    assert_eq!(filtered["withheld_by_filter"], 3);
    let note = filtered["note"].as_str().expect("a note");
    assert!(note.contains("no arguments"), "and how to see them: {note}");

    // The unfiltered answer hid nothing, so it says nothing — the call the description asks for
    // stays exactly as short as it was.
    let all = catalogue.search(None, None);
    assert_eq!(names(&all).len(), 7);
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

/// One `file_write` through the verbs, as a model's call reaches them.
fn writing(verbs: &mut Verbs, path: &str) -> ToolOutcome {
    verbs.call(&call(
        INVOKE_VERB,
        json!({"name": "file_write", "arguments": {"path": path, "text": "whole"}}),
    ))
}

#[test]
fn a_step_of_a_walk_may_narrow_the_runs_scope_and_can_never_widen_it() {
    // A workflow node declares where its own step may write. The run's scope stays where it is and
    // both layers are asked, so a node takes writing away and has no arrangement of rules that
    // gives any back — which is what lets a projection generated by another component be obeyed
    // here at all.
    let dir = tree();
    let mut verbs =
        Verbs::new(Catalogue::of(Everything::at(dir.path(), &[])).scoped(store_scope()));
    verbs.catalogue_mut().narrow(Scope::of(vec![
        ScopeRule::parse("src/**=denied").expect("a rule"),
    ]));

    let refused = writing(&mut verbs, "src/main.rs");
    assert!(
        refused.failed,
        "the step's own rule binds: {}",
        output(&refused)
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/main.rs")).expect("still there"),
        "fn marker() {}\n",
        "refused before the write, not audited after it"
    );
    assert!(
        !writing(&mut verbs, "notes.md").failed,
        "a path neither layer names is still unrestricted"
    );

    // The run says `partial-only` here; the node saying `allowed` changes nothing.
    verbs.catalogue_mut().narrow(Scope::of(vec![
        ScopeRule::parse(".engineering/planning/**=allowed").expect("a rule"),
    ]));
    let widened = writing(&mut verbs, ".engineering/planning/story/a.md");
    assert!(widened.failed, "{}", output(&widened));
    assert!(
        output(&widened).contains("file_edit"),
        "and the refusal is the run's own, naming the way in: {}",
        output(&widened)
    );

    // An empty scope is what a step declaring none runs under: the run's, unchanged.
    verbs.catalogue_mut().narrow(Scope::default());
    assert!(!writing(&mut verbs, "src/main.rs").failed);
}

fn denying(root: &std::path::Path, rule: &str) -> Verbs {
    let local = LocalOperations::unconfined(root, Vec::new()).expect("opens");
    Verbs::new(
        Catalogue::of(local).scoped(Scope::of(vec![ScopeRule::parse(rule).expect("a rule")])),
    )
}

#[test]
fn a_denied_path_reached_through_a_link_inside_the_workspace_is_refused_by_where_it_lands() {
    // The scope is lexical and the provider follows a link that stays inside the workspace, so
    // `ok/link -> target/x` matched no rule and the write overwrote `target/x` under
    // `target/**=denied`. The catalogue now asks the provider where the path lands and puts that
    // spelling through the scope as well.
    let dir = tree();
    std::fs::create_dir_all(dir.path().join("target")).expect("target");
    std::fs::write(dir.path().join("target/x"), "built").expect("a file");
    std::fs::create_dir(dir.path().join("ok")).expect("ok");
    std::os::unix::fs::symlink(dir.path().join("target/x"), dir.path().join("ok/link"))
        .expect("a link");
    let mut verbs = denying(dir.path(), "target/**=denied");

    let answer = verbs.call(&call(
        INVOKE_VERB,
        json!({"name": "file_write", "arguments": {"path": "ok/link", "text": "owned"}}),
    ));

    assert!(answer.failed, "{}", output(&answer));
    let said = output(&answer);
    assert!(said.contains("ok/link"), "names the spelling: {said}");
    assert!(said.contains("target/x"), "and where it lands: {said}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("target/x")).expect("read"),
        "built",
        "nothing was written"
    );
}

#[test]
fn a_path_that_leaves_the_workspace_and_comes_back_is_judged_where_it_lands() {
    // `../<workspace>/target/y` keeps its leading `..` under lexical normalisation, so it matched
    // no workspace-relative glob; the provider then resolved it to `target/y`, inside the root,
    // and wrote into the denied tree.
    let dir = tree();
    let name = dir
        .path()
        .file_name()
        .expect("a name")
        .to_str()
        .expect("utf-8")
        .to_owned();
    let mut verbs = denying(dir.path(), "target/**=denied");

    let answer = verbs.call(&call(
        INVOKE_VERB,
        json!({
            "name": "file_write",
            "arguments": {"path": format!("../{name}/target/y"), "text": "owned"},
        }),
    ));

    assert!(answer.failed, "{}", output(&answer));
    assert!(
        !dir.path().join("target/y").exists(),
        "the write reached the denied tree"
    );
}

#[test]
fn reading_a_denied_path_is_still_reading_because_a_write_scope_bounds_writes() {
    let dir = tree();
    let mut verbs = Verbs::new(
        Catalogue::of(Everything::at(dir.path(), &[])).scoped(Scope::of(vec![
            ScopeRule::parse("**=denied").expect("a rule"),
        ])),
    );

    let answer = verbs.call(&call(
        INVOKE_VERB,
        json!({"name": "file_read", "arguments": {"path": "src/main.rs"}}),
    ));

    assert!(!answer.failed, "{}", output(&answer));
    assert_eq!(answer.output["text"], "     1\tfn marker() {}\n");
}

// --- the flat surface, over the same catalogue ---------------------------------------------------

#[test]
fn the_flat_surface_publishes_one_tool_per_entry_and_none_of_the_verbs() {
    // What the verbs cost and this does not: a per-tool schema the provider can validate against,
    // and no turn spent on `tool_search` before the first useful call.
    let dir = tree();
    let flat = Flat::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));

    let published: Vec<&str> = flat.specs().iter().map(|spec| spec.name.as_str()).collect();
    assert_eq!(
        published,
        vec![
            "file_read",
            "dir_list",
            "search",
            "find",
            "file_write",
            "file_edit",
            "run"
        ]
    );
    for verb in [SEARCH_VERB, DESCRIBE_VERB, INVOKE_VERB] {
        assert!(!published.contains(&verb), "`{verb}` is not published here");
    }
    assert!(
        flat.specs()[0].input_schema["properties"]["offset"].is_object(),
        "each tool carries its own arguments, which is the thing the verbs could not give a provider"
    );
    assert_eq!(
        flat.operations(),
        flat.catalogue().operations(),
        "what a run could do is still the catalogue's answer, not the surface's"
    );
    assert_eq!(
        flat.subjects(&call("file_write", json!({"path": "a.txt", "text": ""}))),
        vec![Subject::file("a.txt")],
        "the arguments are the entry's own, so nothing has to be unwrapped"
    );
}

#[test]
fn a_call_through_the_flat_surface_answers_what_the_verb_answers() {
    let dir = tree();
    let mut flat = Flat::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));
    let mut verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));

    let flatly = flat.call(&call("file_read", json!({"path": "src/main.rs"})));
    let through = verbs.call(&call(
        INVOKE_VERB,
        json!({"name": "file_read", "arguments": {"path": "src/main.rs"}}),
    ));
    assert!(!flatly.failed, "{}", output(&flatly));
    assert_eq!(flatly.output, through.output);
}

#[test]
fn an_unknown_tool_on_the_flat_surface_is_refused_by_name_listing_what_this_run_has() {
    let dir = tree();
    let mut flat = Flat::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));
    let answer = flat.call(&call("Bash", json!({"command": "rm -rf /"})));

    assert!(answer.failed);
    let said = output(&answer);
    assert!(said.contains("`Bash` is not a tool this run has"), "{said}");
    assert!(
        said.contains("file_read") && said.contains("run"),
        "and it lists what is: {said}"
    );
}

// --- a bare entry name through the verbs ---------------------------------------------------------

#[test]
fn an_entry_called_by_its_own_name_is_performed_rather_than_burning_a_turn() {
    // Measured on a live run under this surface: 10 of 82 tool calls were a bare entry name, each
    // refused as unpublished. The published list is still the three verbs — this is a route.
    let dir = tree();
    let mut verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));

    let bare = verbs.call(&call("file_read", json!({"path": "src/main.rs"})));
    let wrapped = verbs.call(&call(
        INVOKE_VERB,
        json!({"name": "file_read", "arguments": {"path": "src/main.rs"}}),
    ));
    assert!(!bare.failed, "{}", output(&bare));
    assert_eq!(bare.output, wrapped.output);

    let offered: Vec<&str> = verbs.specs().iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        offered,
        vec![SEARCH_VERB, DESCRIBE_VERB, INVOKE_VERB],
        "a route is not a publication"
    );
}

#[test]
fn a_bare_entry_name_is_gated_on_that_entrys_own_spec_and_subjects() {
    let dir = tree();
    let verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));

    let spec = verbs
        .invoked(&call("file_write", json!({"path": "a.txt", "text": "x"})))
        .expect("an entry this run has");
    assert_eq!(spec.name.as_str(), "file_write");
    assert_eq!(spec.envelope.risk, harness_wire::Risk::Medium);
    assert_eq!(
        verbs.subjects(&call("file_write", json!({"path": "a.txt", "text": "x"}))),
        vec![Subject::file("a.txt")],
        "the arguments are the entry's own here, not wrapped in a verb"
    );

    assert!(
        verbs.invoked(&call("Bash", json!({}))).is_none(),
        "a name that is neither a verb nor an entry reaches nothing"
    );
}

#[test]
fn a_bare_name_this_run_does_not_have_is_still_refused_and_lists_the_entries() {
    let dir = tree();
    let mut verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));
    let answer = verbs.call(&call("Bash", json!({"command": "id"})));

    assert!(answer.failed);
    let said = output(&answer);
    assert!(said.contains("`Bash` is not a tool this run has"), "{said}");
    assert!(said.contains("dir_list"), "listing what is: {said}");
}

// --- a batch of pure calls -----------------------------------------------------------------------

#[test]
fn a_batch_answers_one_result_per_call_in_the_position_it_was_asked_in() {
    let dir = tree();
    std::fs::write(dir.path().join("src/other.rs"), "fn other() {}\n").expect("a file");
    let mut flat = Flat::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));

    let calls = vec![
        call("file_read", json!({"path": "src/main.rs"})),
        call("file_read", json!({"path": "src/other.rs"})),
        call(
            "file_read",
            json!({"path": "src/main.rs", "offset": 1, "limit": 1}),
        ),
    ];
    let answers = flat.call_batch(&calls, None);

    assert_eq!(answers.len(), 3);
    assert!(answers.iter().all(|answer| !answer.failed));
    assert_eq!(answers[0].output["text"], "     1\tfn marker() {}\n");
    assert_eq!(answers[1].output["text"], "     1\tfn other() {}\n");
    assert_eq!(
        answers[2].output["lines"],
        json!({"from": 1, "to": 1, "total": 1})
    );
}

#[test]
fn a_batch_carrying_a_name_this_run_does_not_have_refuses_in_that_position_alone() {
    let dir = tree();
    let mut flat = Flat::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));

    let calls = vec![
        call("file_read", json!({"path": "src/main.rs"})),
        call("Bash", json!({"command": "id"})),
        call("dir_list", json!({"path": "src"})),
    ];
    let answers = flat.call_batch(&calls, None);

    assert!(!answers[0].failed);
    assert!(answers[1].failed);
    assert!(
        output(&answers[1]).contains("is not a tool this run has"),
        "{}",
        output(&answers[1])
    );
    assert!(!answers[2].failed, "the others still did their work");
}

/// A provider that reads like any other and panics on one named path.
///
/// The thing a batch had no answer for: `std::thread::scope` re-panics on its own thread when any
/// scoped thread panicked, joined or not, so one panicking call took every sibling's answer and the
/// caller with it.
struct Brittle {
    local: LocalOperations,
    explodes: &'static str,
}

impl Operations for Brittle {
    fn file_read(&self, path: &str, window: ReadWindow) -> Result<Value, String> {
        assert!(path != self.explodes, "this read was told to come apart");
        self.local.file_read(path, window)
    }
    fn dir_list(&self, path: &str) -> Result<Value, String> {
        self.local.dir_list(path)
    }
    fn search(&self, p: &str, path: &str, options: &SearchOptions) -> Result<Value, String> {
        self.local.search(p, path, options)
    }
    fn find(&self, glob: &str, path: &str, max: Option<usize>) -> Result<Value, String> {
        self.local.find(glob, path, max)
    }
    fn file_write(&self, path: &str, _text: &str) -> Result<Value, String> {
        Err(format!("`{path}`: this provider only reads"))
    }
    fn file_edit(&self, path: &str, _old: &str, _new: &str) -> Result<Value, String> {
        Err(format!("`{path}`: this provider only reads"))
    }
    fn run(&self, _argv: &[String]) -> Result<Value, Refused> {
        Err("this provider starts nothing".into())
    }
}

#[test]
fn a_call_that_panics_is_a_refusal_in_its_own_position_and_its_siblings_still_answer() {
    let dir = tree();
    std::fs::write(dir.path().join("src/other.rs"), "fn other() {}\n").expect("a file");
    let catalogue = Catalogue::of(Brittle {
        local: LocalOperations::new(dir.path()).expect("opens"),
        explodes: "src/boom.rs",
    });
    let arguments = [
        json!({"path": "src/main.rs"}),
        json!({"path": "src/boom.rs"}),
        json!({"path": "src/other.rs"}),
    ];
    let calls: Vec<(&str, &Value)> = arguments.iter().map(|a| ("file_read", a)).collect();

    let answers = catalogue.invoke_batch(&calls, None);

    assert_eq!(answers.len(), 3);
    assert!(answers[0].is_ok(), "{:?}", answers[0]);
    assert!(answers[2].is_ok(), "the sibling after it did its work too");
    let refusal = answers[1].as_ref().expect_err("refused");
    let refusal = refusal.message();
    assert!(refusal.contains("`file_read`"), "{refusal}");
    assert!(refusal.contains("panicked while running"), "{refusal}");
    assert!(
        refusal.contains("this read was told to come apart"),
        "with the panic's own words: {refusal}"
    );
}

#[test]
fn a_batch_larger_than_the_thread_bound_answers_every_call_in_its_own_position() {
    // One OS thread per call is right for the six reads batching was built for and wrong for the
    // two hundred a turn can hold, so the batch runs in chunks — and a chunked batch has to come
    // back in the order it was asked in.
    let dir = tempfile::tempdir().expect("a temporary tree");
    for index in 0..20 {
        std::fs::write(
            dir.path().join(format!("f{index}.txt")),
            format!("line {index}\n"),
        )
        .expect("a file");
    }
    let catalogue = Catalogue::of(LocalOperations::new(dir.path()).expect("opens"));
    let arguments: Vec<Value> = (0..20)
        .map(|index| json!({"path": format!("f{index}.txt")}))
        .collect();
    let calls: Vec<(&str, &Value)> = arguments.iter().map(|a| ("file_read", a)).collect();

    let answers = catalogue.invoke_batch(&calls, None);

    assert_eq!(answers.len(), 20);
    for (index, answer) in answers.iter().enumerate() {
        let value = answer.as_ref().expect("the read answers");
        assert_eq!(value["text"], json!(format!("     1\tline {index}\n")));
    }
}

#[test]
fn a_batch_of_one_is_the_same_answer_as_invoking_it_alone() {
    let dir = tree();
    let catalogue = Catalogue::of(Everything::at(dir.path(), &["cargo"]));
    let arguments = json!({"path": "src/main.rs"});

    let alone = catalogue.invoke_within("file_read", &arguments, None);
    let batched = catalogue.invoke_batch(&[("file_read", &arguments)], None);

    assert_eq!(batched.len(), 1);
    assert_eq!(batched[0], alone);
}

#[test]
fn the_verbs_batch_runs_invocations_together_and_answers_the_catalogue_questions_in_place() {
    let dir = tree();
    let mut verbs = Verbs::new(Catalogue::of(Everything::at(dir.path(), &["cargo"])));

    let calls = vec![
        call(SEARCH_VERB, json!({})),
        call(
            INVOKE_VERB,
            json!({"name": "file_read", "arguments": {"path": "src/main.rs"}}),
        ),
        call("file_read", json!({"path": "src/main.rs"})),
        call(DESCRIBE_VERB, json!({"name": "run"})),
    ];
    let answers = verbs.call_batch(&calls, None);

    assert_eq!(answers.len(), 4);
    assert_eq!(names(&answers[0].output).len(), 7);
    assert_eq!(answers[1].output["text"], "     1\tfn marker() {}\n");
    assert_eq!(
        answers[2].output, answers[1].output,
        "a bare name and a wrapped one are one call"
    );
    assert_eq!(answers[3].output["operation"], "shell");
}

// --- what asks a person --------------------------------------------------------------------------

#[test]
fn an_edit_is_asked_about_at_exactly_the_ceiling_a_whole_file_write_is() {
    // `--approve-up-to high` used to let `run` and a whole-file `file_write` through unasked and
    // stop at every `file_edit`, because `needs_approval` had a second clause about idempotency.
    // That pushed an unattended run toward rewriting whole files, which is the more dangerous act.
    // The declaration itself stays: it is what a workflow that re-runs a scope reads.
    let dir = tree();
    let catalogue = Catalogue::of(Everything::at(dir.path(), &["cargo"]));
    let edit = catalogue.get("file_edit").expect("an entry").spec();
    let write = catalogue.get("file_write").expect("an entry").spec();

    assert_eq!(
        edit.envelope.idempotency,
        harness_wire::Idempotency::NonIdempotent
    );
    assert_eq!(edit.envelope.risk, harness_wire::Risk::Medium);
    for envelope in [&edit.envelope, &write.envelope] {
        assert!(!envelope.needs_approval(harness_wire::Risk::Medium));
        assert!(envelope.needs_approval(harness_wire::Risk::Low));
    }
}
