//! Adversary cases for `story:mcp-call-stops-when-the-loop-is-cancelled`, pass 1.
//!
//! The unit's own cases drive `McpTools` built directly with a token. These add what they leave
//! unsaid: the outcome the model is handed for a cancelled call (failed, not a refusal, and saying
//! whether the request was sent), that a call after the cancel never reaches the server, that the
//! caller's remaining time still bounds a call when the token never fires, and that the shipped
//! binary's Ctrl-C reaches an MCP call in flight, which is the only place the run's token is
//! handed to `with_mcp`.

use std::fs;
use std::io::{BufRead as _, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use harness_loop::LoopCancel;
use harness_mcp::McpTools;
use harness_wire::{CallId, ToolCall, ToolName, ToolOutcome, ToolPort as _};
use serde_json::{Value, json};

mod common;

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

/// The bound the story's acceptance names for a cancelled call.
const PROMPT: Duration = Duration::from_secs(5);

/// The longest a case waits for a fixture to see the request it is about to abandon.
const SETUP: Duration = Duration::from_secs(20);

const STORY: &str = "story:mcp-call-stops-when-the-loop-is-cancelled";

/// `tests/fixtures/adversary_silent_mcp.rs`, compiled once into this test binary's own name.
fn silent_mcp_binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/adversary_silent_mcp.rs");
            let binary =
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("adversary-1104-u2-silent-mcp");
            let output = Command::new("rustc")
                .args(["--edition=2024", "-C", "debuginfo=0", "-o"])
                .arg(&binary)
                .arg(&source)
                .output()
                .expect("the workspace Rust compiler runs");
            assert!(
                output.status.success(),
                "the silent endpoint compiles: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            binary
        })
        .as_path()
}

/// A reviewed registry and profile over the silent endpoint, and the marker it writes on a call.
struct SilentMcp {
    _directory: tempfile::TempDir,
    registry: PathBuf,
    profile: PathBuf,
    marker: PathBuf,
}

impl SilentMcp {
    fn new() -> Self {
        const CONNECTION: &str = "stdio_fixture";
        let directory = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))
            .expect("a scratch directory in the target");
        let marker = directory.path().join("called");
        let registry_document = format!(
            "[connections.{CONNECTION}]\ntransport = \"stdio\"\nprogram = {:?}\nargs = [{:?}]\ncwd = {:?}\n",
            silent_mcp_binary().to_string_lossy(),
            marker.to_string_lossy(),
            env!("CARGO_MANIFEST_DIR"),
        );
        let registry = directory.path().join("mcp.toml");
        fs::write(&registry, &registry_document).expect("write the MCP registry");
        let registry_sha256 = b10x_mcp_config::Registry::parse(&registry_document)
            .expect("the fixture registry is valid")
            .sha256()
            .expect("hash the fixture registry");
        let snapshot_sha256 = b10x_mcp_testkit::synthetic_snapshot(CONNECTION)
            .expect("build the fixture snapshot")
            .sha256;
        let profile_document = format!(
            r#"connection = "{CONNECTION}"
registry-sha256 = "{registry_sha256}"
snapshot-sha256 = "{snapshot_sha256}"

[[tools]]
remote = "read_issue"
publish = "mcp_fixture_read_issue"
description = "Read one issue from the controlled MCP endpoint"
subjects = [{{ kind = "process", value = "b10x-harness-mcp-endpoint" }}]

[tools.envelope]
effects = ["read"]
risk = "low"
idempotency = "idempotent"
access = []
"#
        );
        let profile = directory.path().join("profile.toml");
        fs::write(&profile, profile_document).expect("write the MCP profile");
        Self {
            _directory: directory,
            registry,
            profile,
            marker,
        }
    }

    fn tools(&self, cancel: &LoopCancel) -> McpTools {
        McpTools::prepare_with_cancel(&self.profile, Some(&self.registry), cancel)
            .expect("the silent endpoint is prepared")
    }
}

