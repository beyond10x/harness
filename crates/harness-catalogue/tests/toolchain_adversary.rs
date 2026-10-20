//! Toolchain calls through the catalogue, driven against what 0.15.1 answered and what
//! `CHANGELOG.md` `[Unreleased]` says changed when they moved onto `tool_toolchain::executor`.
//!
//! Each case names the 0.15.1 behaviour it holds the executor path to, or the changelog sentence
//! it drives. Programs are `touch` and `sleep`, so the cases run wherever this suite builds; one
//! case runs the built-in Go provider and needs `go` and `gofmt` on `PATH`.

#![cfg(unix)]

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_catalogue::{
    Catalogue, LocalOperations, Operations, ReadWindow, Refused, ResolvedProvider, Scope,
    ScopeRule, SearchOptions,
};
use serde_json::{Value, json};

/// The providers `spec` declares, resolved in `root` by `names`.
fn resolve(root: &Path, spec: &str, names: &[&str]) -> Vec<ResolvedProvider> {
    std::fs::write(root.join("probe.lock"), "").expect("the project marker");
    let file = root.join("providers.yaml");
    std::fs::write(&file, spec).expect("the spec");
    let mut registry = tool_toolchain::Registry::builtins().expect("the built-ins");
    registry.load_file(&file).expect("the spec loads");
    let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    registry
        .resolve(root, Some(&names))
        .expect("the providers resolve")
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

fn denied(glob: &str) -> Scope {
    Scope::of(vec![
        ScopeRule::parse(&format!("{glob}=denied")).expect("a rule"),
    ])
}

/// Two providers with a `test` role: `alpha` leaves a marker, `beta` touches each named item.
const ROLE_PAIR: &str = r#"version: 1
toolchains:
  - name: alpha
    description: A test role of one process that leaves a marker.
    project:
      all: [probe.lock]
    sandbox:
      programs: [touch]
      env:
        HOME: "{workspace}"
        PATH: /usr/local/bin:/usr/bin:/bin
    tools:
      - name: alpha_test
        description: Leave a marker.
        role: test
        commands:
          - argv: [touch, alpha-started]
  - name: beta
    description: A test role of one process per named item.
    project:
      all: [probe.lock]
    sandbox:
      programs: [touch]
      env:
        HOME: "{workspace}"
        PATH: /usr/local/bin:/usr/bin:/bin
    tools:
      - name: beta_test
        description: Touch each named item.
        role: test
        parameters:
          items: { kind: string-list, max-items: 8 }
        commands:
          - for-each: items
            argv: [touch, { item: true }]
"#;

#[test]
fn a_tool_whose_plan_starts_no_process_answers_as_0_15_1_did() {
    // 0.15.1's `invoke_toolchain` answered a group's outputs as an array whenever there was not
    // exactly one, so a `for-each` over an empty list answered `[]`. The changelog lists no change
    // for it, and names "toolchain provider produced no output" among the refusals whose words
    // are unchanged.
    let dir = tempfile::tempdir().expect("a workspace");
    let catalogue = local(dir.path(), resolve(dir.path(), ROLE_PAIR, &["beta"]));

    let answer = catalogue.invoke("beta_test", &json!({"items": []}));

    let answer = answer.unwrap_or_else(|refused| {
        panic!("0.15.1 answered `[]` for a plan of no process; this answers a refusal: {refused}")
    });
    assert!(
        answer == json!([]) || answer == json!({"runs": []}),
        "an empty list of runs: {answer}"
    );
}

#[test]
fn a_role_names_every_provider_it_planned_even_one_whose_plan_started_nothing() {
    // 0.15.1 inserted every provider of a role into `toolchains`, `[]` for one that started no
    // process. An answer without `beta` reads as a role `beta` was never part of.
    let dir = tempfile::tempdir().expect("a workspace");
    let catalogue = local(
        dir.path(),
        resolve(dir.path(), ROLE_PAIR, &["alpha", "beta"]),
    );

    let answer = catalogue
        .invoke(
            "toolchain_test",
            &json!({"alpha": {}, "beta": {"items": []}}),
        )
        .expect("alpha ran");

    assert!(
        dir.path().join("alpha-started").exists(),
        "alpha's process ran: {answer}"
    );
    assert!(
        answer["toolchains"].get("beta").is_some(),
        "the role planned `beta` and the answer does not name it: {answer}"
    );
}

/// Two providers with a `test` role: `slow` outlasts any short deadline, `fast` leaves a marker.
const SLOW_THEN_FAST: &str = r#"version: 1
toolchains:
  - name: fast
    description: A test role of one process that leaves a marker.
    project:
      all: [probe.lock]
    sandbox:
      programs: [touch]
      env:
        HOME: "{workspace}"
        PATH: /usr/local/bin:/usr/bin:/bin
    tools:
      - name: fast_test
        description: Leave a marker.
        role: test
        commands:
          - argv: [touch, fast-started]
  - name: aslow
    description: A test role of one process longer than the time left.
    project:
      all: [probe.lock]
    sandbox:
      programs: [sleep]
      env:
        HOME: "{workspace}"
        PATH: /usr/local/bin:/usr/bin:/bin
    tools:
      - name: aslow_test
        description: Sleep.
        role: test
        commands:
          - argv: [sleep, "5"]
"#;

#[test]
fn a_role_cut_short_by_the_deadline_names_the_provider_it_never_started() {
    // Providers run in name order, so `aslow` runs first and uses the whole deadline; `fast` is
    // then not started, which is the shared deadline working. What the model is answered must
    // still say that `fast` did not run: 0.15.1 answered an entry for every provider, and an
    // answer that leaves one out reads like a role that had one provider (invariant 8).
    let dir = tempfile::tempdir().expect("a workspace");
    let catalogue = local(
        dir.path(),
        resolve(dir.path(), SLOW_THEN_FAST, &["aslow", "fast"]),
    );

    let answer = catalogue
        .invoke_within(
            "toolchain_test",
            &json!({"aslow": {}, "fast": {}}),
            Some(Duration::from_millis(300)),
        )
        .expect("aslow started, so the call answers");

    assert!(
        !dir.path().join("fast-started").exists(),
        "fast started after the deadline: {answer}"
    );
    assert_eq!(
        answer["toolchains"]["aslow"]["timed_out"],
        json!(true),
        "{answer}"
    );
    assert!(
        answer["toolchains"].get("fast").is_some(),
        "the role planned `fast`, it never started, and the answer does not name it: {answer}"
    );
}

#[test]
fn a_call_given_no_time_left_starts_nothing_and_says_so_in_the_changelogs_words() {
    let dir = tempfile::tempdir().expect("a workspace");
    let catalogue = local(dir.path(), resolve(dir.path(), ROLE_PAIR, &["alpha"]));

    let refused = catalogue
        .invoke_within("alpha_test", &json!({}), Some(Duration::ZERO))
        .expect_err("nothing is left, so nothing starts");

    assert_eq!(
        refused.message(),
        "the call's deadline passed before `alpha_test` started a process; nothing was started"
    );
    assert!(!dir.path().join("alpha-started").exists());
}

/// A provider whose one tool writes the path it names, `protected/stamp` when it names none.
const DEFAULTED_WRITE: &str = r#"version: 1
toolchains:
  - name: stamp
    description: Touches one written path.
    project:
      all: [probe.lock]
    sandbox:
      programs: [touch]
      env:
        HOME: "{workspace}"
        PATH: /usr/local/bin:/usr/bin:/bin
    tools:
      - name: stamp_write
        description: Touch the target.
        workspace-writes: true
        parameters:
          target: { kind: workspace-path, default: protected/stamp }
        writes: [target]
        commands:
          - argv: [touch, { arg: target }]
"#;

#[test]
#[ignore = "pre-existing in tool-toolchain's plan.writes; fix requested upstream"]
fn a_written_path_taken_from_its_default_goes_through_the_write_scope() {
    // `writes: [target]` declares the path written. Named, it is judged; left to its default, the
    // same path reaches `touch` and the scope is never asked.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir(dir.path().join("protected")).expect("the refused directory");
    let catalogue = local(dir.path(), resolve(dir.path(), DEFAULTED_WRITE, &["stamp"]))
        .scoped(denied("protected/**"));

    let named = catalogue.invoke("stamp_write", &json!({"target": "protected/stamp"}));
    assert!(
        named.is_err(),
        "the scope refuses the path when it is named: {named:?}"
    );
    assert!(!dir.path().join("protected/stamp").exists());

    let defaulted = catalogue.invoke("stamp_write", &json!({}));

    assert!(
        !dir.path().join("protected/stamp").exists(),
        "the default wrote `protected/stamp` under `protected/**=denied`: {defaulted:?}"
    );
    assert!(defaulted.is_err(), "{defaulted:?}");
}

/// Records every argv it is asked to start and starts none, in a tree on this host.
struct Recorder {
    tree: LocalOperations,
    programs: Vec<String>,
    started: Arc<Mutex<Vec<Vec<String>>>>,
}

impl Operations for Recorder {
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
        self.started
            .lock()
            .expect("not poisoned")
            .push(argv.to_vec());
        Ok(json!({"stdout": "", "exit": 0}))
    }
    fn host_root(&self) -> Option<&Path> {
        self.tree.host_root()
    }
    fn programs(&self) -> &[String] {
        &self.programs
    }
}

