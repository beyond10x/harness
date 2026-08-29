//! The shipped binary, against a real endpoint, over a real workspace.
//!
//! This is the composition the unit tests cannot reach: argument parsing, credential resolution,
//! the HTTP client, the loop, the read-only tools and the renderer, all at once. If this passes,
//! `b10x-harness run` works.

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
        Self::of("harness-responses", "fake_responses.py", scenario)
    }

    /// The same fixture, pointed at the second wire's emulator.
    fn messages(scenario: &str) -> Self {
        Self::of("harness-messages", "fake_messages.py", scenario)
    }

    fn of(crate_name: &str, script: &str, scenario: &str) -> Self {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(crate_name)
            .join("tests")
            .join("fixtures")
            .join(script);
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

/// A workspace with one readable file.
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut file = fs::File::create(dir.path().join("README.md")).expect("create");
    file.write_all(b"hello harness\n").expect("write");
    dir
}

struct Output {
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run(arguments: &[&str], workspace: &Path) -> Output {
    let output = Command::new(BINARY)
        .args(arguments)
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

fn run_against(fixture: &Fixture, extra: &[&str], workspace: &Path) -> Output {
    let mut arguments = vec![
        "run",
        "--base-url",
        &fixture.base_url,
        "--model",
        "b10x-emulated",
        "--api-key-env",
        "B10X_HARNESS_TEST_KEY",
        "--input",
        "read the readme and tell me what it says",
    ];
    arguments.extend_from_slice(extra);
    let output = Command::new(BINARY)
        .args(&arguments)
        .arg("--workspace")
        .arg(workspace)
        .env("B10X_HARNESS_TEST_KEY", "test-key")
        .output()
        .expect("the binary runs");
    Output {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

#[test]
fn the_binary_answers_and_puts_only_the_answer_on_stdout() {
    let fixture = Fixture::start("text");
    let workspace = workspace();
    let output = run_against(&fixture, &[], workspace.path());

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert_eq!(output.stdout.trim(), "provider emulation passed");
    assert!(
        output
            .stderr
            .contains("tool_search, tool_describe, tool_invoke"),
        "progress names the three verbs, whatever the catalogue holds: {}",
        output.stderr
    );
}

#[test]
fn the_binary_reads_a_real_file_through_a_real_tool_call() {
    let fixture = Fixture::start("tool");
    let workspace = workspace();
    let output = run_against(&fixture, &[], workspace.path());

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("→ tool_invoke"),
        "the call is reported: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("← ok"),
        "the result is reported: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("usage 42 in"),
        "reported usage is surfaced: {}",
        output.stderr
    );
}

#[test]
fn json_mode_emits_one_event_per_line() {
    let fixture = Fixture::start("tool");
    let workspace = workspace();
    let output = run_against(&fixture, &["--json"], workspace.path());

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let events: Vec<Value> = output
        .stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line is one event"))
        .collect();
    let kinds: Vec<&str> = events
        .iter()
        .filter_map(|event| event["kind"].as_str())
        .collect();
    assert_eq!(kinds.first(), Some(&"started"));
    assert_eq!(kinds.last(), Some(&"finished"));
    assert!(kinds.contains(&"tool-requested"), "{kinds:?}");
    assert!(kinds.contains(&"tool-completed"), "{kinds:?}");
    assert!(kinds.contains(&"usage"), "{kinds:?}");
    assert_eq!(
        events.last().expect("a terminal event")["stop"]["kind"],
        serde_json::json!("completed")
    );
}

#[test]
fn a_run_that_stops_without_an_answer_exits_distinctly_from_a_failure() {
    let fixture = Fixture::start("incomplete");
    let workspace = workspace();
    let output = run_against(&fixture, &[], workspace.path());

    assert_eq!(
        output.status,
        Some(2),
        "a named stop is neither success nor failure: {}",
        output.stderr
    );
    assert!(
        output.stderr.contains("ProviderIncomplete"),
        "{}",
        output.stderr
    );
}

#[test]
fn a_rejected_credential_fails_with_a_message_naming_the_cause() {
    let fixture = Fixture::start("unauthorized");
    let workspace = workspace();
    let output = run_against(&fixture, &[], workspace.path());

    assert_eq!(output.status, Some(1), "stderr: {}", output.stderr);
    assert!(
        output.stderr.to_lowercase().contains("unauthorized"),
        "{}",
        output.stderr
    );
}

#[test]
fn naming_no_credential_source_reaches_the_endpoint_unauthenticated() {
    // The declaration a gateway on this machine needs, and the one `--credentials none` becomes
    // when metaharness launches this loop. It is not a refusal: the request goes out with no
    // `authorization` header and the far end decides. Here nothing is listening, so what comes
    // back is a transport failure — proof the run got as far as the socket.
    let workspace = workspace();
    let output = run(
        &[
            "run",
            "--base-url",
            "http://127.0.0.1:1/v1",
            "--model",
            "m",
            "--input",
            "hi",
        ],
        workspace.path(),
    );
    assert_eq!(output.status, Some(1));
    assert!(
        output.stderr.contains("posting to"),
        "it tried, rather than refusing itself: {}",
        output.stderr
    );
    assert!(!output.stderr.contains("exactly one"), "{}", output.stderr);
}

#[test]
fn the_tools_subcommand_describes_the_toolset_without_an_endpoint() {
    let workspace = workspace();
    let output = run(&["tools"], workspace.path());

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    let described: Value = serde_json::from_str(&output.stdout).expect("valid JSON");
    let names: Vec<&str> = described["tools"]
        .as_array()
        .expect("a tool array")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(
        names,
        vec!["tool_search", "tool_describe", "tool_invoke"],
        "the model is offered three verbs, whatever the machine admits"
    );
    // ...and what stands behind them is the question a reader is actually asking.
    let entries: Vec<&str> = described["catalogue"]["tools"]
        .as_array()
        .expect("a catalogue")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(entries, vec!["file_read", "dir_list", "search"]);
    assert!(
        described["tools"]
            .as_array()
            .expect("a tool array")
            .iter()
            .all(|tool| tool["approval"] == serde_json::json!("not-required")),
        "the shipped toolset is read-only: {}",
        output.stdout
    );
}

#[test]
fn the_binary_drives_the_second_wire_and_calls_a_real_tool_through_it() {
    // Phase 3's exit criterion at the outermost layer: the shipped binary, one flag different,
    // against the other endpoint. Everything between the flag and the answer — the loop, the
    // catalogue, the tools, the renderer — is the same code, which is the whole claim the second
    // wire was built to test.
    let fixture = Fixture::messages("tool");
    let workspace = workspace();
    let output = run_against(
        &fixture,
        &["--wire", "anthropic-messages", "--yes"],
        workspace.path(),
    );

    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);
    assert_eq!(output.stdout.trim(), "The file says: hello harness");
}

#[test]
fn a_thinking_round_trip_completes_through_the_shipped_binary() {
    // The second wire's opaque item, all the way out and back through the real binary: the
    // `reasoning` scenario answers turn one with a `thinking` block and a tool call, and only
    // completes turn two if the block was replayed byte for byte at the head of the assistant
    // message. A wire that dropped it or reordered it ends here instead.
    let fixture = Fixture::messages("reasoning");
    let workspace = workspace();
    let output = run_against(
        &fixture,
        &["--wire", "anthropic-messages", "--yes", "--json"],
        workspace.path(),
    );
    assert_eq!(output.status, Some(0), "stderr: {}", output.stderr);

    let usage: Vec<Value> = output
        .stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event["kind"] == "usage")
        .collect();
    assert!(
        !usage.is_empty(),
        "no usage reached the record: {}",
        output.stdout
    );
    // 42 fresh plus 7 read from cache. This wire reports the two disjointly and the projection
    // sums them, because the neutral `input_tokens` is the whole and cached is a part of it — a
    // run that reported 42 here would price every cached turn low.
    assert_eq!(usage[0]["input_tokens"], 49, "{}", usage[0]);
    assert_eq!(
        usage[0]["cached_input_tokens"],
        Value::from(7),
        "{}",
        usage[0]
    );
}
