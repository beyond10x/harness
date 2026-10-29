//! The shipped ports, awaited from a tokio program through `LoopTask` (story
//! `real-ports-run-under-an-awaited-loop`).
//!
//! The `harness_llm` model, `harness_mcp::McpTools` and the catalogue over the embedded Substrate
//! backend each call `Runtime::block_on` on a private runtime, or own a runtime whose drop panics
//! inside another runtime's context. So each is built in the `LoopTask` builder, which runs on the
//! task's own thread, and dropped there by `shutdown`. Every case runs once on each tokio flavour,
//! because a current-thread runtime and a multi-thread worker refuse a nested runtime in different
//! places.
//!
//! Each case bounds everything from the first command to the end of `shutdown` at [`WAIT`], so a
//! task that never answers is a red test and never a hung one.

use std::collections::VecDeque;
use std::fs;
use std::future::Future;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use harness_catalogue::{Catalogue, Flat};
use harness_llm::{Protocol, Target};
use harness_loop::{
    ApproveAll, LoopCancel, LoopConfig, LoopPorts, LoopStop, LoopTask, VecLoopSink,
};
use harness_mcp::McpTools;
use harness_substrate::{Backend as _, ConfinedOperations, Embedded};
use harness_wire::{
    CallId, Item, ModelPort, StopReason, StreamSink, ToolCall, ToolName, ToolOutcome, ToolPort,
    ToolSpec, TurnOutcome, TurnRequest, WireError, WireId,
};
use serde_json::{Value, json};

mod common;

use common::controlled_fixture_binary;

/// The longest any case waits on the task, from its first command to the end of `shutdown`.
const WAIT: Duration = Duration::from_secs(30);

async fn within<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(WAIT, future)
        .await
        .expect("the loop task answered and shut down within 30 s")
}

/// Refuses to build when the builder runs inside a runtime's context.
///
/// The ports below block on private runtimes, which panics there; the builder's thread is the one
/// place they are safe, and this says so by name if it ever is not.
fn outside_any_runtime() -> Result<(), String> {
    match tokio::runtime::Handle::try_current() {
        Ok(_) => Err("the builder ran inside a tokio runtime's context".to_owned()),
        Err(_) => Ok(()),
    }
}

fn config() -> LoopConfig {
    LoopConfig::new("b10x-emulated", "Answer briefly.")
}

// ---------------------------------------------------------------------------------------------
// The model: `harness_llm::client` against a local Responses endpoint.
// ---------------------------------------------------------------------------------------------

/// What the fixture endpoint streams back as the assistant's answer.
const FIXTURE_ANSWER: &str = "the fixture endpoint answered";

/// A Responses endpoint on a loopback port that answers one request with [`FIXTURE_ANSWER`] and
/// hands the request body it read to the test.
struct ResponsesEndpoint {
    base_url: String,
    requests: mpsc::Receiver<String>,
}

impl ResponsesEndpoint {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the fixture endpoint");
        let address = listener
            .local_addr()
            .expect("the fixture endpoint's address");
        let (sender, requests) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept the model request");
            let body = {
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                reader.read_line(&mut line).expect("read the request line");
                let mut length = 0;
                loop {
                    line.clear();
                    reader.read_line(&mut line).expect("read a request header");
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = line.trim_end().split_once(':')
                        && name.eq_ignore_ascii_case("content-length")
                    {
                        length = value.trim().parse().expect("a numeric content length");
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).expect("read the request body");
                String::from_utf8(body).expect("the request body is UTF-8")
            };
            let events = text_events(FIXTURE_ANSWER);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\
                 Cache-Control: no-store\r\nConnection: close\r\n\r\n{events}",
                events.len()
            )
            .expect("write the fixture response");
            stream.flush().expect("flush the fixture response");
            let _ = sender.send(body);
        });
        Self {
            base_url: format!("http://{address}/v1"),
            requests,
        }
    }
}

