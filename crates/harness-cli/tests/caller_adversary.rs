//! Adversarial cases against `--approve caller` (atlas ADR 0104), written from outside the unit.
//!
//! The decision parser is driven through its public seam (`caller::Caller::over`, `caller::judge`)
//! with inputs the unit's own tests do not name; the binary is driven end to end where only the
//! composition can answer (an interrupt while a decision is pending, the spellings refused before
//! any turn).

use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use b10x_harness_cli::caller::{Caller, MAX_DECISION_LINE_BYTES, judge};
use harness_loop::{ApprovalDecision, ApprovalPort};
use harness_wire::{Approval, CallId, Envelope, ToolCall, ToolName, ToolSpec};
use serde_json::{Value, json};

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");

fn spec(name: &str) -> ToolSpec {
    ToolSpec {
        name: ToolName::new(name).expect("a valid tool name"),
        description: "entry under test".to_owned(),
        input_schema: json!({"type": "object"}),
        approval: Approval::Required,
        envelope: Envelope::default(),
    }
}

fn call(id: &str, name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        call_id: CallId::new(id).expect("a valid call id"),
        name: ToolName::new(name).expect("a valid tool name"),
        arguments,
    }
}

fn write_call(id: &str) -> ToolCall {
    call(id, "file_write", json!({"path": "a.txt", "text": "hi"}))
}

fn caller(input: Vec<u8>) -> Caller<Cursor<Vec<u8>>, Cursor<Vec<u8>>> {
    Caller::over(Cursor::new(input), Cursor::new(Vec::new()))
}

