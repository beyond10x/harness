//! Adversary pass 1 on `story:repo-local-profiles` (wave d288, unit U1).
//!
//! Every case drives the shipped binary on a machine of its own: `HOME`, `XDG_CONFIG_HOME` and
//! `XDG_STATE_HOME` point into one temporary directory. No case needs a model endpoint: each one
//! ends in a refusal, a `profiles` verb, or `profiles trust`/`untrust`. The base URL the runs name
//! is a closed local port, so a run that got past the refusal under test would fail to connect
//! rather than reach anything.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");
/// The environment variable the runs here name as their credential source.
const TEST_KEY_VAR: &str = "B10X_HARNESS_ADVERSARY_KEY";
/// A local port nothing listens on.
const CLOSED_ENDPOINT: &str = "http://127.0.0.1:9";

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

    /// A workspace at `relative`, holding `text` as its `.b10x/harness.toml`.
    fn workspace(&self, relative: &str, text: &str) -> PathBuf {
        let workspace = self.path(relative);
        fs::create_dir_all(workspace.join(".b10x")).expect("a .b10x directory");
        fs::write(workspace.join(".b10x").join("harness.toml"), text)
            .expect("the repository profile");
        workspace
    }

    fn invoke(&self, arguments: &[&str], directory: Option<&Path>) -> Output {
        fs::create_dir_all(self.home()).expect("a home directory");
        let mut command = Command::new(BINARY);
        command
            .stdin(Stdio::null())
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join("config"))
            .env("XDG_STATE_HOME", self.home().join("state"))
            .env(TEST_KEY_VAR, "adversary-fixture-value")
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .args(arguments)
            .current_dir(directory.unwrap_or_else(|| self.root.path()));
        let output = command.output().expect("the binary runs");
        Output {
            status: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn trust(&self, path: &Path) {
        let path = path.to_str().expect("a UTF-8 path");
        let output = self.invoke(&["profiles", "trust", path], None);
        assert_eq!(output.status, Some(0), "trusted: {}", output.stderr);
    }

    fn record_text(&self) -> String {
        fs::read_to_string(self.trust_record()).unwrap_or_default()
    }

    /// A run over `workspace` against a closed port, with a typed endpoint and credential.
    fn run_over(&self, workspace: &Path) -> Output {
        let workspace = workspace.to_str().expect("a UTF-8 path");
        self.invoke(
            &[
                "run",
                "--no-session",
                "--input",
                "say hello",
                "--workspace",
                workspace,
                "--base-url",
                CLOSED_ENDPOINT,
                "--api-key-env",
                TEST_KEY_VAR,
                "--model",
                "adversary-model",
            ],
            None,
        )
    }
}

/// The record names `canonical` as a quoted TOML string.
fn record_names(record: &str, canonical: &Path) -> bool {
    let quoted = format!("\"{}\"", canonical.to_str().expect("a UTF-8 path"));
    record.contains(&quoted)
}

// ---------------------------------------------------------------------------------------------
// `profiles untrust` of a repository that is gone, named by any spelling but the canonical one.
//
// `trust::untrust` canonicalises, and when the path no longer exists falls back to
// `std::path::absolute`, which keeps `..` components and a trailing slash and resolves no
// symlink. The entry stays, and the command says "was not trusted; nothing changed" with exit 0.
// A directory later created at that path is then trusted without anyone saying so.
// ---------------------------------------------------------------------------------------------

#[test]
fn untrust_of_a_deleted_repository_named_relative_to_a_sibling_removes_its_entry() {
    let machine = Machine::new();
    let workspace = machine.workspace("work", "[default]\nmodel = \"m\"\n");
    let canonical = workspace.canonicalize().expect("canonical");
    machine.trust(&workspace);
    assert!(record_names(&machine.record_text(), &canonical));

    fs::remove_dir_all(&workspace).expect("the checkout is deleted");
    let sibling = machine.path("elsewhere");
    fs::create_dir_all(&sibling).expect("a sibling directory");
    let output = machine.invoke(&["profiles", "untrust", "../work"], Some(&sibling));

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert!(
        !record_names(&machine.record_text(), &canonical),
        "story:repo-local-profiles: `profiles untrust ../work` from a sibling directory, after the \
         checkout was deleted, left `{}` trusted and printed: {}record now: {}",
        canonical.display(),
        output.stdout,
        machine.record_text()
    );
}

#[test]
fn untrust_of_a_deleted_repository_named_with_a_trailing_slash_removes_its_entry() {
    let machine = Machine::new();
    let workspace = machine.workspace("work", "[default]\nmodel = \"m\"\n");
    let canonical = workspace.canonicalize().expect("canonical");
    machine.trust(&workspace);

    fs::remove_dir_all(&workspace).expect("the checkout is deleted");
    let spelled = format!("{}/", canonical.display());
    let output = machine.invoke(&["profiles", "untrust", &spelled], None);

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert!(
        !record_names(&machine.record_text(), &canonical),
        "story:repo-local-profiles: `profiles untrust {spelled}` after the checkout was deleted \
         left `{}` trusted and printed: {}",
        canonical.display(),
        output.stdout
    );
}