/// Shaped like the built-in `go_fmt`: `gofmt -w` over every written path.
const GO_FMT_SHAPED: &str = r#"version: 1
toolchains:
  - name: fmtlike
    description: Rewrites every named path in place.
    project:
      all: [probe.lock]
    sandbox:
      programs: [gofmt]
      env:
        HOME: "{workspace}"
        PATH: /usr/local/bin:/usr/bin:/bin
    tools:
      - name: fmtlike_fmt
        description: Format only the explicitly named source files.
        workspace-writes: true
        parameters:
          paths: { kind: workspace-path-list, required: true, min-items: 1, max-items: 128 }
        writes: [paths]
        commands:
          - argv: [gofmt, -w, { each: paths }]
"#;

#[test]
fn a_written_path_naming_a_directory_that_holds_refused_files_starts_nothing() {
    // `gofmt -w <dir>` rewrites every Go file beneath it. The scope refuses each file under
    // `protected/**` by name, and admits the directories that hold them.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir(dir.path().join("protected")).expect("the refused directory");
    std::fs::write(dir.path().join("protected/kept.go"), "package protected\n").expect("a source");
    let operations = Recorder {
        tree: LocalOperations::new(dir.path()).expect("a workspace"),
        programs: vec!["gofmt".to_owned()],
        started: Arc::new(Mutex::new(Vec::new())),
    };
    let catalogue =
        Catalogue::of_with_toolchains(operations, resolve(dir.path(), GO_FMT_SHAPED, &["fmtlike"]))
            .scoped(denied("protected/**"));

    let named = catalogue.invoke("fmtlike_fmt", &json!({"paths": ["protected/kept.go"]}));
    assert!(named.is_err(), "the file itself is refused: {named:?}");

    let mut admitted = Vec::new();
    for spelling in ["protected", ".", "protected/."] {
        if let Ok(answer) = catalogue.invoke("fmtlike_fmt", &json!({"paths": [spelling]})) {
            admitted.push(format!("`{spelling}` -> {answer}"));
        }
    }
    assert!(
        admitted.is_empty(),
        "a directory holding `protected/kept.go` was handed to `gofmt -w`: {admitted:?}"
    );
}