fn read_issue() -> ToolCall {
    ToolCall {
        call_id: CallId::new("call_mcp_1").expect("a call id"),
        name: ToolName::new("mcp_fixture_read_issue").expect("a tool name"),
        arguments: json!({"id": "ISSUE-7"}),
    }
}

fn text(outcome: &ToolOutcome) -> &str {
    outcome
        .output
        .as_str()
        .unwrap_or_else(|| panic!("a cancelled call answers with text, not {outcome:?}"))
}

/// Cancels `cancel` once the silent endpoint has seen the call, and says when.
fn cancel_once_called(marker: &Path, cancel: &LoopCancel) -> thread::JoinHandle<Instant> {
    let (marker, cancel) = (marker.to_path_buf(), cancel.clone());
    thread::spawn(move || {
        let start = Instant::now();
        while !marker.exists() {
            assert!(
                start.elapsed() < SETUP,
                "the MCP call reached the endpoint within {SETUP:?}"
            );
            thread::sleep(Duration::from_millis(10));
        }
        cancel.cancel();
        Instant::now()
    })
}

// ---------------------------------------------------------------------------------------------
// `McpTools` on its own.
// ---------------------------------------------------------------------------------------------

/// A token already cancelled: the call is never sent, and the model is told exactly that.
#[test]
fn adversary_1104_u2_a_call_after_the_cancel_is_never_sent() {
    let mcp = SilentMcp::new();
    let cancel = LoopCancel::new();
    let mut tools = mcp.tools(&cancel);
    cancel.cancel();

    let started = Instant::now();
    let outcome = tools.call(&read_issue());
    let waited = started.elapsed();

    assert!(
        waited < PROMPT,
        "a call under a cancelled token returned after {waited:?}, not within {PROMPT:?}; {STORY}"
    );
    assert!(
        outcome.failed && outcome.refusal.is_none(),
        "a call under a cancelled token is a failed outcome, not a refusal: {outcome:?}; {STORY}"
    );
    assert_eq!(
        text(&outcome),
        "MCP call `read_issue` was not sent: the run was cancelled",
        "the model is told the call was never sent; {STORY}"
    );
    // The endpoint writes the marker the moment `tools/call` reaches it.
    thread::sleep(Duration::from_millis(200));
    assert!(
        !mcp.marker.exists(),
        "a call made after the cancel reached the MCP server; {STORY}"
    );
}

/// Cancelled while the server holds the call: a failed outcome naming the abandoned request.
#[test]
fn adversary_1104_u2_a_call_cancelled_in_flight_is_a_failed_outcome_saying_it_was_sent() {
    let mcp = SilentMcp::new();
    let cancel = LoopCancel::new();
    let mut tools = mcp.tools(&cancel);
    let watcher = cancel_once_called(&mcp.marker, &cancel);

    let outcome = tools.call(&read_issue());
    let ended = Instant::now();
    let cancelled_at = watcher.join().expect("the watcher cancelled the call");
    let waited = ended.saturating_duration_since(cancelled_at);

    assert!(
        waited < PROMPT,
        "the call returned {waited:?} after the cancel, not within {PROMPT:?}; {STORY}"
    );
    assert!(
        outcome.failed && outcome.refusal.is_none(),
        "a call cancelled in flight is a failed outcome, not a refusal: {outcome:?}; {STORY}"
    );
    assert_eq!(
        text(&outcome),
        "MCP call `read_issue` was cancelled with the run before it answered; the server was \
         not told and may still complete it",
        "the model is told the call was sent and abandoned; {STORY}"
    );
}

