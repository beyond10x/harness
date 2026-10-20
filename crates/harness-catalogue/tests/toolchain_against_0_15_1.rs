//! Toolchain calls through the catalogue, held to what harness 0.15.1 answered for an ordinary
//! call and to what `CHANGELOG.md` `[Unreleased]` says changed when they moved onto
//! `tool_toolchain::executor` (tools `0.4.0`).
//!
//! The executor's own suite is the `build-test` contract's, in the `tools` repository, and it runs
//! the executor over its own runner. These cases run it the way harness does — over the active
//! `Operations` — and write the 0.15.1 reply out as a literal, so a reply change in a later tools
//! pin reaches a red test and not only a changelog.

#![cfg(unix)]

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_catalogue::{
    Catalogue, LocalOperations, Operations, ReadWindow, Refusal, Refused, ResolvedProvider, Scope,
    ScopeRule, SearchOptions,
};
use serde_json::{Value, json};

/// A Cargo project the built-in Rust provider resolves in, with one formatted source.
fn crate_tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary workspace");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .expect("a manifest");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::fs::write(dir.path().join("src/lib.rs"), "pub fn formatted() {}\n").expect("a source");
    dir
}

/// The built-in Rust provider, as resolved in `root`.
fn rust(root: &Path) -> Vec<ResolvedProvider> {
    tool_toolchain::Registry::builtins()
        .expect("the built-ins")
        .resolve(root, Some(&["rust".to_owned()]))
        .expect("the built-in Rust provider resolves where this suite builds")
}

/// One provider read from `spec`, resolved in `root`.
fn custom(root: &Path, name: &str, spec: &str) -> Vec<ResolvedProvider> {
    let file = root.join(format!("{name}.yaml"));
    std::fs::write(&file, spec).expect("the spec");
    let mut registry = tool_toolchain::Registry::builtins().expect("the built-ins");
    registry.load_file(&file).expect("the spec loads");
    registry
        .resolve(root, Some(&[name.to_owned()]))
        .expect("the provider resolves")
}

/// The catalogue the local adapter serves, starting the providers' programs and nothing else.
fn local(root: &Path, providers: Vec<ResolvedProvider>) -> Catalogue {
    let programs = providers
        .iter()
        .flat_map(|provider| provider.programs.iter().cloned())
        .collect();
    let operations = LocalOperations::unconfined(root, programs).expect("a workspace");
    Catalogue::of_with_toolchains(operations, providers)
}

#[test]
fn an_ordinary_toolchain_call_answers_0_15_1s_reply_field_for_field() {
    let dir = crate_tree();
    let catalogue = local(dir.path(), rust(dir.path()));

    let answer = catalogue
        .invoke("rust_fmt_check", &json!({}))
        .expect("a formatted crate passes the check");

    // 0.15.1's reply: one process, so one run, answered as the run itself — `rust_fmt_check`'s
    // plan can start one process only, and that answer did not change.
    assert_eq!(
        answer,
        json!({
            "argv": ["cargo", "fmt", "--all", "--", "--check"],
            "exit": 0,
            "stdout": "",
            "stderr": "",
            "truncated": false,
            "omitted_bytes": 0,
            "timed_out": false,
            "timeout_ms": 600_000,
        })
    );
}

#[test]
fn rust_fmt_answers_runs_as_a_list_even_when_it_started_one_process() {
    // `rust_fmt` runs `rustfmt` once per named path, so its plan can start several processes and
    // the contract answers `runs` whatever the count. 0.15.1 answered the run itself for one path
    // and an array for several.
    let dir = crate_tree();
    let catalogue = local(dir.path(), rust(dir.path()));

    let answer = catalogue
        .invoke("rust_fmt", &json!({"paths": ["src/lib.rs"]}))
        .expect("rustfmt runs");

    let runs = answer["runs"]
        .as_array()
        .unwrap_or_else(|| panic!("`runs` is a list: {answer}"));
    assert_eq!(runs.len(), 1, "{answer}");
    assert_eq!(
        runs[0]["argv"],
        json!(["rustfmt", "src/lib.rs"]),
        "{answer}"
    );
    assert_eq!(runs[0]["exit"], json!(0), "{answer}");
    assert_eq!(
        answer.as_object().map(serde_json::Map::len),
        Some(1),
        "`runs` and nothing beside it: {answer}"
    );
}

