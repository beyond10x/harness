//! Adversary pass on `story:started-record-carries-the-loops-cwd`, through the shipped binary.
//!
//! The unit's own end-to-end cases launch `run` in a directory and spell one relative
//! `--workspace`. These drive the record from the other places a directory can come from or go
//! to: a name that is not UTF-8, a symlink, `chat`, a delegate and a fork.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::Value;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

struct Fixture {
    child: Child,
    base_url: String,
}

impl Fixture {
    fn start(scenario: &str) -> Self {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("fake_responses.py");
        let mut child = Command::new("python3")
            .arg(&script)
            .arg("--scenario")
            .arg(scenario)
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

fn readme_in(dir: &Path) {
    let mut file = fs::File::create(dir.join("README.md")).expect("create");
    file.write_all(b"hello harness\n").expect("write");
}

/// A `--json` `run` launched in `directory`, with whatever else the case adds.
fn run_in(
    fixture: &Fixture,
    directory: &Path,
    extra: &[&std::ffi::OsStr],
    session: Option<&Path>,
) -> Output {
    let mut command = Command::new(BINARY);
    command.args([
        "run",
        "--base-url",
        &fixture.base_url,
        "--model",
        "b10x-emulated",
        "--api-key-env",
        "B10X_HARNESS_TEST_KEY",
        "--json",
        "--input",
        "read the readme and tell me what it says",
    ]);
    match session {
        Some(dir) => {
            command.arg("--session-dir").arg(dir);
        }
        None => {
            command.arg("--no-session");
        }
    }
    let output = command
        .args(extra)
        .current_dir(directory)
        .env("B10X_HARNESS_TEST_KEY", "test-key")
        .output()
        .expect("the binary runs");
    Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn events(output: &Output) -> Vec<Value> {
    output
        .stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|error| panic!("{line}: {error}")))
        .collect()
}

/// Every top-level `started` record of a run.
fn started(events: &[Value]) -> Vec<&Value> {
    events
        .iter()
        .filter(|event| event["kind"] == "started")
        .collect()
}

/// Every `started` a delegate emitted, unwrapped from its `delegated` envelope.
fn delegated_started(events: &[Value]) -> Vec<&Value> {
    events
        .iter()
        .filter(|event| event["kind"] == "delegated" && event["event"]["kind"] == "started")
        .map(|event| &event["event"])
        .collect()
}

fn canonical(path: &Path) -> String {
    path.canonicalize()
        .expect("canonical")
        .to_str()
        .expect("this case's directory is UTF-8")
        .to_owned()
}

/// **The record must not attest a directory the run never worked in.**
///
/// A Linux directory name is bytes, and `--workspace` (here, the launch directory) accepts one
/// that is not UTF-8: the tools open it, the run completes. The record then has three honest
/// answers — the exact path, or *not stated* (`null` / absent), which is what the field's own
/// documentation says a reader concludes when the loop has nothing it can say. What it must not
/// do is write a lossy rendering: `U+FFFD` in place of the byte names a path that does not exist,
/// and the converter would pass it on as `session.started.cwd`, an attestation of a place nobody
/// saw the run work in — the very thing `metaharness.rs`' comment on `cwd` says it refuses to do.
///
/// Expected to fail: `prepare` writes `resolved_workspace(..).display().to_string()`.
#[cfg(unix)]
#[test]
fn a_workspace_whose_name_is_not_utf8_is_not_recorded_under_a_name_that_does_not_exist() {
    use std::os::unix::ffi::OsStrExt as _;

    let fixture = Fixture::start("text");
    let root = tempfile::tempdir().expect("a temporary directory");
    let workspace = root
        .path()
        .join(std::ffi::OsStr::from_bytes(b"ws-latin1-\xe9t\xe9"));
    fs::create_dir(&workspace).expect("a non-UTF-8 directory name is legal here");
    readme_in(&workspace);
    let expected = workspace.canonicalize().expect("canonical");

    let output = run_in(&fixture, &workspace, &[], None);
    assert_eq!(
        output.status,
        Some(0),
        "the run itself works in such a directory, so the record is reached: {}",
        output.stderr
    );
    let events = events(&output);
    let first = started(&events);
    let first = first.first().expect("a `started` record is still written");
    match &first["cwd"] {
        Value::Null => {}
        Value::String(cwd) => {
            let named = Path::new(cwd);
            assert_eq!(
                named.canonicalize().ok(),
                Some(expected.clone()),
                "`started.cwd` is {cwd:?}: a directory that does not exist, standing in for \
                 {expected:?}. Write the exact path, or say nothing; never a lossy rendering"
            );
        }
        other => panic!("`cwd` is a string or absent, not {other}"),
    }
}

/// A symlink is resolved, and the record and the transcript name one directory — the claim the
/// comment at the `with_cwd` call makes, checked against the session file the same run wrote.
#[cfg(unix)]
#[test]
fn a_symlinked_workspace_is_recorded_as_its_target_and_as_the_session_files_it() {
    let fixture = Fixture::start("text");
    let root = tempfile::tempdir().expect("a temporary directory");
    let real = root.path().join("real");
    fs::create_dir(&real).expect("create");
    readme_in(&real);
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    let elsewhere = tempfile::tempdir().expect("a temporary directory");
    let sessions = tempfile::tempdir().expect("a temporary directory");

    // Given by flag, as `link/./`, from a directory that is not the workspace at all.
    let spelled = link.join(".").join("");
    let output = run_in(
        &fixture,
        elsewhere.path(),
        &[std::ffi::OsStr::new("--workspace"), spelled.as_os_str()],
        Some(sessions.path()),
    );
    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let events = events(&output);
    let first = started(&events);
    let cwd = first.first().expect("a started record")["cwd"].clone();
    assert_eq!(
        cwd,
        Value::String(canonical(&real)),
        "the target, not the link"
    );
    assert_ne!(
        cwd,
        Value::String(canonical(elsewhere.path())),
        "the workspace, not the launch directory"
    );

    let session: Vec<PathBuf> = fs::read_dir(sessions.path())
        .expect("sessions")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    assert_eq!(session.len(), 1, "{session:?}");
    let filed: Value =
        serde_json::from_str(&fs::read_to_string(&session[0]).expect("readable")).expect("json");
    assert_eq!(
        filed["workspace"], cwd,
        "the record and the transcript name one directory"
    );
}

/// `chat` goes through the same `prepare`: every line is a run, and every run says where it is.
#[test]
fn every_chat_line_names_the_directory_it_works_in() {
    let fixture = Fixture::start("text");
    let workspace = tempfile::tempdir().expect("a temporary directory");
    readme_in(workspace.path());
    let mut child = Command::new(BINARY)
        .args([
            "chat",
            "--base-url",
            &fixture.base_url,
            "--model",
            "b10x-emulated",
            "--api-key-env",
            "B10X_HARNESS_TEST_KEY",
            "--no-session",
            "--json",
        ])
        .current_dir(workspace.path())
        .env("B10X_HARNESS_TEST_KEY", "test-key")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(b"first question\nsecond question\n")
        .expect("write");
    let output = child.wait_with_output().expect("the chat ends");
    let output = Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    };
    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let events = events(&output);
    let records = started(&events);
    assert_eq!(
        records.len(),
        2,
        "one `started` per line: {}",
        output.stdout
    );
    for record in records {
        assert_eq!(
            record["cwd"],
            Value::String(canonical(workspace.path())),
            "{record}"
        );
    }
}

/// A delegate runs over the parent's port, in the parent's directory, and its own `started`
/// (nested in `delegated`) says so — in order, and side by side over forked ports.
#[test]
fn a_delegate_and_a_forked_pair_name_their_parents_directory() {
    for scenario in ["delegate", "delegate-pair"] {
        let fixture = Fixture::start(scenario);
        let workspace = tempfile::tempdir().expect("a temporary directory");
        readme_in(workspace.path());
        let output = run_in(
            &fixture,
            workspace.path(),
            &[std::ffi::OsStr::new("--delegate")],
            None,
        );
        assert_eq!(output.status, Some(0), "{scenario}: {}", output.stderr);
        let events = events(&output);
        let children = delegated_started(&events);
        let expected = if scenario == "delegate" { 1 } else { 2 };
        assert_eq!(children.len(), expected, "{scenario}: {}", output.stdout);
        for child in children {
            assert_eq!(
                child["cwd"],
                Value::String(canonical(workspace.path())),
                "{scenario}: {child}"
            );
        }
    }
}