/// One assistant message carrying `text`, as a Responses stream.
fn text_events(text: &str) -> String {
    let item = format!(
        r#"{{"id":"msg_b10x_001","type":"message","status":"completed","role":"assistant","content":[{{"type":"output_text","text":"{text}","annotations":[]}}]}}"#
    );
    [
        r#"{"type":"response.created","response":{"id":"resp_b10x_001","object":"response","created_at":1786706400,"status":"in_progress","model":"b10x-emulated","output":[],"incomplete_details":null,"error":null,"usage":null}}"#.to_owned(),
        r#"{"type":"response.output_item.added","output_index":0,"item":{"id":"msg_b10x_001","type":"message","status":"in_progress","role":"assistant","content":[]}}"#.to_owned(),
        format!(r#"{{"type":"response.output_text.delta","item_id":"msg_b10x_001","output_index":0,"delta":"{text}"}}"#),
        format!(r#"{{"type":"response.output_item.done","output_index":0,"item":{item}}}"#),
        format!(r#"{{"type":"response.completed","response":{{"id":"resp_b10x_001","object":"response","created_at":1786706400,"status":"completed","model":"b10x-emulated","output":[{item}],"incomplete_details":null,"error":null,"usage":{{"input_tokens":42,"input_tokens_details":{{"cached_tokens":7}},"output_tokens":4,"output_tokens_details":{{"reasoning_tokens":0}},"total_tokens":46}}}}}}"#),
    ]
    .iter()
    .fold(String::new(), |mut stream, event| {
        stream.push_str("data: ");
        stream.push_str(event);
        stream.push_str("\n\n");
        stream
    })
}

async fn llm_port_runs_under_loop_task() {
    let endpoint = ResponsesEndpoint::start();
    let base_url = endpoint.base_url.clone();
    let task = LoopTask::spawn(config(), move |cancel: &LoopCancel| {
        outside_any_runtime()?;
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

    let shut = within(async move {
        let ran = task
            .run("Say what the fixture says.", VecLoopSink::new())
            .await
            .expect("the task answered the run");
        let outcome = ran
            .result
            .expect("the run over the real model port completed");
        assert_eq!(outcome.stop, LoopStop::Completed);
        assert_eq!(
            outcome.text, FIXTURE_ANSWER,
            "the outcome's text is the fixture's"
        );
        task.shutdown().await
    })
    .await;
    assert_eq!(shut, Ok(()), "the model port dropped on the task's thread");

    let request = endpoint
        .requests
        .recv_timeout(WAIT)
        .expect("the fixture endpoint read the model request");
    assert!(
        request.contains("b10x-emulated"),
        "the request named the run's model: {request}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn llm_port_runs_under_loop_task_current_thread() {
    llm_port_runs_under_loop_task().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn llm_port_runs_under_loop_task_multi_thread() {
    llm_port_runs_under_loop_task().await;
}

// ---------------------------------------------------------------------------------------------
// A scripted model, for the cases whose subject is the tool port.
// ---------------------------------------------------------------------------------------------

/// Answers each turn with the next scripted outcome.
struct Scripted {
    wire: WireId,
    script: VecDeque<TurnOutcome>,
}

impl Scripted {
    /// One tool call, then one closing answer.
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

/// A tool port with nothing in it, for the case whose subject is the model.
struct NoTools;

impl ToolPort for NoTools {
    fn specs(&self) -> &[ToolSpec] {
        &[]
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        ToolOutcome::failed(format!("no tool `{}` in this run", call.name))
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

fn call(id: &str, name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        call_id: CallId::new(id).expect("a call id"),
        name: ToolName::new(name).expect("a tool name"),
        arguments,
    }
}

/// The result the run left in its conversation for `call_id`.
fn result_of<'a>(items: &'a [Item], call_id: &str) -> (&'a Value, bool) {
    items
        .iter()
        .find_map(|item| match item {
            Item::ToolResult {
                call_id: id,
                output,
                failed,
            } if id.as_str() == call_id => Some((output, *failed)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("a result for `{call_id}` in the returned items: {items:?}"))
}

// ---------------------------------------------------------------------------------------------
// MCP: `McpTools` over the stdio controlled endpoint.
// ---------------------------------------------------------------------------------------------

/// A reviewed registry and profile publishing the stdio fixture's `read_issue`.
struct McpFiles {
    _directory: tempfile::TempDir,
    registry: PathBuf,
    profile: PathBuf,
}

impl McpFiles {
    fn stdio() -> Self {
        const CONNECTION: &str = "stdio_fixture";
        let directory = tempfile::tempdir().expect("an MCP configuration directory");
        let registry_document = format!(
            "[connections.{CONNECTION}]\ntransport = \"stdio\"\nprogram = {:?}\nargs = [\"stdio\"]\ncwd = {:?}\n",
            controlled_fixture_binary().to_string_lossy(),
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
        }
    }
}

async fn mcp_tools_run_under_loop_task() {
    // Outside the bound: compiling the fixture is the test's set-up, not the task's answer.
    let files = McpFiles::stdio();
    let (profile, registry) = (files.profile.clone(), files.registry.clone());
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        outside_any_runtime()?;
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
    });

    let shut = within(async move {
        let ran = task
            .run_in(Vec::new(), "Read ISSUE-7.", VecLoopSink::new())
            .await
            .expect("the task answered the run");
        let outcome = ran.result.expect("the run over the MCP tools completed");
        assert_eq!(outcome.stop, LoopStop::Completed);
        let (output, failed) = result_of(&ran.items, "call_mcp_1");
        assert!(!failed, "the MCP call succeeded: {output}");
        assert!(
            output.to_string().contains("controlled issue is open"),
            "the fixture tool's result is in the returned items: {output}"
        );
        task.shutdown().await
    })
    .await;
    assert_eq!(
        shut,
        Ok(()),
        "McpTools and its runtime dropped on the task's thread"
    );
    drop(files);
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_tools_run_under_loop_task_current_thread() {
    mcp_tools_run_under_loop_task().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_tools_run_under_loop_task_multi_thread() {
    mcp_tools_run_under_loop_task().await;
}

// ---------------------------------------------------------------------------------------------
// Substrate: the catalogue over the embedded backend.
// ---------------------------------------------------------------------------------------------

/// What the builder writes into the workspace before the run, and the model's call reads back.
const NOTE: &str = "written on the task's thread\n";

async fn embedded_file_operation_runs_under_loop_task() {
    let root = tempfile::tempdir().expect("a workspace root");
    let path = root.path().to_owned();
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        outside_any_runtime()?;
        let embedded = Embedded::open(&path, None).map_err(|error| error.to_string())?;
        let facts = embedded.machine().map_err(|error| error.to_string())?;
        let workspace = embedded
            .workspace_create(600_000)
            .map_err(|error| error.to_string())?;
        embedded
            .file_write(&workspace, "note.txt", NOTE)
            .map_err(|error| error.to_string())?;
        let catalogue = Catalogue::of(ConfinedOperations::new(
            embedded,
            &facts,
            workspace,
            Vec::new(),
        ));
        Ok::<_, String>(LoopPorts::new(
            Scripted::calling(call(
                "call_file_1",
                "file_read",
                json!({"path": "note.txt"}),
            )),
            Flat::new(catalogue),
            ApproveAll,
        ))
    });

    let shut = within(async move {
        let ran = task
            .run_in(Vec::new(), "Read note.txt.", VecLoopSink::new())
            .await
            .expect("the task answered the run");
        let outcome = ran
            .result
            .expect("the run over the embedded backend completed");
        assert_eq!(outcome.stop, LoopStop::Completed);
        let (output, failed) = result_of(&ran.items, "call_file_1");
        assert!(!failed, "the file operation succeeded: {output}");
        assert!(
            output.to_string().contains("written on the task's thread"),
            "the file operation's result is in the returned items: {output}"
        );
        task.shutdown().await
    })
    .await;
    assert_eq!(
        shut,
        Ok(()),
        "the embedded backend and its runtime dropped on the task's thread"
    );
    drop(root);
}

#[tokio::test(flavor = "current_thread")]
async fn embedded_file_operation_runs_under_loop_task_current_thread() {
    embedded_file_operation_runs_under_loop_task().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_file_operation_runs_under_loop_task_multi_thread() {
    embedded_file_operation_runs_under_loop_task().await;
}
