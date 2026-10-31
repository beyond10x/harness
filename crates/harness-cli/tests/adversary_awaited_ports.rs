//! Adversary cases for story `real-ports-run-under-an-awaited-loop`.
//!
//! The story's own cases drive each shipped port to a clean finish. These drop a run's future while
//! the real port is in the middle of its work, as an async program does on a timeout or a
//! `select!`, and check that the task still shuts down promptly and cleanly, and that the caller's
//! runtime keeps running while the task's thread is busy.

use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead as _, BufReader, Read as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use harness_catalogue::{Catalogue, Flat};
use harness_llm::{Protocol, Target};
use harness_loop::{ApproveAll, LoopCancel, LoopConfig, LoopPorts, LoopTask, VecLoopSink};
use harness_mcp::McpTools;
use harness_substrate::{Backend as _, ConfinedOperations, Embedded};
use harness_wire::{
    CallId, Item, ModelPort, StopReason, StreamSink, ToolCall, ToolName, ToolOutcome, ToolPort,
    ToolSpec, TurnOutcome, TurnRequest, WireError, WireId,
};
use serde_json::json;

/// How long a task may take to shut down once the future of its running command was dropped.
///
/// The model port polls the token every 10 ms; five seconds is generous for any port that
/// honours it.
const PROMPT: Duration = Duration::from_secs(5);

/// The longest these cases wait for a fixture to see the request they are about to abandon.
const SETUP: Duration = Duration::from_secs(20);

fn config() -> LoopConfig {
    LoopConfig::new("b10x-emulated", "Answer briefly.")
}

fn scratch() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("a scratch directory in the target")
}

async fn until(what: &str, mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(start.elapsed() < SETUP, "{what} within {SETUP:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

// ---------------------------------------------------------------------------------------------
// Ports and fixtures.
// ---------------------------------------------------------------------------------------------

struct Scripted {
    wire: WireId,
    script: VecDeque<TurnOutcome>,
}

impl Scripted {
    fn calling(call: ToolCall) -> Self {
        Self {
            wire: WireId::new("fixture").expect("a wire id"),
            script: VecDeque::from([
                TurnOutcome {
                    stop_reason: StopReason::ToolCalls,
                    items: vec![Item::ToolCall(call)],
                    usage: None,
                },
                TurnOutcome {
                    stop_reason: StopReason::EndTurn,
                    items: vec![Item::assistant("done")],
                    usage: None,
                },
            ]),
        }
    }
}

impl ModelPort for Scripted {
    fn wire(&self) -> &WireId {
        &self.wire
    }

    fn turn(&mut self, _: &TurnRequest, _: &mut dyn StreamSink) -> Result<TurnOutcome, WireError> {
        self.script
            .pop_front()
            .ok_or_else(|| WireError::protocol("the script is exhausted"))
    }
}

struct NoTools;

impl ToolPort for NoTools {
    fn specs(&self) -> &[ToolSpec] {
        &[]
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        ToolOutcome::failed(format!("no tool `{}` in this run", call.name))
    }
}

fn call(id: &str, name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        call_id: CallId::new(id).expect("a call id"),
        name: ToolName::new(name).expect("a tool name"),
        arguments,
    }
}

/// `tests/fixtures/adversary_silent_mcp.rs`, compiled once into this package's target scratch.
fn silent_mcp_binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY
        .get_or_init(|| {
            let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/adversary_silent_mcp.rs");
            let binary = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("adversary-silent-mcp");
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
        let directory = scratch();
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

    fn spawn(&self) -> LoopTask {
        let (profile, registry) = (self.profile.clone(), self.registry.clone());
        LoopTask::spawn(config(), move |_: &LoopCancel| {
            let tools = McpTools::prepare(&profile, Some(&registry))?;
            Ok::<_, String>(LoopPorts::new(
                Scripted::calling(call(
                    "call_mcp_1",
                    "mcp_fixture_read_issue",
                    json!({"id": "ISSUE-7"}),
                )),
                tools,
                ApproveAll,
            ))
        })
    }
}

/// A Responses endpoint that reads one request, says so, and never answers it.
fn silent_responses_endpoint() -> (String, mpsc::Receiver<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the silent endpoint");
    let address = listener
        .local_addr()
        .expect("the silent endpoint's address");
    let (sender, received) = mpsc::channel();
    thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        let mut length = 0;
        let _ = reader.read_line(&mut line);
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.trim_end().split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0; length];
        let _ = reader.read_exact(&mut body);
        let _ = sender.send(());
        // Hold the connection open, unanswered, until the client closes it.
        let mut rest = Vec::new();
        let _ = reader.read_to_end(&mut rest);
    });
    (format!("http://{address}/v1"), received)
}