/// Two processes: the first outlasts any time left it is given, the second leaves a marker.
const TWICE: &str = "version: 1\n\
     toolchains:\n\
     \x20 - name: twice\n\
     \x20   description: Two processes, the first longer than the time left.\n\
     \x20   project:\n\
     \x20     all: [twice.lock]\n\
     \x20   sandbox:\n\
     \x20     programs: [sleep, touch]\n\
     \x20     env:\n\
     \x20       HOME: \"{workspace}\"\n\
     \x20       PATH: /usr/local/bin:/usr/bin:/bin\n\
     \x20   tools:\n\
     \x20     - name: twice_run\n\
     \x20       description: Sleep, then leave a marker.\n\
     \x20       commands:\n\
     \x20         - argv: [sleep, \"5\"]\n\
     \x20         - argv: [touch, second-started]\n";

#[test]
fn a_plan_starts_no_second_process_once_the_time_left_is_gone() {
    // One deadline for the whole call, taken when it starts. 0.15.1 handed every argv of a plan
    // the same time left, so the second process started after the first had used all of it.
    let dir = tempfile::tempdir().expect("a temporary workspace");
    std::fs::write(dir.path().join("twice.lock"), "").expect("the marker");
    let catalogue = local(dir.path(), custom(dir.path(), "twice", TWICE));

    let answer = catalogue
        .invoke_within("twice_run", &json!({}), Some(Duration::from_millis(300)))
        .expect("the first process started, so the call answers its run");

    assert!(
        !dir.path().join("second-started").exists(),
        "the second process started after the time left was gone: {answer}"
    );
    let runs = answer["runs"]
        .as_array()
        .unwrap_or_else(|| panic!("`runs` is a list: {answer}"));
    assert_eq!(runs.len(), 1, "only the run that started: {answer}");
    assert_eq!(runs[0]["argv"], json!(["sleep", "5"]), "{answer}");
    assert_eq!(runs[0]["timed_out"], json!(true), "{answer}");
    // A list cut short by the deadline says so, or it reads as the whole plan (invariant 8).
    assert_eq!(answer["deadline_passed"], json!(true), "{answer}");
    assert_eq!(answer["not_started"], json!(1), "{answer}");
}

#[test]
fn a_role_cut_short_inside_a_provider_says_how_many_of_its_processes_never_started() {
    // The same cut, one level down: a role whose provider planned two processes and started one.
    let dir = tempfile::tempdir().expect("a temporary workspace");
    std::fs::write(dir.path().join("pair.lock"), "").expect("the marker");
    let spec = dir.path().join("pair.yaml");
    std::fs::write(&spec, CUT_PAIR).expect("the spec");
    let mut registry = tool_toolchain::Registry::builtins().expect("the built-ins");
    registry.load_file(&spec).expect("the spec loads");
    let providers = registry
        .resolve(dir.path(), Some(&["late".to_owned(), "slow".to_owned()]))
        .expect("both providers resolve");
    let catalogue = local(dir.path(), providers);

    let answer = catalogue
        .invoke_within(
            "toolchain_test",
            &json!({"late": {}, "slow": {}}),
            Some(Duration::from_millis(300)),
        )
        .expect("the first provider started");

    let late = &answer["toolchains"]["late"];
    assert_eq!(late["runs"].as_array().map(Vec::len), Some(1), "{answer}");
    assert_eq!(late["deadline_passed"], json!(true), "{answer}");
    assert_eq!(late["not_started"], json!(1), "{answer}");
    assert!(
        !dir.path().join("late-second").exists(),
        "the second process started after the deadline: {answer}"
    );
    let slow = &answer["toolchains"]["slow"];
    assert_eq!(slow["deadline_passed"], json!(true), "{answer}");
    assert_eq!(slow["not_started"], json!(1), "{answer}");
}

/// Two providers with a `test` role. `late` sorts first and runs first: a process that outlasts
/// the time left, then one that leaves a marker. `slow` comes after the deadline has passed.
const CUT_PAIR: &str = "version: 1\n\
     toolchains:\n\
     \x20 - name: late\n\
     \x20   description: Two processes, the first longer than the time left.\n\
     \x20   project:\n\
     \x20     all: [pair.lock]\n\
     \x20   sandbox:\n\
     \x20     programs: [sleep, touch]\n\
     \x20     env:\n\
     \x20       HOME: \"{workspace}\"\n\
     \x20       PATH: /usr/local/bin:/usr/bin:/bin\n\
     \x20   tools:\n\
     \x20     - name: late_test\n\
     \x20       description: Sleep, then leave a marker.\n\
     \x20       role: test\n\
     \x20       commands:\n\
     \x20         - argv: [sleep, \"5\"]\n\
     \x20         - argv: [touch, late-second]\n\
     \x20 - name: slow\n\
     \x20   description: One process.\n\
     \x20   project:\n\
     \x20     all: [pair.lock]\n\
     \x20   sandbox:\n\
     \x20     programs: [sleep]\n\
     \x20     env:\n\
     \x20       HOME: \"{workspace}\"\n\
     \x20       PATH: /usr/local/bin:/usr/bin:/bin\n\
     \x20   tools:\n\
     \x20     - name: slow_test\n\
     \x20       description: Sleep.\n\
     \x20       role: test\n\
     \x20       commands:\n\
     \x20         - argv: [sleep, \"5\"]\n";