#[test]
fn a_written_link_that_resolves_to_a_directory_holding_refused_files_starts_nothing() {
    // The link's own spelling holds nothing a rule names; where it lands is `protected`, and that
    // directory is judged by what it holds. A directory nothing beneath is refused still runs.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir(dir.path().join("protected")).expect("the refused directory");
    std::fs::write(dir.path().join("protected/kept.go"), "package protected\n").expect("a source");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::fs::write(dir.path().join("src/ok.go"), "package src\n").expect("a source");
    std::fs::create_dir(dir.path().join("links")).expect("links");
    std::os::unix::fs::symlink("../protected", dir.path().join("links/protected"))
        .expect("a link to the refused directory");
    let started = Arc::new(Mutex::new(Vec::new()));
    let operations = Recorder {
        tree: LocalOperations::new(dir.path()).expect("a workspace"),
        programs: vec!["gofmt".to_owned()],
        started: Arc::clone(&started),
    };
    let catalogue =
        Catalogue::of_with_toolchains(operations, resolve(dir.path(), GO_FMT_SHAPED, &["fmtlike"]))
            .scoped(denied("protected/**"));

    let refused = catalogue
        .invoke("fmtlike_fmt", &json!({"paths": ["links/protected"]}))
        .expect_err("the link lands on the refused directory");
    assert!(
        refused.message().contains("leads to `protected`"),
        "{refused}"
    );
    assert!(
        refused.message().contains("`protected/**=denied`"),
        "{refused}"
    );
    assert!(
        started.lock().expect("not poisoned").is_empty(),
        "an argv reached the provider"
    );

    catalogue
        .invoke("fmtlike_fmt", &json!({"paths": ["src"]}))
        .expect("a directory nothing beneath is refused runs");
    assert_eq!(
        *started.lock().expect("not poisoned"),
        vec![vec!["gofmt".to_owned(), "-w".to_owned(), "src".to_owned()]]
    );
}