/// A token that never fires: the caller's remaining time still ends the call, as a client
/// failure rather than as a cancel.
#[test]
fn adversary_1104_u2_the_remaining_time_still_bounds_a_call_the_token_never_cancels() {
    let mcp = SilentMcp::new();
    let cancel = LoopCancel::new();
    let mut tools = mcp.tools(&cancel);

    let started = Instant::now();
    let outcome = tools.call_within(&read_issue(), Some(Duration::from_millis(500)));
    let waited = started.elapsed();

    assert!(
        waited < PROMPT,
        "a call with 500 ms remaining returned after {waited:?}; {STORY}"
    );
    assert!(
        mcp.marker.exists(),
        "the call reached the endpoint before it timed out"
    );
    assert!(
        outcome.failed && outcome.refusal.is_none(),
        "a timed-out call is a failed outcome: {outcome:?}"
    );
    let said = text(&outcome);
    assert!(
        said.starts_with("MCP call failed:") && !said.contains("cancelled"),
        "a call ended by its deadline is not reported as a cancel: {said:?}; {STORY}"
    );
}

// ---------------------------------------------------------------------------------------------
// The shipped binary: Ctrl-C during an MCP call.
// ---------------------------------------------------------------------------------------------

struct Provider(Child);

impl Drop for Provider {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The controlled Responses endpoint whose first answer calls `mcp_fixture_read_issue`.
fn provider() -> (Provider, String) {
    let mut child = Command::new(common::controlled_fixture_binary())
        .arg("provider")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the Rust controlled endpoint starts");
    let mut line = String::new();
    BufReader::new(child.stdout.as_mut().expect("piped stdout"))
        .read_line(&mut line)
        .expect("the controlled endpoint announces its address");
    let ready: Value = serde_json::from_str(&line).expect("the announcement is JSON");
    let url = ready["url"].as_str().expect("an endpoint URL").to_owned();
    (Provider(child), url)
}

/// `run` holds the run's token in `with_mcp`; SIGINT, which cancels that token, must end an MCP
/// call in flight within the bound, not at the MCP client's 30 s request timeout.
#[test]
fn adversary_1104_u2_ctrl_c_ends_a_cli_run_waiting_on_an_mcp_call() {
    let mcp = SilentMcp::new();
    let (_provider, base_url) = provider();
    let workspace = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a workspace");
    fs::write(workspace.path().join("README.md"), "hello harness\n").expect("write the readme");
    let state = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a state directory");

    let mut child = Command::new(BINARY)
        .args([
            "run",
            "--base-url",
            &base_url,
            "--model",
            "b10x-emulated",
            "--api-key-env",
            "B10X_HARNESS_TEST_KEY",
            "--no-session",
            "--json",
            "--input",
            "read controlled issue ISSUE-7",
            "--mcp-profile",
            mcp.profile.to_str().expect("UTF-8 profile path"),
            "--mcp-registry",
            mcp.registry.to_str().expect("UTF-8 registry path"),
        ])
        .arg("--workspace")
        .arg(workspace.path())
        .env("B10X_HARNESS_TEST_KEY", "test-key")
        .env("XDG_STATE_HOME", state.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs with an MCP profile");

    let start = Instant::now();
    while !mcp.marker.exists() {
        if start.elapsed() > SETUP || child.try_wait().expect("poll the run").is_some() {
            let _ = child.kill();
            let output = child.wait_with_output().expect("collect the run");
            panic!(
                "the run never reached the MCP call within {SETUP:?}: stderr {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
    let killed = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(killed.success(), "SIGINT was delivered");
    let interrupted = Instant::now();
    let ended = loop {
        if let Some(status) = child.try_wait().expect("poll the run") {
            break Some(status);
        }
        if interrupted.elapsed() > PROMPT {
            break None;
        }
        thread::sleep(Duration::from_millis(20));
    };
    let waited = interrupted.elapsed();
    if ended.is_none() {
        let _ = child.kill();
    }
    let output = child.wait_with_output().expect("collect the run");
    assert!(
        ended.is_some(),
        "{waited:?} after SIGINT the run was still waiting on its MCP call, not ended within \
         {PROMPT:?}; {STORY}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let events: Vec<Value> = stdout
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    assert!(
        events.iter().any(|event| {
            event["kind"] == "tool-completed"
                && event["call_id"] == "call_b10x_001"
                && event["failed"] == true
        }),
        "the cancelled MCP call still answered the model's call as failed: {events:?}; {STORY}"
    );
}