const UNFORMATTED: &str = "pub fn   unformatted( ) {}\n";

#[test]
fn a_path_through_a_link_into_a_refused_directory_is_refused_and_nothing_starts() {
    let dir = crate_tree();
    std::fs::create_dir(dir.path().join("protected")).expect("the refused directory");
    std::fs::write(dir.path().join("protected/kept.rs"), UNFORMATTED).expect("a source");
    std::os::unix::fs::symlink("../protected/kept.rs", dir.path().join("src/alias.rs"))
        .expect("a link into the refused directory");
    let catalogue = local(dir.path(), rust(dir.path())).scoped(Scope::of(vec![
        ScopeRule::parse("protected/**=denied").expect("a rule"),
    ]));

    let refused = catalogue
        .invoke("rust_fmt", &json!({"paths": ["src/alias.rs"]}))
        .expect_err("a written path is judged where it lands");

    let said = refused.message();
    assert!(said.contains("src/alias.rs"), "names the spelling: {said}");
    assert!(
        said.contains("protected/kept.rs"),
        "and where it lands: {said}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("protected/kept.rs")).expect("read"),
        UNFORMATTED,
        "nothing rewrote the file behind the link"
    );
}

/// A provider with no directory on this host, which records every argv it is asked to start.
struct Elsewhere {
    programs: Vec<String>,
    started: Arc<Mutex<Vec<Vec<String>>>>,
}

impl Operations for Elsewhere {
    fn file_read(&self, _path: &str, _window: ReadWindow) -> Result<Value, String> {
        Err("nothing is read here".to_owned())
    }
    fn file_write(&self, _path: &str, _text: &str) -> Result<Value, String> {
        Err("nothing is written here".to_owned())
    }
    fn file_edit(&self, _path: &str, _old: &str, _new: &str) -> Result<Value, String> {
        Err("nothing is written here".to_owned())
    }
    fn dir_list(&self, _path: &str) -> Result<Value, String> {
        Err("nothing is listed here".to_owned())
    }
    fn search(&self, _p: &str, _path: &str, _options: &SearchOptions) -> Result<Value, String> {
        Err("nothing is searched here".to_owned())
    }
    fn run(&self, argv: &[String]) -> Result<Value, Refused> {
        self.started
            .lock()
            .expect("not poisoned")
            .push(argv.to_vec());
        Ok(json!({"stdout": "", "exit": 0}))
    }
    fn programs(&self) -> &[String] {
        &self.programs
    }
}

#[test]
fn a_toolchain_call_where_no_directory_on_this_host_can_be_judged_starts_nothing() {
    // Every path of a toolchain call is judged where it lands, and that needs the directory the
    // programs run in. A provider that names none is refused by name, not run unjudged.
    let dir = crate_tree();
    let started = Arc::new(Mutex::new(Vec::new()));
    let operations = Elsewhere {
        programs: vec!["cargo".to_owned(), "rustc".to_owned(), "rustfmt".to_owned()],
        started: Arc::clone(&started),
    };
    let catalogue = Catalogue::of_with_toolchains(operations, rust(dir.path()));

    let refused = catalogue
        .invoke("rust_fmt", &json!({"paths": ["src/lib.rs"]}))
        .expect_err("nothing can be judged, so nothing starts");

    let said = refused.message();
    assert!(said.contains("`rust_fmt`"), "names the call: {said}");
    assert!(said.contains("nothing was started"), "{said}");
    assert!(
        started.lock().expect("not poisoned").is_empty(),
        "an argv reached the provider: {said}"
    );
}

/// A provider that answers every run in a shape of its own, as the confined one does, and
/// refuses by name a program outside its own declared set.
struct Answering {
    tree: LocalOperations,
    programs: Vec<String>,
}