fn denial(decision: ApprovalDecision) -> String {
    match decision {
        ApprovalDecision::Denied { reason } => reason,
        other => panic!("expected a denial, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// What the decision line can and cannot approve.
// ---------------------------------------------------------------------------------------------

/// A duplicated key cannot smuggle the asked-about `call_id` past a first one naming another call,
/// nor an `allow` past a first `deny`: serde's last-wins would otherwise make the line mean
/// whatever its final occurrence says.
#[test]
fn a_duplicated_key_never_approves() {
    for line in [
        r#"{"call_id":"call-2","call_id":"call-1","decision":"allow"}"#,
        r#"{"call_id":"call-1","call_id":"call-1","decision":"allow"}"#,
        r#"{"call_id":"call-1","decision":"deny","decision":"allow"}"#,
        r#"{"call_id":"call-1","decision":"deny","reason":"no","decision":"allow"}"#,
    ] {
        let decision = judge(line.as_bytes(), "call-1", "file_write");
        assert!(!decision.is_approved(), "{line} approved");
    }
}

/// A call id that differs only by surrounding whitespace, case, an escape that decodes to a
/// different byte or a fullwidth look-alike is another call.
#[test]
fn a_call_id_that_only_looks_like_the_asked_one_never_approves() {
    for id in [
        " call-1",
        "call-1 ",
        "call-1\n",
        "call-1\u{0}",
        "CALL-1",
        "call\u{2011}1",
        "\u{ff43}all-1",
        "call-1\u{200b}",
    ] {
        let line = json!({"call_id": id, "decision": "allow"}).to_string();
        let reason = denial(judge(line.as_bytes(), "call-1", "file_write"));
        assert!(reason.contains("another call"), "{id:?}: {reason}");
    }
}

/// A byte-order mark, a trailing second object, and non-UTF-8 inside a string are not decisions.
#[test]
fn a_bom_trailing_object_or_invalid_utf8_never_approves() {
    let allow = br#"{"call_id":"call-1","decision":"allow"}"#;
    let mut bom = b"\xef\xbb\xbf".to_vec();
    bom.extend_from_slice(allow);
    let mut trailing = allow.to_vec();
    trailing.extend_from_slice(br#" {"call_id":"call-1","decision":"allow"}"#);
    let invalid = b"{\"call_id\":\"call-1\",\"decision\":\"allow\",\"reason\":\"\xff\"}".to_vec();
    for line in [bom, trailing, invalid] {
        let decision = judge(&line, "call-1", "file_write");
        assert!(
            !decision.is_approved(),
            "{:?} approved",
            String::from_utf8_lossy(&line)
        );
    }
}

/// An `allow` written early for a later call is spent on the call it was read for, and does not
/// survive to approve the later one.
#[test]
fn an_early_allow_for_a_later_call_approves_neither() {
    let mut approver = caller(
        b"{\"call_id\":\"call-2\",\"decision\":\"allow\"}\n\
          {\"call_id\":\"call-1\",\"decision\":\"allow\"}\n"
            .to_vec(),
    );
    let spec = spec("file_write");
    assert!(!approver.decide(&write_call("call-1"), &spec).is_approved());
    assert!(!approver.decide(&write_call("call-2"), &spec).is_approved());
}

/// A line the caller wrote for one call must not approve a different call that happens to carry
/// the same `call_id`. The decision names only the id, so a caller that answered one question
/// twice — a retry after a slow write, a second `allow` on the same event — has left a line in
/// the pipe that approves whatever is asked next under that id: here a `run` it never saw.
#[test]
fn a_stale_allow_does_not_approve_a_different_call_reusing_the_id() {
    let mut approver = caller(
        b"{\"call_id\":\"call_0\",\"decision\":\"allow\"}\n\
          {\"call_id\":\"call_0\",\"decision\":\"allow\"}\n"
            .to_vec(),
    );
    assert!(
        approver
            .decide(&write_call("call_0"), &spec("file_write"))
            .is_approved(),
        "the answer the caller gave"
    );
    let unseen = call("call_0", "run", json!({"argv": ["rm", "-rf", "."]}));
    assert!(
        !approver.decide(&unseen, &spec("run")).is_approved(),
        "a line written for `file_write` approved a `run` the caller was never shown"
    );
}

// ---------------------------------------------------------------------------------------------
// The bound, read while reading.
// ---------------------------------------------------------------------------------------------

/// One byte past the bound — the newline itself the byte over — is refused, and the next line is
/// still read as the next answer.
#[test]
fn a_line_one_byte_past_the_bound_is_refused_and_resynchronises() {
    let head = br#"{"call_id":"call-1","decision":"deny","reason":""#;
    let tail = b"\"}\n";
    let filler = MAX_DECISION_LINE_BYTES + 1 - head.len() - tail.len();
    let mut input = head.to_vec();
    input.extend(std::iter::repeat_n(b'x', filler));
    input.extend_from_slice(tail);
    assert_eq!(input.len(), MAX_DECISION_LINE_BYTES + 1);
    input.extend_from_slice(b"{\"call_id\":\"call-2\",\"decision\":\"allow\"}\n");
    let mut approver = caller(input);
    let spec = spec("file_write");
    let reason = denial(approver.decide(&write_call("call-1"), &spec));
    assert!(
        reason.contains(&MAX_DECISION_LINE_BYTES.to_string()),
        "{reason}"
    );
    assert!(approver.decide(&write_call("call-2"), &spec).is_approved());
}

/// A reader delivering one enormous line in small reads is consumed to its newline without the
/// approver keeping it, and an `allow` buried at its tail is never read as an answer.
#[test]
fn an_endless_line_is_drained_not_held_and_its_tail_is_not_an_answer() {
    struct Endless {
        left: usize,
        tail: Vec<u8>,
    }
    impl Read for Endless {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.left > 0 {
                let n = buffer.len().min(self.left).min(4096);
                buffer[..n].fill(b'x');
                self.left -= n;
                return Ok(n);
            }
            let n = buffer.len().min(self.tail.len());
            buffer[..n].copy_from_slice(&self.tail[..n]);
            self.tail.drain(..n);
            Ok(n)
        }
    }
    let reader = Endless {
        left: 16 * MAX_DECISION_LINE_BYTES,
        tail: b"{\"call_id\":\"call-1\",\"decision\":\"allow\"}\n\
                {\"call_id\":\"call-2\",\"decision\":\"allow\"}\n"
            .to_vec(),
    };
    let mut approver = Caller::over(BufReader::new(reader), Cursor::new(Vec::new()));
    let spec = spec("file_write");
    assert!(!approver.decide(&write_call("call-1"), &spec).is_approved());
    assert!(approver.decide(&write_call("call-2"), &spec).is_approved());
}

/// An empty line is one answer, and it is a refusal rather than skipped: skipping would let the
/// caller's next line answer a question it was not written for.
#[test]
fn an_empty_line_is_a_refusal_of_the_call_it_was_read_for() {
    let mut approver = caller(b"\n{\"call_id\":\"call-1\",\"decision\":\"allow\"}\n".to_vec());
    let spec = spec("file_write");
    assert!(!approver.decide(&write_call("call-1"), &spec).is_approved());
}

// ---------------------------------------------------------------------------------------------
// The binary.
// ---------------------------------------------------------------------------------------------

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
            .expect("the fixture starts");
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

fn workspace(name: &str) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("a temporary directory");
    let workspace = root.path().join(name);
    std::fs::create_dir(&workspace).expect("create the workspace directory");
    (root, workspace)
}

fn base<'a>(fixture: &'a Fixture, workspace: &'a Path) -> Vec<&'a str> {
    vec![
        "run",
        "--base-url",
        &fixture.base_url,
        "--model",
        "b10x-emulated",
        "--api-key-env",
        "B10X_HARNESS_TEST_KEY",
        "--no-session",
        "--input",
        "write the note",
        "--substrate-embedded",
        "--workspace",
        workspace.to_str().expect("a utf-8 workspace path"),
    ]
}

/// `--yes` beside `caller` is refused whatever the order and however `--approve` is spelled.
#[test]
fn yes_beside_caller_is_refused_in_every_spelling() {
    let fixture = Fixture::start("flat-write");
    let (_root, workspace) = workspace("ws_adv_yes");
    for extra in [
        &["--yes", "--json", "--approve", "caller"][..],
        &["--json", "--approve=caller", "--yes"][..],
        &["--approve=caller", "--yes", "--json"][..],
    ] {
        let mut arguments = base(&fixture, &workspace);
        arguments.extend_from_slice(extra);
        let output = Command::new(BINARY)
            .args(&arguments)
            .env("B10X_HARNESS_TEST_KEY", "test-key")
            .stdin(Stdio::null())
            .output()
            .expect("the binary runs");
        assert_eq!(output.status.code(), Some(1), "{extra:?}");
        assert!(!workspace.join("note.md").exists(), "{extra:?}");
    }
}

/// `app-server` has no `--approve`: bridge mode's approver is the client's, and `caller` must not
/// parse there.
#[test]
fn app_server_does_not_accept_approve_caller() {
    let output = Command::new(BINARY)
        .args([
            "app-server",
            "--base-url",
            "http://127.0.0.1:1/v1",
            "--model",
            "m",
            "--approve",
            "caller",
        ])
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unexpected argument '--approve'"),
        "{output:?}"
    );
}

/// An interrupt ends a run that is waiting on its caller's decision.
///
/// `install_interrupt` turns Ctrl-C into the loop's cancellation token, so a run interrupted at
/// any other point stops; one interrupted while the approver blocks on stdin keeps blocking until
/// the caller writes or closes the pipe, so the process that launched it cannot stop it the way
/// it stops every other run.
#[test]
fn an_interrupt_ends_a_run_waiting_on_its_caller() {
    let fixture = Fixture::start("flat-write");
    let (_root, workspace) = workspace("ws_adv_interrupt");
    let mut arguments = base(&fixture, &workspace);
    arguments.extend(["--json", "--approve", "caller"]);
    let mut child = Command::new(BINARY)
        .args(&arguments)
        .env("B10X_HARNESS_TEST_KEY", "test-key")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    let stdin = child.stdin.take().expect("piped stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
    loop {
        let mut line = String::new();
        let read = stdout.read_line(&mut line).expect("read the record");
        assert!(read > 0, "the record ended before asking");
        let event: Value = serde_json::from_str(&line).expect("a JSON event");
        if event["kind"] == "approval-required" {
            break;
        }
    }
    let killed = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(killed.success());
    let deadline = Instant::now() + Duration::from_secs(10);
    let ended = loop {
        if let Some(status) = child.try_wait().expect("poll the run") {
            break Some(status);
        }
        if Instant::now() > deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if ended.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let mut keep = stdin;
    let _ = keep.flush();
    drop(keep);
    assert!(
        ended.is_some(),
        "10 s after SIGINT the run was still blocked reading its caller's decision"
    );
    assert!(!workspace.join("note.md").exists());
}
