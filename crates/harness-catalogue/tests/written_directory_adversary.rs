//! Adversary cases for story:a-written-directory-is-judged-by-what-it-holds, driven through the
//! public catalogue against the story's own acceptance statement and outcome.
//!
//! Most cases record every argv a provider is asked to start and start none. Four run the
//! built-in Go or Rust provider and need `go`/`gofmt`, or a rustup `stable` toolchain, on this
//! host.

#![cfg(unix)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use harness_catalogue::{
    Catalogue, LocalOperations, Operations, ReadWindow, Refused, ResolvedProvider, Scope,
    ScopeRule, SearchOptions,
};
use serde_json::{Value, json};

fn rules(declarations: &[&str]) -> Scope {
    Scope::of(
        declarations
            .iter()
            .map(|declaration| ScopeRule::parse(declaration).expect("a rule"))
            .collect(),
    )
}

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

/// Shaped like the built-in `rust_fmt`: `rustfmt <path>` once per written path.
const RUST_FMT_SHAPED: &str = r#"version: 1
toolchains:
  - name: rustfmtlike
    description: Rewrites every named Rust file in place.
    project:
      all: [probe.lock]
    sandbox:
      programs: [rustfmt]
      env:
        HOME: "{workspace}"
        PATH: /usr/local/bin:/usr/bin:/bin
    tools:
      - name: rustfmtlike_fmt
        description: Format only the explicitly named Rust source files.
        workspace-writes: true
        parameters:
          paths: { kind: workspace-path-list, required: true, min-items: 1, max-items: 128 }
        writes: [paths]
        commands:
          - for-each: paths
            argv: [rustfmt, { item: true }]
"#;

/// The `gofmt -w`-shaped catalogue over a [`Recorder`] in `root`, and every argv it was asked to
/// start.
fn recording(root: &Path) -> (Catalogue, Arc<Mutex<Vec<Vec<String>>>>) {
    recording_with(root, GO_FMT_SHAPED, "fmtlike", "gofmt")
}

/// The catalogue `spec` declares as `provider`, running `program`, over a [`Recorder`] in
/// `root`, and every argv it was asked to start.
fn recording_with(
    root: &Path,
    spec: &str,
    provider: &str,
    program: &str,
) -> (Catalogue, Arc<Mutex<Vec<Vec<String>>>>) {
    let started = Arc::new(Mutex::new(Vec::new()));
    let operations = Recorder {
        tree: LocalOperations::new(root).expect("a workspace"),
        programs: vec![program.to_owned()],
        started: Arc::clone(&started),
    };
    let catalogue = Catalogue::of_with_toolchains(operations, resolve(root, spec, &[provider]));
    (catalogue, started)
}

/// Writes each file of `files`, a path and its text, under `root`.
fn holding(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().expect("a parent")).expect("its directory");
        std::fs::write(at, text).expect("a file");
    }
}

fn formatting(catalogue: &Catalogue, path: &str) -> Result<Value, Refused> {
    catalogue.invoke("fmtlike_fmt", &json!({"paths": [path]}))
}

const UNFORMATTED_GO: &str = "package protected\n\nfunc   F( ) {}\n";

// --- acceptance: a directory none of whose possible contents a rule denies --------------------

#[test]
fn under_the_default_profile_a_directory_no_rule_reaches_into_is_admitted_as_before() {
    // The story's acceptance: "A written directory none of whose possible contents a rule denies
    // is admitted as before". The default profile (`harness-cli`'s `default_write_scope`) denies
    // `.git/**` and allows the rest, so nothing beneath `web` or `vendor` is denied, through the
    // link or otherwise, and both were admitted before this unit.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir(dir.path().join("shared")).expect("shared");
    std::fs::write(dir.path().join("shared/x.go"), "package shared\n").expect("a source");
    std::fs::create_dir(dir.path().join("web")).expect("web");
    std::os::unix::fs::symlink("../shared", dir.path().join("web/shared"))
        .expect("a link to an admitted directory");
    std::fs::create_dir(dir.path().join("vendor")).expect("vendor");
    for file in 0..=10_000 {
        std::fs::write(dir.path().join(format!("vendor/{file}.go")), "").expect("a file");
    }
    let (catalogue, started) = recording(dir.path());
    let catalogue = catalogue.scoped(rules(&[".git/**=denied", "**=allowed"]));

    let refused: Vec<String> = ["web", "vendor"]
        .into_iter()
        .filter_map(|path| {
            formatting(&catalogue, path)
                .err()
                .map(|refused| format!("`{path}`: {refused}"))
        })
        .collect();

    assert!(
        refused.is_empty(),
        "directories none of whose possible contents a rule denies were refused under the \
         default profile: {refused:#?}"
    );
    assert_eq!(started.lock().expect("not poisoned").len(), 2);
}