impl Operations for Answering {
    fn file_read(&self, path: &str, window: ReadWindow) -> Result<Value, String> {
        self.tree.file_read(path, window)
    }
    fn file_write(&self, _path: &str, _text: &str) -> Result<Value, String> {
        Err("nothing is written here".to_owned())
    }
    fn file_edit(&self, _path: &str, _old: &str, _new: &str) -> Result<Value, String> {
        Err("nothing is written here".to_owned())
    }
    fn dir_list(&self, path: &str) -> Result<Value, String> {
        self.tree.dir_list(path)
    }
    fn search(&self, pattern: &str, path: &str, options: &SearchOptions) -> Result<Value, String> {
        self.tree.search(pattern, path, options)
    }
    fn run(&self, argv: &[String]) -> Result<Value, Refused> {
        if !self.programs.contains(&argv[0]) {
            return Err(Refusal::ProgramNotDeclared {
                program: argv[0].clone(),
                declared: self.programs.clone(),
            }
            .into());
        }
        Ok(json!({"started": argv, "stdout": "", "observed": "by this provider"}))
    }
    fn host_root(&self) -> Option<&Path> {
        self.tree.host_root()
    }
    fn programs(&self) -> &[String] {
        &self.programs
    }
}

/// A second provider beside Rust's, whose `test` role starts two processes.
const PROBE: &str = "version: 1\n\
     toolchains:\n\
     \x20 - name: probe\n\
     \x20   description: A test role of two processes.\n\
     \x20   project:\n\
     \x20     all: [probe.lock]\n\
     \x20   sandbox:\n\
     \x20     programs: [\"true\"]\n\
     \x20     env:\n\
     \x20       HOME: \"{workspace}\"\n\
     \x20       PATH: /usr/local/bin:/usr/bin:/bin\n\
     \x20   tools:\n\
     \x20     - name: probe_test\n\
     \x20       description: Two passes.\n\
     \x20       role: test\n\
     \x20       commands:\n\
     \x20         - argv: [\"true\", first]\n\
     \x20         - argv: [\"true\", second]\n";

#[test]
fn a_role_over_two_providers_answers_each_providers_own_replies_under_toolchains() {
    // The executor shapes the answer and the provider says what each run did: a role over two
    // providers is `toolchains` by provider, as in 0.15.1, and each run in it is the reply the
    // provider gave, in the order its processes ran — not a `RunOutput` rebuilt from it.
    let dir = crate_tree();
    std::fs::write(dir.path().join("probe.lock"), "").expect("the marker");
    let spec = dir.path().join("probe.yaml");
    std::fs::write(&spec, PROBE).expect("the spec");
    let mut registry = tool_toolchain::Registry::builtins().expect("the built-ins");
    registry.load_file(&spec).expect("the probe spec loads");
    let providers = registry
        .resolve(dir.path(), Some(&["probe".to_owned(), "rust".to_owned()]))
        .expect("both providers resolve");
    let operations = Answering {
        tree: LocalOperations::new(dir.path()).expect("a workspace"),
        programs: vec!["cargo".to_owned(), "true".to_owned()],
    };
    let catalogue = Catalogue::of_with_toolchains(operations, providers);

    let answer = catalogue
        .invoke("toolchain_test", &json!({"probe": {}, "rust": {}}))
        .expect("both providers ran");

    let ran = |argv: Value| json!({"started": argv, "stdout": "", "observed": "by this provider"});
    assert_eq!(
        answer,
        json!({"toolchains": {
            "probe": {"runs": [ran(json!(["true", "first"])), ran(json!(["true", "second"]))]},
            "rust": ran(json!(["cargo", "test", "--locked", "--offline"])),
        }})
    );
}

#[test]
fn a_providers_own_refusal_keeps_its_name_through_the_executor() {
    // The executor carries a runner's refusal as a sentence; the record counts a refused program
    // by its name, so the provider's own `ProgramNotDeclared` is what the call answers.
    let dir = crate_tree();
    let operations = Answering {
        tree: LocalOperations::new(dir.path()).expect("a workspace"),
        programs: vec!["rustfmt".to_owned()],
    };
    let catalogue = Catalogue::of_with_toolchains(operations, rust(dir.path()));

    let refused = catalogue
        .invoke("rust_test", &json!({}))
        .expect_err("the provider does not start `cargo`");

    assert_eq!(
        refused.refusal(),
        Some(&Refusal::ProgramNotDeclared {
            program: "cargo".to_owned(),
            declared: vec!["rustfmt".to_owned()],
        }),
        "{refused}"
    );
}