// ---------------------------------------------------------------------------------------------
// A run's future dropped while the MCP tool call is in flight.
// ---------------------------------------------------------------------------------------------

/// The MCP client's default request timeout (`b10x-mcp-types`), which today bounds how long a
/// cancelled task stays blocked in an MCP call, plus a margin for a loaded host.
const MCP_REQUEST_TIMEOUT_AND_MARGIN: Duration = Duration::from_secs(45);

/// Asserts today's state of an open defect: `McpTools` never sees the task's cancel token, so a
/// run dropped mid-MCP-call keeps the task thread blocked until the MCP request timeout (30 s).
/// When `story:mcp-call-stops-when-the-loop-is-cancelled` lands, replace both assertions with
/// `shutdown()` resolving `Ok` within `PROMPT`.
async fn mcp_shutdown_waits_for_the_request_timeout_after_a_dropped_call() {
    let mcp = SilentMcp::new();
    let task = mcp.spawn();
    let mut run = task.run_in(Vec::new(), "Read ISSUE-7.", VecLoopSink::new());
    until("the MCP call reached the endpoint", || mcp.marker.exists()).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(200), &mut run)
            .await
            .is_err(),
        "the run is still waiting on the silent tool"
    );
    drop(run);

    let started = Instant::now();
    let mut shutdown = task.shutdown();
    assert!(
        tokio::time::timeout(PROMPT, &mut shutdown).await.is_err(),
        "shutdown resolved within {PROMPT:?} of a run dropped mid-MCP-call: the open defect of \
         story:mcp-call-stops-when-the-loop-is-cancelled looks fixed; make this case assert the \
         prompt shutdown that story asks for"
    );
    let shut = tokio::time::timeout(MCP_REQUEST_TIMEOUT_AND_MARGIN, shutdown).await;
    assert_eq!(
        shut,
        Ok(Ok(())),
        "the task shut down within {MCP_REQUEST_TIMEOUT_AND_MARGIN:?} of the run's future being \
         dropped mid-MCP-call (waited {:?}); story:mcp-call-stops-when-the-loop-is-cancelled",
        started.elapsed()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn adversary_mcp_shutdown_waits_for_the_request_timeout_after_a_dropped_call_current_thread()
{
    mcp_shutdown_waits_for_the_request_timeout_after_a_dropped_call().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adversary_mcp_shutdown_waits_for_the_request_timeout_after_a_dropped_call_multi_thread() {
    mcp_shutdown_waits_for_the_request_timeout_after_a_dropped_call().await;
}

// ---------------------------------------------------------------------------------------------
// A run's future dropped while the model's request is in flight.
// ---------------------------------------------------------------------------------------------

async fn llm_shutdown_is_prompt_after_a_dropped_turn() {
    let (base_url, received) = silent_responses_endpoint();
    let task = LoopTask::spawn(config(), move |cancel: &LoopCancel| {
        let model = harness_llm::client(
            &Target {
                protocol: Protocol::Responses,
                base_url: &base_url,
                model: "b10x-emulated",
                context_window: 128_000,
                max_output_tokens: None,
            },
            None,
            cancel,
        )?;
        Ok::<_, String>(LoopPorts::boxed(
            model,
            Box::new(NoTools),
            Box::new(ApproveAll),
        ))
    });
    let mut run = task.run("Say anything.", VecLoopSink::new());
    until("the model request reached the endpoint", || {
        received.try_recv().is_ok()
    })
    .await;
    assert!(
        tokio::time::timeout(Duration::from_millis(200), &mut run)
            .await
            .is_err(),
        "the run is still waiting on the silent endpoint"
    );
    drop(run);

    let started = Instant::now();
    let shut = tokio::time::timeout(PROMPT, task.shutdown()).await;
    assert_eq!(
        shut,
        Ok(Ok(())),
        "the task shut down within {PROMPT:?} of the run's future being dropped mid-turn \
         (waited {:?})",
        started.elapsed()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn adversary_llm_shutdown_is_prompt_after_a_dropped_turn_current_thread() {
    llm_shutdown_is_prompt_after_a_dropped_turn().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adversary_llm_shutdown_is_prompt_after_a_dropped_turn_multi_thread() {
    llm_shutdown_is_prompt_after_a_dropped_turn().await;
}

// ---------------------------------------------------------------------------------------------
// The caller's runtime keeps running while the task's thread is blocked in a real port.
// ---------------------------------------------------------------------------------------------

async fn caller_runtime_progresses_during_a_blocked_mcp_call() {
    let mcp = SilentMcp::new();
    let task = mcp.spawn();
    let ticks = Arc::new(AtomicU64::new(0));
    let ticker = {
        let ticks = Arc::clone(&ticks);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(10)).await;
                ticks.fetch_add(1, Ordering::Relaxed);
            }
        })
    };
    let mut run = task.run_in(Vec::new(), "Read ISSUE-7.", VecLoopSink::new());
    until("the MCP call reached the endpoint", || mcp.marker.exists()).await;
    let before = ticks.load(Ordering::Relaxed);
    let waited = tokio::time::timeout(Duration::from_secs(1), &mut run).await;
    let during = ticks.load(Ordering::Relaxed) - before;
    assert!(waited.is_err(), "the silent tool is still blocking the run");
    assert!(
        during >= 20,
        "a concurrent task ticked {during} times in the second the run was blocked in MCP"
    );
    ticker.abort();
    drop(run);
    drop(task);
}

#[tokio::test(flavor = "current_thread")]
async fn adversary_caller_runtime_progresses_during_a_blocked_mcp_call_current_thread() {
    caller_runtime_progresses_during_a_blocked_mcp_call().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn adversary_caller_runtime_progresses_during_a_blocked_mcp_call_one_worker() {
    caller_runtime_progresses_during_a_blocked_mcp_call().await;
}

// ---------------------------------------------------------------------------------------------
// The embedded backend: runs dropped at varying instants, each task shut down cleanly.
// ---------------------------------------------------------------------------------------------

async fn embedded_shutdown_is_clean_after_runs_dropped_at_any_instant() {
    for step in 0_u64..16 {
        let root = scratch();
        let path = root.path().to_owned();
        let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
            let embedded = Embedded::open(&path, None).map_err(|error| error.to_string())?;
            let facts = embedded.machine().map_err(|error| error.to_string())?;
            let workspace = embedded
                .workspace_create(600_000)
                .map_err(|error| error.to_string())?;
            embedded
                .file_write(&workspace, "note.txt", "dropped mid-run\n")
                .map_err(|error| error.to_string())?;
            Ok::<_, String>(LoopPorts::new(
                Scripted::calling(call(
                    "call_file_1",
                    "file_read",
                    json!({"path": "note.txt"}),
                )),
                Flat::new(Catalogue::of(ConfinedOperations::new(
                    embedded,
                    &facts,
                    workspace,
                    Vec::new(),
                ))),
                ApproveAll,
            ))
        });
        let ran = tokio::time::timeout(
            Duration::from_micros(step * 400),
            task.run_in(Vec::new(), "Read note.txt.", VecLoopSink::new()),
        )
        .await;
        if let Ok(answer) = ran {
            let ran = answer.expect("the task answered the run");
            assert!(
                ran.result.is_ok(),
                "step {step}: the run ended: {:?}",
                ran.result
            );
        }
        let shut = tokio::time::timeout(PROMPT, task.shutdown()).await;
        assert!(
            matches!(shut, Ok(Ok(()))),
            "step {step}: the embedded task shut down cleanly after its run was dropped: {shut:?}"
        );
        drop(root);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn adversary_embedded_shutdown_is_clean_after_runs_dropped_at_any_instant_current_thread() {
    embedded_shutdown_is_clean_after_runs_dropped_at_any_instant().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn adversary_embedded_shutdown_is_clean_after_runs_dropped_at_any_instant_one_worker() {
    embedded_shutdown_is_clean_after_runs_dropped_at_any_instant().await;
}