// --- the class: a write through a directory reaches a file the scope denies -------------------

#[test]
#[ignore = "a stated remaining gap: hard links (no harness tool or git checkout creates one)"]
fn go_fmt_on_a_directory_rewrites_no_refused_file_through_a_hard_link_beneath_it() {
    // A hard link is a second name for the refused file's bytes, and `gofmt -w` rewrites a file in
    // place (measured: `gofmt -w src` with `src/k.go` hard-linked to `protected/kept.go` rewrote
    // `protected/kept.go`). The walk judges symbolic links only. Needs `go` on `PATH`.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::write(dir.path().join("go.mod"), "module probe\n\ngo 1.21\n").expect("a module");
    std::fs::create_dir(dir.path().join("protected")).expect("the refused directory");
    std::fs::write(dir.path().join("protected/kept.go"), UNFORMATTED_GO).expect("a source");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::fs::hard_link(
        dir.path().join("protected/kept.go"),
        dir.path().join("src/k.go"),
    )
    .expect("a hard link to a refused file");
    let providers = tool_toolchain::Registry::builtins()
        .expect("the built-ins")
        .resolve(dir.path(), Some(&["go".to_owned()]))
        .expect("the built-in Go provider resolves where `go` is on `PATH`");
    let catalogue = local(dir.path(), providers).scoped(rules(&["protected/**=denied"]));

    let answer = catalogue.invoke("go_fmt", &json!({"paths": ["src"]}));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("protected/kept.go")).expect("read"),
        UNFORMATTED_GO,
        "`go_fmt` on `src` rewrote `protected/kept.go` under `protected/**=denied` through a \
         hard link: {answer:?}"
    );
}

// --- outside the class: what the executor refuses without any scope ---------------------------

#[test]
#[ignore = "unconfined library embedders with no refusing rule; the CLI runs toolchains confined (stated gap)"]
fn go_fmt_on_a_directory_writes_nothing_outside_the_workspace_through_a_link_beneath_it() {
    // The executor refuses a written path that lands outside the workspace "with no scope at
    // all" (tool-toolchain's F1, D6), and does for the link named as a file. Named through the
    // directory that holds it, the walk is not made, because no layer refuses anything, and
    // `gofmt -w src` follows the link. Needs `go` on `PATH`.
    let outside = tempfile::tempdir().expect("outside the workspace");
    std::fs::write(outside.path().join("x.go"), UNFORMATTED_GO).expect("a file outside");
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::write(dir.path().join("go.mod"), "module probe\n\ngo 1.21\n").expect("a module");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::os::unix::fs::symlink(outside.path().join("x.go"), dir.path().join("src/x.go"))
        .expect("a link out of the workspace");
    let providers = tool_toolchain::Registry::builtins()
        .expect("the built-ins")
        .resolve(dir.path(), Some(&["go".to_owned()]))
        .expect("the built-in Go provider resolves where `go` is on `PATH`");
    let catalogue = local(dir.path(), providers);

    let named = catalogue.invoke("go_fmt", &json!({"paths": ["src/x.go"]}));
    assert!(
        named.is_err(),
        "the link named as a file is refused: {named:?}"
    );

    let answer = catalogue.invoke("go_fmt", &json!({"paths": ["src"]}));

    assert_eq!(
        std::fs::read_to_string(outside.path().join("x.go")).expect("read"),
        UNFORMATTED_GO,
        "`go_fmt` on `src` rewrote a file outside the workspace through a link: {answer:?}"
    );
}

// --- outside the class: a written file whose program reaches other files ----------------------

