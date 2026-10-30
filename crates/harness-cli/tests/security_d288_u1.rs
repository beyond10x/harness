//! Integrity conformance checks for repository profiles (`story:repo-local-profiles`), written by
//! the second security pass of wave d288 against the unit's own contract:
//!
//! - `website/docs/guides/profiles.md`: "No refusal quotes the file's text."
//! - `crates/harness-cli/src/profile.rs`, `load_repository`: "A refusal never quotes the file's
//!   text."
//! - `crates/harness-cli/src/trust.rs`, `untrust`: "a path that no longer exists — a repository
//!   deleted after it was trusted — is made absolute instead, so its entry can still be removed by
//!   naming it."
//!
//! A trusted repository's file is read as it is at each run, so its text is whatever the
//! repository's latest commit says. A refusal that prints that text raw lets the file write
//! terminal control sequences (a carriage return that hides the line, an OSC title or clipboard
//! write) onto the operator's terminal or a CI log under the harness's own `error:` prefix.
//!
//! Every case runs on a machine of its own: `HOME`, `XDG_CONFIG_HOME` and `XDG_STATE_HOME` point
//! into one temporary directory, so no case reads or writes the operator's real configuration or
//! trust record. No case reaches a network: each refusal comes before any request.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

/// One operator's machine and one repository on it, inside one temporary directory.
struct Machine {
    root: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("a temporary directory");
        fs::create_dir_all(root.path().join("work")).expect("a workspace");
        fs::write(root.path().join("work").join("README.md"), "hello\n").expect("a file");
        Self { root }
    }

    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn trust_record(&self) -> PathBuf {
        self.home()
            .join("config")
            .join("b10x")
            .join("harness-trust.toml")
    }

    fn workspace(&self) -> PathBuf {
        self.root.path().join("work")
    }

    fn repository(&self, text: &str) -> &Self {
        fs::create_dir_all(self.workspace().join(".b10x")).expect("a .b10x directory");
        fs::write(self.workspace().join(".b10x").join("harness.toml"), text)
            .expect("the repository profile");
        self
    }

    fn invoke_in(&self, arguments: &[&str], directory: Option<&Path>) -> Output {
        let mut command = Command::new(BINARY);
        command
            .stdin(Stdio::null())
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.home().join("config"))
            .env("XDG_STATE_HOME", self.home().join("state"))
            .env_remove("OPENAI_API_KEY")
            .env_remove("ANTHROPIC_API_KEY")
            .args(arguments);
        if let Some(directory) = directory {
            command.current_dir(directory);
        }
        let output = command.output().expect("the binary runs");
        Output {
            status: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn invoke(&self, arguments: &[&str]) -> Output {
        self.invoke_in(arguments, None)
    }

    fn trust(&self, path: &Path) -> &Self {
        let output = self.invoke(&["profiles", "trust", path.to_str().expect("UTF-8")]);
        assert_eq!(output.status, Some(0), "trusted: {}", output.stderr);
        self
    }

    fn trusted(&self) -> &Self {
        self.trust(&self.workspace())
    }

    /// A run over the workspace that types nothing about the endpoint, so the repository's
    /// `provider` is consulted.
    fn run(&self, extra: &[&str]) -> Output {
        let workspace = self.workspace();
        let mut arguments = vec![
            "run",
            "--json",
            "--no-session",
            "--input",
            "read the readme",
            "--workspace",
            workspace.to_str().expect("UTF-8"),
        ];
        arguments.extend_from_slice(extra);
        self.invoke(&arguments)
    }

    /// The canonical paths the trust record names.
    fn recorded(&self) -> Vec<String> {
        let text = fs::read_to_string(self.trust_record()).expect("a trust record");
        let parsed: toml::Table = toml::from_str(&text).expect("the record parses");
        parsed["repositories"]
            .as_array()
            .expect("an array")
            .iter()
            .map(|path| path.as_str().expect("a string").to_owned())
            .collect()
    }
}

/// Every character in `text` that a terminal acts on rather than prints, except the line ends
/// `eprintln!` itself writes.
fn control_characters(text: &str) -> Vec<char> {
    text.chars()
        .filter(|character| character.is_control() && *character != '\n')
        .collect()
}

/// `parse_repository` names every refused key "by name", and the name is the file's text: a quoted
/// TOML key may hold any character through a `\u` escape. The refusal must name the key without
/// handing its control characters to the terminal.
#[test]
fn a_refused_key_name_carrying_control_characters_is_not_written_raw_to_stderr() {
    let machine = Machine::new();
    machine
        .repository(
            "[default]\nmodel = \"m\"\n\"approve\\rhidden\\u001B]0;retitled\\u0007\" = \"high\"\n",
        )
        .trusted();

    let output = machine.run(&[]);

    assert_eq!(output.status, Some(1), "refused: {}", output.stderr);
    assert!(
        output.stderr.contains(".b10x/harness.toml"),
        "the refusal names the file: {}",
        output.stderr
    );
    let raw = control_characters(&output.stderr);
    assert!(
        raw.is_empty(),
        "story:repo-local-profiles — the refusal of a trusted repository profile wrote the file's \
         own control characters {raw:?} raw to stderr (a key name is the file's text; a carriage \
         return hides the line before it, ESC ] sets the terminal title)\nstderr: {:?}",
        output.stderr
    );
}