/// The `gofmt -w`-shaped catalogue over a [`Recorder`] in `root`, and every argv it was asked to
/// start.
fn recording(root: &Path) -> (Catalogue, Arc<Mutex<Vec<Vec<String>>>>) {
    let started = Arc::new(Mutex::new(Vec::new()));
    let operations = Recorder {
        tree: LocalOperations::new(root).expect("a workspace"),
        programs: vec!["gofmt".to_owned()],
        started: Arc::clone(&started),
    };
    let catalogue =
        Catalogue::of_with_toolchains(operations, resolve(root, GO_FMT_SHAPED, &["fmtlike"]));
    (catalogue, started)
}

fn formatting(catalogue: &Catalogue, path: &str) -> Result<Value, Refused> {
    catalogue.invoke("fmtlike_fmt", &json!({"paths": [path]}))
}

/// What every refusal of a written directory tells the caller to do instead.
const INSTEAD: &str = "Name the subdirectories or files to format or write instead";

#[test]
fn a_written_directory_holding_a_link_to_a_refused_file_starts_nothing() {
    // `gofmt -w src` follows `src/alias.go` and rewrites `protected/kept.go`, which the scope
    // refuses by name; `go_fmt_on_a_directory_rewrites_no_refused_file_through_a_link_beneath_it`
    // runs the real `gofmt`. The name `src` holds nothing a rule names.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir(dir.path().join("protected")).expect("the refused directory");
    std::fs::write(dir.path().join("protected/kept.go"), "package protected\n").expect("a source");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::fs::write(dir.path().join("src/ok.go"), "package src\n").expect("a source");
    std::os::unix::fs::symlink("../protected/kept.go", dir.path().join("src/alias.go"))
        .expect("a link to a refused file");
    let (catalogue, started) = recording(dir.path());
    let catalogue = catalogue.scoped(denied("protected/**"));

    let refused = formatting(&catalogue, "src").expect_err("the link reaches a refused file");
    assert_eq!(
        refused.message(),
        format!(
            "`src` is a directory, and the link `src/alias.go` beneath it leads to \
             `protected/kept.go`, where `protected/**=denied` refuses this write; a write through \
             the directory reaches that file through the link. {INSTEAD}, none of which holds \
             that link."
        )
    );
    assert!(
        started.lock().expect("not poisoned").is_empty(),
        "an argv reached the provider"
    );

    formatting(&catalogue, "src/ok.go").expect("a file beside the link runs");
    assert_eq!(
        *started.lock().expect("not poisoned"),
        vec![vec![
            "gofmt".to_owned(),
            "-w".to_owned(),
            "src/ok.go".to_owned()
        ]]
    );
}