#[test]
fn rust_fmt_on_a_named_file_rewrites_no_module_file_the_scope_refuses() {
    // The built-in `rust_fmt`, "Format only the explicitly named Rust source files", runs
    // `rustfmt <path>`, and rustfmt formats every out-of-line module the named file declares.
    // Needs a rustup `stable` toolchain under `$RUSTUP_HOME` or `~/.rustup`.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("a manifest");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::fs::write(dir.path().join("src/lib.rs"), "mod generated;\n").expect("the crate root");
    let unformatted = "pub   fn   generated( ) {}\n";
    std::fs::write(dir.path().join("src/generated.rs"), unformatted).expect("a refused module");
    let providers = tool_toolchain::Registry::builtins()
        .expect("the built-ins")
        .resolve(dir.path(), Some(&["rust".to_owned()]))
        .expect("the built-in Rust provider resolves where rustup `stable` is installed");
    let catalogue = local(dir.path(), providers).scoped(rules(&["src/generated.rs=denied"]));

    let named = catalogue.invoke("rust_fmt", &json!({"paths": ["src/generated.rs"]}));
    assert!(named.is_err(), "the module itself is refused: {named:?}");

    let answer = catalogue.invoke("rust_fmt", &json!({"paths": ["src/lib.rs"]}));

    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/generated.rs")).expect("read"),
        unformatted,
        "`rust_fmt` on `src/lib.rs` rewrote `src/generated.rs` under `src/generated.rs=denied`: \
         {answer:?}"
    );
}

#[test]
fn rust_fmt_on_a_file_the_model_wrote_rewrites_no_module_its_path_attribute_names() {
    // The adversary's variant: the model writes a file the scope admits, declaring a module in
    // a file the scope refuses through `#[path]`, and then formats it. Needs a rustup `stable`
    // toolchain under `$RUSTUP_HOME` or `~/.rustup`.
    let dir = tempfile::tempdir().expect("a workspace");
    let unformatted = "pub   fn   kept( ) {}\n";
    holding(
        dir.path(),
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("src/lib.rs", ""),
            ("protected/x.rs", unformatted),
        ],
    );
    let providers = tool_toolchain::Registry::builtins()
        .expect("the built-ins")
        .resolve(dir.path(), Some(&["rust".to_owned()]))
        .expect("the built-in Rust provider resolves where rustup `stable` is installed");
    let catalogue = local(dir.path(), providers).scoped(rules(&["protected/**=denied"]));
    catalogue
        .invoke(
            "file_write",
            &json!({"path": "src/evil.rs", "text": "#[path = \"../protected/x.rs\"]\nmod k;\n"}),
        )
        .expect("`src/evil.rs` is admitted");

    let refused = catalogue
        .invoke("rust_fmt", &json!({"paths": ["src/evil.rs"]}))
        .expect_err("the module it declares is refused");

    assert_eq!(
        std::fs::read_to_string(dir.path().join("protected/x.rs")).expect("read"),
        unformatted,
        "`rust_fmt` on `src/evil.rs` rewrote `protected/x.rs` under `protected/**=denied`"
    );
    assert_eq!(
        refused.message(),
        "`src/evil.rs:2` declares the module `k` in `src/../protected/x.rs`, which `rustfmt \
         src/evil.rs` rewrites as well, and `src/../protected/x.rs` is outside what this run may \
         change. Nothing here writes to it; if the work needs that path, the run was scoped \
         wrongly and a person should say so. Name the files to format, none of which declares a \
         refused module."
    );
}

#[test]
fn a_formatting_call_on_a_crate_none_of_whose_modules_a_rule_refuses_starts_as_before() {
    // Under the default profile and under a narrower one: every module is read and judged, none
    // is refused, and the argv started is the one the plan names, byte for byte.
    let dir = tempfile::tempdir().expect("a workspace");
    holding(
        dir.path(),
        &[
            (
                "src/lib.rs",
                "mod a;\nmod i {\n    mod j;\n}\n#[path = \"other/k.rs\"]\nmod k;\n",
            ),
            ("src/a.rs", "mod b;\n"),
            ("src/a/b.rs", ""),
            ("src/i/j.rs", ""),
            ("src/other/k.rs", ""),
        ],
    );
    let (catalogue, started) =
        recording_with(dir.path(), RUST_FMT_SHAPED, "rustfmtlike", "rustfmt");
    let mut catalogue = catalogue.scoped(rules(&[".git/**=denied", "**=allowed"]));
    let call = json!({"paths": ["src/lib.rs"]});

    catalogue
        .invoke("rustfmtlike_fmt", &call)
        .expect("no module is refused under the default profile");
    catalogue.narrow(rules(&["protected/**=denied"]));
    catalogue
        .invoke("rustfmtlike_fmt", &call)
        .expect("nor under a step that refuses another directory");

    assert_eq!(
        *started.lock().expect("not poisoned"),
        vec![vec!["rustfmt".to_owned(), "src/lib.rs".to_owned()]; 2]
    );
    catalogue.narrow(rules(&["src/a/b.rs=denied"]));
    let refused = catalogue
        .invoke("rustfmtlike_fmt", &call)
        .expect_err("a module two files down is refused");
    assert!(
        refused.message().starts_with(
            "`src/a.rs:1` declares the module `b` in `src/a/b.rs`, which `rustfmt src/lib.rs` \
             rewrites as well, and "
        ),
        "{refused}"
    );
    assert_eq!(
        started.lock().expect("not poisoned").len(),
        2,
        "nothing started"
    );
}