/// The same for a parse failure: `parse_failure` keeps the parser's message, and the parser's
/// message for a duplicate key names the key.
#[test]
fn a_parse_failure_naming_a_key_with_control_characters_is_not_written_raw_to_stderr() {
    let machine = Machine::new();
    machine
        .repository("[default]\n\"m\\rx\\u001B[2K\" = 1\n\"m\\rx\\u001B[2K\" = 2\n")
        .trusted();

    let output = machine.run(&[]);

    assert_eq!(output.status, Some(1), "refused: {}", output.stderr);
    assert!(
        output.stderr.contains(".b10x/harness.toml"),
        "the refusal names the file: {}",
        output.stderr
    );
    let raw = control_characters(&output.stderr);
    assert!(
        raw.is_empty(),
        "story:repo-local-profiles — a trusted repository profile that does not parse had its own \
         control characters {raw:?} written raw to stderr through the parser's message\nstderr: \
         {:?}",
        output.stderr
    );
}

/// A trusted repository's `provider` value that names no provider is refused by
/// `provider::resolve`, whose message quotes the name it was given. The guide promises "No refusal
/// quotes the file's text"; at the least the value's control characters must not reach the
/// terminal raw. (`refuse_a_typed_credential_for_a_repository_provider` already prints the same
/// value with `{:?}`, which escapes them.)
#[test]
fn a_repository_provider_value_carrying_control_characters_is_not_written_raw_to_stderr() {
    let machine = Machine::new();
    machine
        .repository(
            "[default]\nprovider = \"claude\\r\\u001B[2Kopenai\\u001B]52;c;Zml4dHVyZQ==\\u0007\"\n",
        )
        .trusted();

    let output = machine.run(&[]);

    assert_eq!(output.status, Some(1), "refused: {}", output.stderr);
    assert!(
        !output.stdout.contains("\"started\""),
        "no run started: {}",
        output.stdout
    );
    let raw = control_characters(&output.stderr);
    assert!(
        raw.is_empty(),
        "story:repo-local-profiles — the trusted repository profile's `provider` value was quoted \
         into the refusal with its control characters {raw:?} raw (ESC ] 52 is a clipboard write \
         in terminals that honour it)\nstderr: {:?}",
        output.stderr
    );
}

/// `untrust` documents that a repository deleted after it was trusted can still be removed "by
/// naming it". Named the way a shell user names a sibling directory, `../<name>`, the entry stays,
/// the command exits 0 saying it "was not trusted", and a repository later cloned at that path is
/// trusted without anybody running `trust` again.
#[test]
fn untrusting_a_deleted_repository_named_through_a_parent_component_removes_its_entry() {
    let machine = Machine::new();
    let gone = machine.root.path().join("gone");
    let here = machine.root.path().join("here");
    fs::create_dir_all(&gone).expect("a repository");
    fs::create_dir_all(&here).expect("the directory the operator stands in");
    let canonical = gone
        .canonicalize()
        .expect("canonical")
        .to_str()
        .expect("UTF-8")
        .to_owned();
    machine.trust(&gone);
    assert_eq!(machine.recorded(), vec![canonical.clone()], "trusted");
    fs::remove_dir_all(&gone).expect("the repository is deleted");

    let output = machine.invoke_in(&["profiles", "untrust", "../gone"], Some(&here));

    assert_eq!(output.status, Some(0), "untrust: {}", output.stderr);
    assert!(
        !machine.recorded().contains(&canonical),
        "story:repo-local-profiles — `profiles untrust ../gone` for a deleted repository left \
         `{canonical}` trusted; a repository cloned there later is trusted again. untrust said: {}",
        output.stdout.trim()
    );
}

/// The same promise, for the path the operator trusted it through: a symlink whose target has
/// since been deleted. `trust` recorded the target; `untrust` of the link makes the link absolute
/// and leaves the target's entry.
#[cfg(unix)]
#[test]
fn untrusting_a_deleted_repository_by_the_symlink_it_was_trusted_through_removes_its_entry() {
    let machine = Machine::new();
    let real = machine.root.path().join("real");
    let link = machine.root.path().join("link");
    fs::create_dir_all(&real).expect("a repository");
    std::os::unix::fs::symlink(&real, &link).expect("a symlink");
    let canonical = real
        .canonicalize()
        .expect("canonical")
        .to_str()
        .expect("UTF-8")
        .to_owned();
    machine.trust(&link);
    assert_eq!(machine.recorded(), vec![canonical.clone()], "trusted");
    fs::remove_dir_all(&real).expect("the repository is deleted");

    let output = machine.invoke(&["profiles", "untrust", link.to_str().expect("UTF-8")]);

    assert_eq!(output.status, Some(0), "untrust: {}", output.stderr);
    assert!(
        !machine.recorded().contains(&canonical),
        "story:repo-local-profiles — `profiles untrust {}` (the path it was trusted through) left \
         `{canonical}` trusted after the repository was deleted. untrust said: {}",
        link.display(),
        output.stdout.trim()
    );
}