#[cfg(unix)]
#[test]
fn untrust_of_a_deleted_repository_through_the_symlinked_parent_it_was_trusted_by_removes_it() {
    let machine = Machine::new();
    fs::create_dir_all(machine.path("real")).expect("the real parent");
    std::os::unix::fs::symlink(machine.path("real"), machine.path("link")).expect("a symlink");
    let workspace = machine.workspace("real/work", "[default]\nmodel = \"m\"\n");
    let canonical = workspace.canonicalize().expect("canonical");
    let spelled = machine.path("link").join("work");
    machine.trust(&spelled);
    assert!(record_names(&machine.record_text(), &canonical));

    fs::remove_dir_all(&workspace).expect("the checkout is deleted");
    let spelled = spelled.to_str().expect("a UTF-8 path");
    let output = machine.invoke(&["profiles", "untrust", spelled], None);

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert!(
        !record_names(&machine.record_text(), &canonical),
        "story:repo-local-profiles: `profiles untrust {spelled}`, the very path `trust` was given, \
         left `{}` trusted once the checkout was deleted; it printed: {}",
        canonical.display(),
        output.stdout
    );
}

#[cfg(unix)]
#[test]
fn untrust_of_a_trusted_path_that_became_a_symlink_removes_the_entry_for_that_path() {
    let machine = Machine::new();
    let workspace = machine.workspace("work", "[default]\nmodel = \"m\"\n");
    let canonical = workspace.canonicalize().expect("canonical");
    machine.trust(&workspace);

    // The checkout is moved aside and the old path left pointing at it.
    fs::rename(&workspace, machine.path("work-moved")).expect("moved");
    std::os::unix::fs::symlink(machine.path("work-moved"), &workspace).expect("a symlink");
    let spelled = workspace.to_str().expect("a UTF-8 path");
    let output = machine.invoke(&["profiles", "untrust", spelled], None);

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert!(
        !record_names(&machine.record_text(), &canonical),
        "story:repo-local-profiles: `profiles untrust {spelled}` left the entry for `{}` in the \
         record because the path now resolves elsewhere; a directory put back there is trusted \
         again. It printed: {}",
        canonical.display(),
        output.stdout
    );
}

// ---------------------------------------------------------------------------------------------
// A trusted repository's file is read at every run, so its text is a later commit's to choose.
// What the binary prints from it reaches the operator's terminal.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_refused_key_carrying_control_characters_reaches_stderr_escaped_and_on_one_line() {
    let machine = Machine::new();
    // A quoted TOML key: ESC [ 2 J clears the screen, and a newline forges a second line.
    let workspace = machine.workspace(
        "work",
        "[default]\n\"x\\u001B[2Jforged\\nwarning: all clear\" = 1\n",
    );
    machine.trust(&workspace);

    let output = machine.run_over(&workspace);

    assert_ne!(output.status, Some(0), "the refused key refuses the run");
    assert!(
        output.stderr.contains(".b10x/harness.toml"),
        "the refusal names the file: {}",
        output.stderr
    );
    assert!(
        !output.stderr.contains('\u{1b}'),
        "story:repo-local-profiles: the refusal of a trusted repository profile wrote a raw ESC \
         from the file's key to stderr: {:?}",
        output.stderr
    );
    let forged: Vec<&str> = output
        .stderr
        .lines()
        .filter(|line| line.starts_with("warning: all clear"))
        .collect();
    assert!(
        forged.is_empty(),
        "story:repo-local-profiles: a key in the repository profile forged a stderr line of its \
         own: {:?}",
        output.stderr
    );
}

#[test]
fn profiles_explain_prints_a_repository_model_without_raw_control_characters() {
    let machine = Machine::new();
    // OSC 0 sets the terminal title; BEL ends it.
    let workspace = machine.workspace("work", "[default]\nmodel = \"m\\u001B]0;owned\\u0007x\"\n");
    machine.trust(&workspace);

    let output = machine.invoke(&["profiles", "explain"], Some(&workspace));

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert!(
        output.stdout.contains("--model"),
        "explain prints the model: {}",
        output.stdout
    );
    assert!(
        !output.stdout.contains('\u{1b}') && !output.stdout.contains('\u{7}'),
        "story:repo-local-profiles: `profiles explain` printed a trusted repository's `model` \
         with its ESC and BEL raw: {:?}",
        output.stdout
    );
}

// ---------------------------------------------------------------------------------------------
// The untrusted notice is one stderr line (decision 4).
// ---------------------------------------------------------------------------------------------

#[test]
fn the_untrusted_notice_is_one_line_whatever_the_workspace_directory_is_called() {
    let machine = Machine::new();
    let workspace = machine.workspace("work\nspace", "[default]\nmodel = \"m\"\n");

    let output = machine.invoke(&["profiles", "explain"], Some(&workspace));

    let notice_lines = output
        .stderr
        .lines()
        .filter(|line| !line.is_empty())
        .count();
    assert_eq!(
        notice_lines, 1,
        "story:repo-local-profiles: the untrusted notice for a workspace whose name holds a \
         newline is not one stderr line: {:?}",
        output.stderr
    );
}

// ---------------------------------------------------------------------------------------------
// `max-turns` bounds.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_repository_max_turns_of_zero_is_refused_naming_the_repository_file() {
    let machine = Machine::new();
    let workspace = machine.workspace("work", "[default]\nmax-turns = 0\n");
    machine.trust(&workspace);

    let output = machine.run_over(&workspace);

    assert_ne!(
        output.status,
        Some(0),
        "a ceiling of zero admits no turn: {}",
        output.stderr
    );
    let refusal = format!("{}{}", output.stdout, output.stderr);
    assert!(
        refusal.contains("max_turns") || refusal.contains("max-turns"),
        "the refusal is the zero bound and nothing else: {refusal}"
    );
    assert!(
        refusal.contains(".b10x/harness.toml"),
        "story:repo-local-profiles: the repository profile's `max-turns = 0` was accepted by the \
         reader (\"a whole number, zero or more\") and the run then refused a zero bound without \
         naming the file it came from, while the operator typed no `--max-turns`: {refusal}"
    );
}