#[test]
fn with_no_refusing_rule_a_formatting_call_reads_no_module() {
    // `#[cfg_attr(…, path = …)]` is refused wherever the modules are read, so an admission is a
    // call that read none.
    let dir = tempfile::tempdir().expect("a workspace");
    holding(
        dir.path(),
        &[
            ("src/lib.rs", "#[cfg_attr(unix, path = \"u.rs\")]\nmod k;\n"),
            ("src/u.rs", ""),
            ("src/k.rs", ""),
        ],
    );
    let (catalogue, started) =
        recording_with(dir.path(), RUST_FMT_SHAPED, "rustfmtlike", "rustfmt");
    let call = json!({"paths": ["src/lib.rs"]});
    catalogue
        .invoke("rustfmtlike_fmt", &call)
        .expect("no scope, nothing read");
    let mut catalogue = catalogue.scoped(rules(&["**=allowed"]));
    catalogue
        .invoke("rustfmtlike_fmt", &call)
        .expect("a scope that refuses nothing, nothing read");
    catalogue.narrow(rules(&["protected/**=denied"]));
    let refused = catalogue
        .invoke("rustfmtlike_fmt", &call)
        .expect_err("read once a layer refuses something");
    assert!(
        refused
            .message()
            .contains("with an attribute that carries `path`"),
        "{refused}"
    );
    assert_eq!(started.lock().expect("not poisoned").len(), 2);
}

// --- the bounds at their real values, and the step layer --------------------------------------

#[test]
fn a_written_directory_at_exactly_the_bounds_is_walked_and_one_past_them_is_refused() {
    // The unit's own cases pin the bounds' arithmetic with a bound of three; these pin the values
    // the unit states, 200,000 entries and 32 directories deep.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir(dir.path().join("at")).expect("at");
    for file in 0..200_000 {
        std::fs::write(dir.path().join(format!("at/{file}.go")), "").expect("a file");
    }
    std::fs::create_dir(dir.path().join("past")).expect("past");
    for file in 0..=200_000 {
        std::fs::write(dir.path().join(format!("past/{file}.go")), "").expect("a file");
    }
    let twelve: Vec<String> = (1..=32).map(|level| format!("d{level}")).collect();
    std::fs::create_dir_all(dir.path().join("deep").join(twelve.join("/"))).expect("32 deep");
    let (catalogue, started) = recording(dir.path());
    let catalogue = catalogue.scoped(rules(&["protected/**=denied"]));

    formatting(&catalogue, "at").expect("200,000 entries are within the bound");
    formatting(&catalogue, "deep").expect("32 directories deep is within the bound");
    let refused = formatting(&catalogue, "past").expect_err("200,001 entries are past it");
    assert!(
        refused.message().contains("more than 200000 entries"),
        "{refused}"
    );
    assert_eq!(started.lock().expect("not poisoned").len(), 2);
}

#[test]
fn a_step_that_narrows_refuses_a_written_directory_holding_a_link_to_what_it_denies() {
    // The run's layer refuses nothing; the step's refuses `protected/**`, and the walk is made
    // for it and judges the link by the step's rule.
    let dir = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir(dir.path().join("protected")).expect("the refused directory");
    std::fs::write(dir.path().join("protected/kept.go"), "package protected\n").expect("a source");
    std::fs::create_dir(dir.path().join("src")).expect("src");
    std::os::unix::fs::symlink("../protected/kept.go", dir.path().join("src/alias.go"))
        .expect("a link to a refused file");
    let (catalogue, started) = recording(dir.path());
    let mut catalogue = catalogue.scoped(rules(&["**=allowed"]));
    formatting(&catalogue, "src").expect("the run's layer alone refuses nothing");
    catalogue.narrow(rules(&["protected/**=denied"]));

    // `src/../src` is refused by the registry before the scope ("not a contained relative path").
    for path in ["src", "src/", "./src", "src/."] {
        let refused = formatting(&catalogue, path).expect_err("the step refuses the link");
        assert!(
            refused.message().contains("`protected/**=denied`"),
            "{path}: {refused}"
        );
    }
    assert_eq!(started.lock().expect("not poisoned").len(), 1);
}
