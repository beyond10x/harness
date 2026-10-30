//! Integrity conformance checks for repository profiles (`story:repo-local-profiles`), written by
//! the second security pass of wave d288 against U1 correction 1 (`9631c61`) and the contract it
//! wrote about itself:
//!
//! - `crates/harness-cli/src/trust.rs`, `untrust`: every entry the path can name is removed, "the
//!   path resolved the way `trust` resolves it", and "a dangling symbolic link on the way is
//!   followed to where it points".
//! - `website/docs/guides/profiles.md`: "No refusal quotes the file's values; ... so nothing from
//!   the file acts on your terminal", and "A run's default write scope denies `.b10x/**` as well as
//!   `.git/**`, so a run that may write cannot rewrite the profile the next run applies."
//! - `AGENTS.md` § Credentials, "Named exception: a trusted repository's `provider`".
//!
//! Every case runs on a machine of its own: `HOME`, `XDG_CONFIG_HOME` and `XDG_STATE_HOME` point
//! into one temporary directory, so no case reads or writes the operator's real configuration or
//! trust record. A run names a closed local port as its endpoint, so nothing reaches a network.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");
/// The environment variable the runs here name as their credential source.
const TEST_KEY_VAR: &str = "B10X_HARNESS_SECURITY_P2_KEY";
/// A local port nothing listens on.
const CLOSED_ENDPOINT: &str = "http://127.0.0.1:9";
/// OSC 0 (set the terminal title), ended by BEL, then a carriage return that hides the line.
const TERMINAL_CONTROL: &str = "\u{1b}]0;retitled\u{7}\r";

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

/// One operator's machine inside one temporary directory.
struct Machine {
    root: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().expect("a temporary directory"),
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    fn home(&self) -> PathBuf {
        self.path("home")
    }

    fn trust_record(&self) -> PathBuf {
        self.home()
            .join("config")
            .join("b10x")
            .join("harness-trust.toml")
    }

    fn directory(&self, relative: &str) -> PathBuf {
        let directory = self.path(relative);
        fs::create_dir_all(&directory).expect("a directory");
        directory
    }

    /// `relative` holding `text` as its `.b10x/harness.toml`.
    fn workspace(&self, relative: &str, text: &str) -> PathBuf {
        let workspace = self.directory(relative);
        fs::create_dir_all(workspace.join(".b10x")).expect("a .b10x directory");
        fs::write(workspace.join(".b10x").join("harness.toml"), text)
            .expect("the repository profile");
        workspace
    }