#[test]
fn a_link_beneath_a_written_directory_to_another_directory_is_judged_as_the_rest_of_the_walk() {
    // A program follows a link to a directory, so the walk does too: what it leads to is judged
    // by name as a written directory is, and walked for its own links. A cycle ends where it
    // returns to a directory already walked.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir(dir.path().join("protected")).expect("the refused directory");
    std::fs::write(dir.path().join("protected/kept.go"), "package protected\n").expect("a source");
    std::fs::create_dir(dir.path().join("shared")).expect("shared");
    std::fs::write(dir.path().join("shared/x.go"), "package shared\n").expect("a source");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::os::unix::fs::symlink("../shared", dir.path().join("src/shared"))
        .expect("a link to an admitted directory");
    std::os::unix::fs::symlink("../src", dir.path().join("shared/back"))
        .expect("a link back, closing a cycle");
    std::fs::create_dir(dir.path().join("lib")).expect("lib");
    std::os::unix::fs::symlink("../protected/kept.go", dir.path().join("lib/alias.go"))
        .expect("a link to a refused file");
    std::fs::create_dir(dir.path().join("app")).expect("app");
    std::os::unix::fs::symlink("../lib", dir.path().join("app/lib"))
        .expect("a link to a directory holding a link to a refused file");
    std::fs::create_dir(dir.path().join("web")).expect("web");
    std::os::unix::fs::symlink("../protected", dir.path().join("web/protected"))
        .expect("a link to the refused directory");
    let (catalogue, started) = recording(dir.path());
    let catalogue = catalogue.scoped(denied("protected/**"));

    formatting(&catalogue, "src").expect("`shared` holds nothing refused, and the cycle ends");
    assert_eq!(started.lock().expect("not poisoned").len(), 1);

    let refused = formatting(&catalogue, "app").expect_err("`lib` holds a link to a refused file");
    assert_eq!(
        refused.message(),
        format!(
            "`app` is a directory, and the link `app/lib/alias.go` beneath it leads to \
             `protected/kept.go`, where `protected/**=denied` refuses this write; a write through \
             the directory reaches that file through the link. {INSTEAD}, none of which holds \
             that link."
        )
    );
    let refused = formatting(&catalogue, "web").expect_err("the link leads to a refused directory");
    assert_eq!(
        refused.message(),
        format!(
            "`web` is a directory, and the link `web/protected` beneath it leads to the directory \
             `protected`, where `protected/**=denied` refuses this write; a write through the \
             directory reaches what that directory holds through the link. {INSTEAD}, none of \
             which holds that link."
        )
    );
    assert_eq!(
        started.lock().expect("not poisoned").len(),
        1,
        "an argv reached the provider"
    );
}

