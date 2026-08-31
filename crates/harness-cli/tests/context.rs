//! What `--context` does with a file that is not there, and with one that is.
//!
//! `--context` is a declaration about the run, not a convenience: a run given a smaller context
//! than its invocation says it has cannot be reproduced from that invocation. So a file that
//! cannot be read **refuses the run** rather than warning and going on
//! (`harness-cli/src/lib.rs:1636-1641`), and the refusal happens in `prepare`, before the first
//! request — which is the part nothing pinned. The two tests here are the two halves of that
//! claim: nothing was sent and nothing was filed when the file is absent, and the file's own path
//! labels its text in the request when it is present.
//!
//! **`--hooks` is the same rule, so it is pinned beside it.** `Hooks::load` reads the named file
//! (`harness-cli/src/hooks.rs:162-165`) and `prepare` propagates its refusal with `?`
//! (`harness-cli/src/lib.rs:1185-1189`) — later in `prepare` than the context files, and still
//! before the first request and before anything is written. A hook is the operator's own program;
//! a run that could not read the file naming it and went on would be a run whose gate is missing
//! and which says so nowhere.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

/// The deterministic local endpoint, writing every request it serves to `record`.
///
/// The record is the evidence both tests turn on: its absence says no request was made, and its
/// one line says what the request carried.
struct Fixture {
    child: Child,
    base_url: String,
}