    fn command(&self, arguments: &[&str], directory: Option<&Path>) -> Command {
        fs::create_dir_all(self.home()).expect("a home directory");
        let mut command = Command::new(BINARY);
        command
            .stdin(Stdio::null())
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join("config"))
            .env("XDG_STATE_HOME", self.home().join("state"))
            .env(TEST_KEY_VAR, "security-fixture-value")
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .args(arguments)
            .current_dir(directory.unwrap_or_else(|| self.root.path()));
        command
    }

    fn invoke(&self, arguments: &[&str], directory: Option<&Path>) -> Output {
        let output = self
            .command(arguments, directory)
            .output()
            .expect("the binary runs");
        Output {
            status: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn trust(&self, path: &Path) {
        let output = self.invoke(&["profiles", "trust", utf8(path)], None);
        assert_eq!(output.status, Some(0), "trusted: {}", output.stderr);
    }

    fn untrust(&self, path: &Path) -> Output {
        self.invoke(&["profiles", "untrust", utf8(path)], None)
    }

    /// The paths the trust record names, sorted.
    fn recorded(&self) -> Vec<String> {
        let text = fs::read_to_string(self.trust_record()).expect("a trust record");
        let parsed: toml::Table = toml::from_str(&text).expect("the record parses");
        let mut paths: Vec<String> = parsed["repositories"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|path| path.as_str().expect("a string").to_owned())
            .collect();
        paths.sort();
        paths
    }
}

fn utf8(path: &Path) -> &str {
    path.to_str().expect("a UTF-8 path")
}

fn canonical(path: &Path) -> String {
    utf8(&path.canonicalize().expect("canonical")).to_owned()
}

/// Every character in `text` a terminal acts on rather than shows: controls other than the line
/// end `println!` writes itself, and the bidi embeddings, overrides and isolates.
fn unshown(text: &str) -> Vec<char> {
    text.chars()
        .filter(|character| {
            (character.is_control() && *character != '\n')
                || matches!(*character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// `profiles untrust` after the correction: lexical `..`, the deepest existing ancestor, dangling
// links followed, both spellings removed.
// ---------------------------------------------------------------------------------------------

/// The doc comment on `trust::untrust` says the first entry removed is "the path resolved the way
/// `trust` resolves it". `trust` canonicalises, so `link/../sibling` is the sibling of the link's
/// *target*. `untrust` now takes `..` lexically before anything is resolved, even for a path that
/// exists, so the same spelling names the sibling of the *link*: it removes an entry the operator
/// did not name and leaves the one they did. Before the correction an existing path was
/// canonicalised first, and this held.
#[cfg(unix)]
#[test]
fn untrust_of_an_existing_path_spelled_through_a_link_and_dot_dot_removes_what_trust_recorded() {
    let machine = Machine::new();
    let project = machine.directory("data/x/proj");
    let named = machine.directory("data/x/sibling");
    let bystander = machine.directory("sibling");
    let link = machine.path("proj-link");
    std::os::unix::fs::symlink(&project, &link).expect("a symlink");
    let spelled = link.join("..").join("sibling");
    machine.trust(&spelled);
    machine.trust(&bystander);
    let mut before = vec![canonical(&named), canonical(&bystander)];
    before.sort();
    assert_eq!(
        machine.recorded(),
        before,
        "`trust` resolved `{}` the way the kernel does",
        spelled.display()
    );

    let output = machine.untrust(&spelled);

    assert_eq!(output.status, Some(0), "untrust: {}", output.stderr);
    assert_eq!(
        machine.recorded(),
        vec![canonical(&bystander)],
        "story:repo-local-profiles — `profiles untrust {}` (the spelling `trust` was given) did not \
         remove the entry `trust` recorded for it, `{}`, and removed `{}` instead, which the \
         operator did not name: `..` is taken lexically before the existing path is canonicalised. \
         untrust said: {}",
        spelled.display(),
        canonical(&named),
        canonical(&bystander),
        output.stdout.trim()
    );
}

/// A dangling relative link is followed against its parent *as spelled*, then `..` is taken
/// lexically, so a `..` in the link's target cancels the symlinked directory the link sits in
/// rather than the directory it really is in. Trusted through `work/current -> ../releases/v2`
/// where `work` is itself a link, the entry stays after the release is deleted, and the command
/// says it was not trusted.
#[cfg(unix)]
#[test]
fn untrust_through_a_dangling_relative_link_under_a_symlinked_directory_removes_its_entry() {
    let machine = Machine::new();
    let real_work = machine.directory("data/work");
    let release = machine.directory("data/releases/v2");
    std::os::unix::fs::symlink("../releases/v2", real_work.join("current"))
        .expect("a relative link, the way a deployment points at its release");
    let work = machine.path("work");
    std::os::unix::fs::symlink(&real_work, &work).expect("a symlinked working directory");
    let spelled = work.join("current");
    machine.trust(&spelled);
    let entry = canonical(&release);
    assert_eq!(machine.recorded(), vec![entry.clone()], "trusted");
    fs::remove_dir_all(&release).expect("the release is deleted, leaving the link dangling");

    let output = machine.untrust(&spelled);

    assert_eq!(output.status, Some(0), "untrust: {}", output.stderr);
    assert!(
        !machine.recorded().contains(&entry),
        "story:repo-local-profiles — `profiles untrust {}`, the path it was trusted through, left \
         `{entry}` trusted after the release was deleted: the dangling link's `..` was taken \
         against the unresolved `work` link. A release later created there is trusted again. \
         untrust said: {}",
        spelled.display(),
        output.stdout.trim()
    );
}

/// The link limit is what keeps `resolve` from following a cycle for ever. Cleared here: a cycle of
/// two links ends, and the command answers.
#[cfg(unix)]
#[test]
fn untrust_of_a_cycle_of_links_ends() {
    let machine = Machine::new();
    let directory = machine.directory("loop");
    std::os::unix::fs::symlink("b/x", directory.join("a")).expect("a link");
    std::os::unix::fs::symlink("a/y", directory.join("b")).expect("a link");
    let mut child = machine
        .command(
            &["profiles", "untrust", utf8(&directory.join("a").join("z"))],
            None,
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the binary starts");
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().expect("waits") {
            break Some(status);
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(
        status.is_some(),
        "story:repo-local-profiles — `profiles untrust` did not end within 30 s on a cycle of links"
    );
}

/// The correction made `untrust` follow a dangling link, and the line it prints names where the
/// link led. A repository's committed link target is the repository's text, so it is printed raw.
/// Constructed: no caller was found who names a repository's own link to `untrust`.
#[cfg(unix)]
#[test]
fn untrust_through_a_dangling_link_does_not_print_its_target_raw() {
    let machine = Machine::new();
    let workspace = machine.directory("ws");
    std::os::unix::fs::symlink(format!("gone{TERMINAL_CONTROL}"), workspace.join("old"))
        .expect("a dangling link whose target holds control characters");

    let output = machine.untrust(&workspace.join("old"));

    assert_eq!(output.status, Some(0), "untrust: {}", output.stderr);
    let raw = unshown(&format!("{}{}", output.stdout, output.stderr));
    assert!(
        raw.is_empty(),
        "story:repo-local-profiles — `profiles untrust` printed a dangling link's target with its \
         control characters {raw:?} raw\nstdout: {:?}",
        output.stdout
    );
}

// ---------------------------------------------------------------------------------------------
// What still reaches the terminal raw.
// ---------------------------------------------------------------------------------------------

/// The correction escapes the `model` in `profiles explain` and deliberately admits a `model`
/// holding control characters at read. A run then hands it to `harness-llm`, whose binding refuses
/// an id that is not printable ASCII and quotes it raw (`` `<model>`: binding id must be … ``): the
/// same text, refused before any request, written to the terminal as it is.
#[test]
fn a_run_does_not_write_a_repository_model_raw_to_the_terminal() {
    let machine = Machine::new();
    let workspace = machine.workspace(
        "work",
        "[default]\nmodel = \"m\\u001B]0;retitled\\u0007\\rhidden\"\n",
    );
    machine.trust(&workspace);

    let output = machine.invoke(
        &[
            "run",
            "--no-session",
            "--input",
            "say hello",
            "--workspace",
            utf8(&workspace),
            "--base-url",
            CLOSED_ENDPOINT,
            "--api-key-env",
            TEST_KEY_VAR,
        ],
        None,
    );

    assert_eq!(
        output.status,
        Some(1),
        "refused before any request: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("retitled"),
        "the refusal names the repository's model; stderr: {}",
        output.stderr
    );
    let raw = unshown(&output.stderr);
    assert!(
        raw.is_empty(),
        "story:repo-local-profiles — a run in a trusted repository wrote the repository profile's \
         `model` to stderr with its control characters {raw:?} raw (`profiles explain` escapes the \
         same value)\nstderr: {:?}",
        output.stderr
    );
}

/// A symlinked `.b10x` directory is refused by name, and the refusal says where the file
/// "resolves to". That path is the link's target, which is the repository's text: a later commit
/// to a trusted repository can make it hold any character but `/` and NUL.
#[cfg(unix)]
#[test]
fn the_refusal_of_a_symlinked_b10x_directory_does_not_print_the_link_target_raw() {
    let machine = Machine::new();
    let workspace = machine.directory("work");
    let target = format!("profiles{TERMINAL_CONTROL}");
    fs::create_dir_all(workspace.join(&target)).expect("the directory the link names");
    fs::write(
        workspace.join(&target).join("harness.toml"),
        "[default]\nmodel = \"m\"\n",
    )
    .expect("a profile behind the link");
    std::os::unix::fs::symlink(&target, workspace.join(".b10x")).expect("a symlinked .b10x");
    machine.trust(&workspace);

    let output = machine.invoke(
        &[
            "run",
            "--no-session",
            "--input",
            "say hello",
            "--workspace",
            utf8(&workspace),
            "--base-url",
            CLOSED_ENDPOINT,
            "--api-key-env",
            TEST_KEY_VAR,
            "--model",
            "m",
        ],
        None,
    );

    assert_eq!(output.status, Some(1), "refused: {}", output.stderr);
    assert!(
        output.stderr.contains("symbolic link"),
        "refused as a link: {}",
        output.stderr
    );
    let raw = unshown(&output.stderr);
    assert!(
        raw.is_empty(),
        "story:repo-local-profiles — the refusal of a trusted repository's symlinked `.b10x` wrote \
         the link's target with its control characters {raw:?} raw\nstderr: {:?}",
        output.stderr
    );
}

/// The correction shows the workspace path through `inert` in the untrusted notice and in
/// `profiles list` for an untrusted workspace, "whatever the workspace is called". For a trusted
/// one, `profiles list` and the `profile repository <- …` line of `profiles explain` print the same
/// path raw. Constructed: a workspace whose name holds control characters, as the adversary's
/// notice case was.
#[test]
fn profiles_list_and_explain_show_a_trusted_workspace_path_escaped() {
    let machine = Machine::new();
    let workspace = machine.workspace(
        &format!("work{TERMINAL_CONTROL}"),
        "[default]\nmodel = \"m\"\n",
    );
    machine.trust(&workspace);

    let list = machine.invoke(&["profiles", "list"], Some(&workspace));
    let explain = machine.invoke(&["profiles", "explain"], Some(&workspace));

    assert_eq!(list.status, Some(0), "list: {}", list.stderr);
    assert_eq!(explain.status, Some(0), "explain: {}", explain.stderr);
    assert!(
        list.stdout.contains("(trusted)") && explain.stdout.contains("profile repository <-"),
        "both name the trusted repository profile\nlist: {}\nexplain: {}",
        list.stdout,
        explain.stdout
    );
    let raw = unshown(&format!("{}{}", list.stdout, explain.stdout));
    assert!(
        raw.is_empty(),
        "story:repo-local-profiles — `profiles list` or `profiles explain` printed a trusted \
         workspace's path with its control characters {raw:?} raw, where the untrusted branch \
         escapes it\nlist: {:?}\nexplain: {:?}",
        list.stdout,
        explain.stdout
    );
}

// ---------------------------------------------------------------------------------------------
// The `.b10x/**=denied` default write scope.
// ---------------------------------------------------------------------------------------------

fn scope_of(rules: &[String]) -> harness_catalogue::Scope {
    harness_catalogue::Scope::of(
        rules
            .iter()
            .map(|rule| harness_catalogue::ScopeRule::parse(rule).expect("a rule"))
            .collect(),
    )
}

/// Cleared here: every spelling of the repository profile a file tool can be handed is refused by
/// the default scope, and so is a write through the directory or the workspace root.
#[test]
fn the_default_write_scope_refuses_every_spelling_of_the_repository_profile() {
    let scope = scope_of(&b10x_harness_cli::profile::default_write_scope());
    for spelling in [
        ".b10x/harness.toml",
        "./.b10x/harness.toml",
        ".b10x/./harness.toml",
        ".b10x//harness.toml",
        "src/../.b10x/harness.toml",
        ".b10x/sub/../harness.toml",
    ] {
        for operation in ["file.write", "file.edit"] {
            assert!(
                scope.refusal(operation, spelling).is_some(),
                "story:repo-local-profiles — {operation} `{spelling}` is admitted by the default \
                 write scope"
            );
        }
    }
    for directory in [".b10x", "./.b10x/", "."] {
        assert!(
            scope
                .refusal_of_directory("file.write", directory)
                .is_some(),
            "story:repo-local-profiles — a write through `{directory}` is admitted by the default \
             write scope"
        );
    }
}

/// The `--write-scope` values in a fenced command block of `document` that runs `b10x-harness run`.
fn typed_write_scope(document: &str) -> Vec<String> {
    let block = document
        .split("```")
        .find(|block| block.contains("b10x-harness run") && block.contains("--write-scope"))
        .expect("a `b10x-harness run` block typing `--write-scope`");
    block
        .split("--write-scope")
        .skip(1)
        .map(|rest| {
            rest.trim_start()
                .trim_start_matches('\'')
                .split('\'')
                .next()
                .expect("a quoted rule")
                .to_owned()
        })
        .collect()
}

/// The guide says a typed `write-scope` replaces the default, "keep `.b10x/**=denied` in it", and
/// that "a run that may write cannot rewrite the profile the next run applies". The tutorial the
/// profiles guide's `confined-change` section feeds types its own `--write-scope` and drops it, so
/// a run made by following it in a trusted repository may rewrite `.b10x/harness.toml`.
#[test]
fn the_confined_change_tutorial_run_refuses_a_write_to_the_repository_profile() {
    let tutorial = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../website/docs/tutorials/confined-change.md");
    let document = fs::read_to_string(&tutorial).expect("the tutorial");
    let typed = typed_write_scope(&document);
    assert!(
        typed.iter().any(|rule| rule == ".git/**=denied"),
        "the tutorial types its own scope: {typed:?}"
    );

    let scope = scope_of(&typed);

    assert!(
        scope.refusal("file.write", ".b10x/harness.toml").is_some(),
        "story:repo-local-profiles — the run `{}` documents types `--write-scope` {typed:?}, which \
         replaces the default and admits a write to `.b10x/harness.toml`: a run made by following \
         it in a trusted repository can choose the next run's provider, model and turn budget",
        tutorial.display()
    );
}

// ---------------------------------------------------------------------------------------------
// The credential exception in AGENTS.md, read against the code.
// ---------------------------------------------------------------------------------------------

/// The paragraph of `document` that starts with `opening`, up to the next blank line or list item.
fn paragraph<'a>(document: &'a str, opening: &str) -> &'a str {
    let start = document.find(opening).expect("the paragraph");
    let rest = &document[start..];
    let end = [rest.find("\n\n"), rest.find("\n- ")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(rest.len());
    &rest[..end]
}

/// Every backticked `--flag` in `text`.
fn flags(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .filter(|quoted| quoted.starts_with("--"))
        .map(str::to_owned)
        .collect()
}

/// The exception says a repository's `provider` reads that provider's default credential "exactly
/// as a typed `--provider` would". `run` has no `--provider`: a provider is chosen only in a
/// config file, by `[default]`, a `-p` profile or the repository. The guide repeats the sentence.
#[test]
fn the_credential_exception_names_only_flags_a_run_has() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let agents = fs::read_to_string(workspace.join("AGENTS.md")).expect("AGENTS.md");
    let guide = fs::read_to_string(workspace.join("website/docs/guides/profiles.md"))
        .expect("the profiles guide");
    let mut named = flags(paragraph(
        &agents,
        "**Named exception: a trusted repository's `provider`.**",
    ));
    named.extend(flags(paragraph(
        &guide,
        "A repository's `provider` reads that provider's default credential",
    )));
    assert!(!named.is_empty(), "the exception names flags");
    let machine = Machine::new();
    let help = machine.invoke(&["run", "--help"], None);
    assert_eq!(help.status, Some(0), "help: {}", help.stderr);

    let missing: Vec<&String> = named
        .iter()
        .filter(|flag| !help.stdout.contains(&format!("{flag} ")))
        .collect();

    assert!(
        missing.is_empty(),
        "story:repo-local-profiles — the credential exception in AGENTS.md and the profiles guide \
         compares a repository's `provider` to {missing:?}, which `b10x-harness run` does not \
         have"
    );
}