#[test]
fn a_link_beneath_a_written_directory_that_stays_inside_it_is_admitted() {
    // Both links land inside `src`, which the walk covers, one of them on `src` itself.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir_all(dir.path().join("src/inner")).expect("src/inner");
    std::fs::write(dir.path().join("src/inner/a.go"), "package inner\n").expect("a source");
    std::os::unix::fs::symlink("inner/a.go", dir.path().join("src/alias.go"))
        .expect("a link within");
    std::os::unix::fs::symlink("..", dir.path().join("src/inner/up")).expect("a link up to src");
    let (catalogue, started) = recording(dir.path());
    let catalogue = catalogue.scoped(denied("protected/**"));

    formatting(&catalogue, "src").expect("every link beneath stays inside");
    assert_eq!(
        *started.lock().expect("not poisoned"),
        vec![vec!["gofmt".to_owned(), "-w".to_owned(), "src".to_owned()]]
    );
}

#[test]
fn a_link_beneath_a_written_directory_that_leads_nowhere_is_judged_where_its_target_would_be() {
    // A program that writes through a link that leads nowhere creates its target.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir(dir.path().join("protected")).expect("the refused directory");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::os::unix::fs::symlink("../protected/new.go", dir.path().join("src/new.go"))
        .expect("a link to a refused path not yet written");
    std::fs::create_dir(dir.path().join("other")).expect("other");
    std::os::unix::fs::symlink("../elsewhere/new.go", dir.path().join("other/new.go"))
        .expect("a link to an admitted path not yet written");
    let (catalogue, started) = recording(dir.path());
    let catalogue = catalogue.scoped(denied("protected/**"));

    let refused = formatting(&catalogue, "src").expect_err("its target is refused");
    assert_eq!(
        refused.message(),
        format!(
            "`src` is a directory, and the link `src/new.go` beneath it leads to \
             `protected/new.go`, where `protected/**=denied` refuses this write; a write through \
             the directory reaches that file through the link. {INSTEAD}, none of which holds \
             that link."
        )
    );
    assert!(started.lock().expect("not poisoned").is_empty());

    formatting(&catalogue, "other").expect("its target is admitted");
    assert_eq!(started.lock().expect("not poisoned").len(), 1);
}

#[test]
fn a_written_directory_is_walked_only_when_a_rule_refuses_something() {
    // `src` is one directory deeper than the walk's bound, 32 directories, so a walk of it is
    // refused by the bound; a scope that refuses nothing never walks it.
    let dir = tempfile::tempdir().expect("a workspace");
    let deep: Vec<String> = (1..=33).map(|level| format!("d{level}")).collect();
    std::fs::create_dir_all(dir.path().join("src").join(deep.join("/"))).expect("a deep tree");

    let (catalogue, started) = recording(dir.path());
    let admitting = catalogue.scoped(Scope::of(vec![
        ScopeRule::parse("**=allowed").expect("a rule"),
    ]));
    formatting(&admitting, "src").expect("nothing is refused, so nothing is walked");
    assert_eq!(started.lock().expect("not poisoned").len(), 1);

    let (catalogue, started) = recording(dir.path());
    let refusing = catalogue.scoped(denied("protected/**"));
    let refused = formatting(&refusing, "src").expect_err("the walk meets its bound");
    assert_eq!(
        refused.message(),
        format!(
            "`src` is a directory nested more than 32 directories deep, deeper than a write \
             through a directory is judged while `protected/**=denied` refuses writes, so it is \
             refused rather than judged in part. {INSTEAD}."
        )
    );
    assert!(started.lock().expect("not poisoned").is_empty());

    // The step's layer is asked as well, and a directory within the bound is walked and runs.
    let (mut catalogue, started) = recording(dir.path());
    catalogue.narrow(denied("protected/**"));
    let refused = formatting(&catalogue, "src").expect_err("the step refuses something");
    assert!(
        refused.message().contains("more than 32 directories deep"),
        "{refused}"
    );
    formatting(&catalogue, "src/d1/d2").expect("31 levels deep is within the bound");
    assert_eq!(started.lock().expect("not poisoned").len(), 1);
}

const UNFORMATTED_GO: &str = "package protected\n\nfunc   F( ) {}\n";