impl Fixture {
    fn start(scenario: &str, record: &Path) -> Self {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("harness-responses")
            .join("tests")
            .join("fixtures")
            .join("fake_responses.py");
        let mut child = Command::new("python3")
            .arg(&script)
            .arg("--scenario")
            .arg(scenario)
            .arg("--record")
            .arg(record)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the fixture starts; python3 must be available");
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().expect("piped stdout"))
            .read_line(&mut line)
            .expect("the fixture announces its address");
        let ready: Value = serde_json::from_str(&line).expect("the announcement is JSON");
        Self {
            base_url: ready["base_url"].as_str().expect("a base url").to_owned(),
            child,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

/// One `run` against the fixture, over a workspace the test owns.
fn run_against(fixture: &Fixture, extra: &[&str], workspace: &Path) -> Output {
    let mut arguments = vec![
        "run",
        "--base-url",
        &fixture.base_url,
        "--model",
        "b10x-emulated",
        "--input",
        "read the readme and tell me what it says",
    ];
    arguments.extend_from_slice(extra);
    let output = Command::new(BINARY)
        .args(&arguments)
        .arg("--workspace")
        .arg(workspace)
        .output()
        .expect("the binary runs");
    Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A workspace with one readable file, so the run has something legitimate to do.
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut file = fs::File::create(dir.path().join("README.md")).expect("create");
    file.write_all(b"hello harness\n").expect("write");
    dir
}

/// What the endpoint was sent, one entry per request, or nothing at all when it was never called.
fn requests(record: &Path) -> Vec<Value> {
    let Ok(text) = fs::read_to_string(record) else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("a recorded request is JSON"))
        .collect()
}

/// The whole refusal a declared-but-absent file produces, asserted from outside the process.
///
/// `flag` is the declaration and `absent` the file it points at; `rule` is the words the refusal
/// must use. The rest is what "before the first request" looks like to a caller above this
/// process: exit `1`, the path on stderr, an endpoint that was up and never called, and a session
/// directory the run left exactly as it found it.
fn refuses_before_anything_happens(flag: &str, absent: &str, rule: &str) {
    let workspace = workspace();
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let record = scratch.path().join("requests.jsonl");
    // A directory the run may file a session in. It is left empty on purpose: an empty directory
    // after the run is what "no session was started" looks like from outside the process.
    let sessions = tempfile::tempdir().expect("a temporary directory");

    let fixture = Fixture::start("text", &record);
    let output = run_against(
        &fixture,
        &[
            flag,
            absent,
            "--session-dir",
            sessions.path().to_str().expect("utf-8 path"),
            "--json",
        ],
        workspace.path(),
    );

    // A refusal, not a warning, and `1` rather than `2`: on this command line `2` means the run
    // happened and stopped for a named reason.
    assert_eq!(output.status, Some(1), "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains(&format!("{rule} `{absent}`")),
        "the refusal names the rule and the path, so the invocation can be fixed: {}",
        output.stderr
    );

    // Before the first request. The endpoint was up and reachable — the fixture served this same
    // invocation without `--context` in the test below — and it was never called.
    assert!(
        requests(&record).is_empty(),
        "nothing was sent: {:?}",
        requests(&record)
    );

    // And before anything was filed. `--session-dir` named a directory and the run left it as it
    // found it, so there is no transcript of a run that never started.
    let filed: Vec<PathBuf> = fs::read_dir(sessions.path())
        .expect("the session directory exists")
        .map(|entry| entry.expect("a directory entry").path())
        .collect();
    assert!(filed.is_empty(), "no session record was written: {filed:?}");

    // What a driver above this process reads: the one line that says the run never started, rather
    // than an exit status with no record at all.
    let refused: Value = serde_json::from_str(output.stdout.trim()).unwrap_or_else(|error| {
        panic!("the refusal is one JSON line ({error}): {}", output.stdout)
    });
    assert_eq!(refused["kind"], "refused", "{refused}");
    assert!(
        refused["reason"].as_str().expect("a reason").contains(rule),
        "{refused}"
    );
}

#[test]
fn a_declared_context_file_that_is_absent_refuses_the_run_before_any_session() {
    // Named but never created, and under a directory that does not exist either, so the failure is
    // the file's own absence and not a permission on the way to it.
    refuses_before_anything_happens(
        "--context",
        "/nonexistent/b10x-harness-context-absent-9c1f4b7e.md",
        "reading the context file",
    );
}

#[test]
fn a_declared_hooks_file_that_is_absent_refuses_the_run_before_any_session() {
    // The same rule one flag over. `Hooks::load` refuses (`harness-cli/src/hooks.rs:162-165`) and
    // `prepare` propagates it with `?` (`harness-cli/src/lib.rs:1185-1189`) — later in `prepare`
    // than the context files, and on the same side of the first request. A run that could not read
    // the file naming the operator's own program and started anyway would be a run whose gate is
    // missing and which says so nowhere.
    refuses_before_anything_happens(
        "--hooks",
        "/nonexistent/b10x-harness-hooks-absent-4d02e6a1.json",
        "reading the hooks file",
    );
}

#[test]
fn a_declared_context_file_that_exists_is_handed_to_the_run_labelled_by_its_path() {
    // The positive twin, observable without a live model: the standing instruction rides at the
    // head of `input` as a developer message, and the fixture records that text.
    let workspace = workspace();
    let scratch = tempfile::tempdir().expect("a temporary directory");
    let record = scratch.path().join("requests.jsonl");
    let given = scratch.path().join("architecture.md");
    fs::write(&given, "the harness drives the model directly\n").expect("write the context file");
    let path = given.to_str().expect("utf-8 path");

    let fixture = Fixture::start("text", &record);
    let output = run_against(
        &fixture,
        &["--context", path, "--no-session"],
        workspace.path(),
    );
    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);

    let requests = requests(&record);
    assert!(!requests.is_empty(), "the endpoint was called");
    let instruction = requests[0]["first_input_text"]
        .as_str()
        .expect("the standing instruction heads the input");
    // The semantic kind and source are the only framing the model needs. Cache and digest data stay
    // in the run manifest rather than spending prompt tokens.
    assert!(
        instruction.contains("kind=\"provided_context\""),
        "{instruction}"
    );
    assert!(
        instruction.contains(&format!("source=\"{path}\"")),
        "labelled by its path: {instruction}"
    );
    assert!(
        instruction.contains("the harness drives the model directly"),
        "and the file's own text is there: {instruction}"
    );
}

#[test]
fn toolchain_context_exposes_only_the_named_fact_that_can_help_the_model() {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(BINARY)
        .args(["context", "show", "--toolchain", "rust", "--body"])
        .arg("--workspace")
        .arg(workspace)
        .output()
        .expect("the binary runs");
    let body = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(body.contains("rust.version="), "{body}");
    assert!(!body.contains("sha256="), "{body}");
    assert!(!body.contains("cache="), "{body}");
    assert!(!body.contains("mount="), "{body}");
}