#[test]
fn go_fmt_on_a_directory_rewrites_no_refused_file_through_a_link_beneath_it() {
    // The measurement this case repeats: `gofmt -w src`, with `src/alias.go` a link to
    // `protected/kept.go`, rewrote `protected/kept.go`. Needs `go` on `PATH`.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::write(dir.path().join("go.mod"), "module probe\n\ngo 1.21\n").expect("a module");
    std::fs::create_dir(dir.path().join("protected")).expect("the refused directory");
    std::fs::write(dir.path().join("protected/kept.go"), UNFORMATTED_GO).expect("a source");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::os::unix::fs::symlink("../protected/kept.go", dir.path().join("src/alias.go"))
        .expect("a link to a refused file");
    let providers = tool_toolchain::Registry::builtins()
        .expect("the built-ins")
        .resolve(dir.path(), Some(&["go".to_owned()]))
        .expect("the built-in Go provider resolves where `go` is on `PATH`");
    let catalogue = local(dir.path(), providers).scoped(denied("protected/**"));

    let answer = catalogue.invoke("go_fmt", &json!({"paths": ["src"]}));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("protected/kept.go")).expect("read"),
        UNFORMATTED_GO,
        "`go_fmt` rewrote a file under `protected/**=denied` through a link: {answer:?}"
    );
    assert!(answer.is_err(), "{answer:?}");
}

#[test]
fn go_fmt_on_a_directory_rewrites_no_file_the_scope_refuses() {
    // The built-in `go_fmt`, "Format only the explicitly named Go source files". Needs `go` on
    // `PATH`, which the built-in provider resolves its root from.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::write(dir.path().join("go.mod"), "module probe\n\ngo 1.21\n").expect("a module");
    std::fs::create_dir(dir.path().join("protected")).expect("the refused directory");
    std::fs::write(dir.path().join("protected/kept.go"), UNFORMATTED_GO).expect("a source");
    let providers = tool_toolchain::Registry::builtins()
        .expect("the built-ins")
        .resolve(dir.path(), Some(&["go".to_owned()]))
        .expect("the built-in Go provider resolves where `go` is on `PATH`");
    let catalogue = local(dir.path(), providers).scoped(denied("protected/**"));

    let answer = catalogue.invoke("go_fmt", &json!({"paths": ["."]}));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("protected/kept.go")).expect("read"),
        UNFORMATTED_GO,
        "`go_fmt` rewrote a file under `protected/**=denied`: {answer:?}"
    );
}

/// Two providers with a `test` role: `alpha` leaves a marker, `beta` writes each named path.
const LATER_WRITE: &str = r#"version: 1
toolchains:
  - name: alpha
    description: A test role of one process that leaves a marker.
    project:
      all: [probe.lock]
    sandbox:
      programs: [touch]
      env:
        HOME: "{workspace}"
        PATH: /usr/local/bin:/usr/bin:/bin
    tools:
      - name: alpha_test
        description: Leave a marker.
        role: test
        commands:
          - argv: [touch, alpha-started]
  - name: beta
    description: A test role that writes the named paths.
    project:
      all: [probe.lock]
    sandbox:
      programs: [touch]
      env:
        HOME: "{workspace}"
        PATH: /usr/local/bin:/usr/bin:/bin
    tools:
      - name: beta_test
        description: Touch each named path.
        role: test
        workspace-writes: true
        parameters:
          paths: { kind: workspace-path-list, required: true, min-items: 1, max-items: 8 }
        writes: [paths]
        commands:
          - argv: [touch, { each: paths }]
"#;

#[test]
fn a_role_whose_later_provider_writes_a_refused_path_starts_nothing() {
    // The changelog: "every written path of every provider is judged before the first process of
    // the call starts, where 0.15.1 judged each provider's as it reached it".
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir(dir.path().join("protected")).expect("the refused directory");
    let catalogue = local(
        dir.path(),
        resolve(dir.path(), LATER_WRITE, &["alpha", "beta"]),
    )
    .scoped(denied("protected/**"));

    let refused = catalogue
        .invoke(
            "toolchain_test",
            &json!({"alpha": {}, "beta": {"paths": ["protected/x"]}}),
        )
        .expect_err("beta's written path is refused");

    assert!(refused.message().contains("protected/x"), "{refused}");
    assert!(
        !dir.path().join("alpha-started").exists(),
        "alpha ran before beta's path was refused"
    );
    assert!(!dir.path().join("protected/x").exists());
}

#[test]
fn run_cannot_name_a_program_a_toolchain_provider_declares() {
    // The refusal `invoke_shell` makes with `tool_toolchain::executor::programs`, which replaced
    // `crate::toolchain::programs`. `run` is published here, for `sleep`, so the call reaches it.
    let dir = tempfile::tempdir().expect("a workspace");
    let providers = resolve(dir.path(), ROLE_PAIR, &["alpha"]);
    let operations =
        LocalOperations::unconfined(dir.path(), vec!["touch".to_owned(), "sleep".to_owned()])
            .expect("a workspace");
    let catalogue = Catalogue::of_with_toolchains(operations, providers);
    assert!(
        catalogue.get("run").is_some(),
        "`run` is published for `sleep`"
    );

    let refused = catalogue
        .invoke("run", &json!({"argv": ["touch", "by-run"]}))
        .expect_err("`touch` is a toolchain program");

    assert_eq!(
        refused.message(),
        "`touch` is admitted only through its typed toolchain tools; `run` cannot name it"
    );
    assert!(!dir.path().join("by-run").exists());
    catalogue
        .invoke("run", &json!({"argv": ["sleep", "0"]}))
        .expect("a program only `run` declares runs");
}

#[test]
fn a_step_scope_alone_refuses_a_toolchain_write_as_spelled_and_where_it_lands() {
    // `invoke_toolchain` asks the run's layer and then the step's, in a closure of its own where
    // 0.15.1 called `Catalogue::refusal`. The run declares nothing here; the step refuses
    // `protected/**`.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir(dir.path().join("protected")).expect("the refused directory");
    std::fs::write(dir.path().join("protected/kept.go"), "package protected\n").expect("a source");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::fs::write(dir.path().join("src/ok.go"), "package src\n").expect("a source");
    std::os::unix::fs::symlink("../protected/kept.go", dir.path().join("src/alias.go"))
        .expect("a link into the refused directory");
    let started = Arc::new(Mutex::new(Vec::new()));
    let operations = Recorder {
        tree: LocalOperations::new(dir.path()).expect("a workspace"),
        programs: vec!["gofmt".to_owned()],
        started: Arc::clone(&started),
    };
    let mut catalogue =
        Catalogue::of_with_toolchains(operations, resolve(dir.path(), GO_FMT_SHAPED, &["fmtlike"]));
    catalogue.narrow(denied("protected/**"));

    let spelled = catalogue
        .invoke("fmtlike_fmt", &json!({"paths": ["protected/kept.go"]}))
        .expect_err("the step refuses the path as spelled");
    assert!(spelled.message().contains("protected/kept.go"), "{spelled}");
    let landed = catalogue
        .invoke("fmtlike_fmt", &json!({"paths": ["src/alias.go"]}))
        .expect_err("the step refuses where the path lands");
    assert!(
        landed.message().contains("leads to `protected/kept.go`"),
        "{landed}"
    );
    assert!(
        started.lock().expect("not poisoned").is_empty(),
        "an argv reached the provider"
    );

    catalogue
        .invoke("fmtlike_fmt", &json!({"paths": ["src/ok.go"]}))
        .expect("a path the step admits runs");
    assert_eq!(
        *started.lock().expect("not poisoned"),
        vec![vec![
            "gofmt".to_owned(),
            "-w".to_owned(),
            "src/ok.go".to_owned()
        ]]
    );
}
