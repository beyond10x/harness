//! The shipped binary driven the way `runtime/agent`'s bridge drives Codex.
//!
//! This test *is* the client: it writes the pinned client methods to the process's stdin and reads
//! the pinned notifications back. If it passes, the same frames the existing bridge sends produce
//! the frames it expects — which is what makes bridge mode reuse rather than a second integration.
//!
//! It is not a substitute for running the real bridge against this binary. That crosses a component
//! boundary and has not been done; `STATUS.md` says so.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use harness_app_server::ServerConfig;
use harness_loop::Budget;
use harness_wire::{Cancel, ModelPort, StreamSink, TurnOutcome, TurnRequest, WireError, WireId};
use serde_json::{Value, json};

const BINARY: &str = env!("CARGO_BIN_EXE_b10x-harness");
const READ_TIMEOUT: Duration = Duration::from_secs(20);

/// Methods this server is allowed to emit. A copy of the crate's own inventory, so a test failure
/// names the drift rather than quietly accepting a new method.
const SERVER_METHODS: &[&str] = &[
    "item/agentMessage/delta",
    "item/completed",
    "item/started",
    "item/tool/call",
    "thread/started",
    "thread/tokenUsage/updated",
    "turn/completed",
    "turn/started",
];

struct Endpoint {
    child: Child,
    base_url: String,
}

impl Endpoint {
    fn start(scenario: &str) -> Self {
        let script = PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets the manifest directory"),
        )
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

impl Drop for Endpoint {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The bridge side of the connection.
struct Bridge {
    child: Child,
    stdin: Option<ChildStdin>,
    frames: Receiver<Value>,
    /// Notifications read while waiting for a response, kept so the test still sees them.
    held: VecDeque<Value>,
    next_id: i64,
}

impl Bridge {
    fn start(endpoint: &Endpoint, extra: &[&str]) -> Self {
        Self::start_at(&endpoint.base_url, extra)
    }

    /// The same server, pointed at a `--base-url` of the caller's choosing.
    ///
    /// For a case that needs the binary to fail before it reaches a wire at all: `model_client`
    /// validates the URL per turn, so the handshake and `thread/start` still work.
    fn start_at(base_url: &str, extra: &[&str]) -> Self {
        let mut arguments = vec![
            "app-server",
            "--base-url",
            base_url,
            "--model",
            "b10x-emulated",
            "--api-key-env",
            "B10X_HARNESS_TEST_KEY",
        ];
        arguments.extend_from_slice(extra);
        let mut child = Command::new(BINARY)
            .args(&arguments)
            .env("B10X_HARNESS_TEST_KEY", "test-key")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the server starts");
        let stdout = child.stdout.take().expect("piped stdout");
        let (sender, frames) = channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                let frame: Value = serde_json::from_str(&line).unwrap_or_else(|error| {
                    panic!("server wrote a non-JSON frame: {line} ({error})")
                });
                if sender.send(frame).is_err() {
                    return;
                }
            }
        });
        Self {
            stdin: child.stdin.take(),
            child,
            frames,
            held: VecDeque::new(),
            next_id: 1,
        }
    }

    fn write(&mut self, frame: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        writeln!(stdin, "{frame}").expect("writing to the server");
        stdin.flush().expect("flushing to the server");
    }

    /// Writes two frames in one `write`, the way a client that pipelines them actually does.
    ///
    /// Not a detail. Written separately, the server's reading thread decodes the first, comes back
    /// for the second and finds the pipe empty, and *blocks* -- handing the main thread a whole
    /// scheduling quantum in which to run a turn that ends in microseconds. The interrupt is then
    /// decoded after its turn is over, which is a real client behaving differently from the one
    /// the case means to describe, not a defect in the server. In one buffer the second line is
    /// already in the reader's `BufReader` when it asks, so no second read syscall happens at all.
    ///
    /// Measured on the `ftp://not-a-wire` case, 20 attempts per run, whole suite at
    /// `--test-threads=17`: 15/17/19/19 hits split, 20/20 every run combined.
    fn write_pipelined(&mut self, first: &Value, second: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        let both = format!("{first}\n{second}\n");
        stdin
            .write_all(both.as_bytes())
            .expect("writing to the server");
        stdin.flush().expect("flushing to the server");
    }

    fn request(&mut self, method: &str, params: &Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.write(&json!({"id": id, "method": method, "params": params}));
        self.await_response(id)
    }

    fn notify(&mut self, method: &str, params: &Value) {
        self.write(&json!({"method": method, "params": params}));
    }

    fn next_frame(&mut self) -> Value {
        if let Some(frame) = self.held.pop_front() {
            return frame;
        }
        match self.frames.recv_timeout(READ_TIMEOUT) {
            Ok(frame) => frame,
            Err(RecvTimeoutError::Timeout) => {
                panic!("the server sent nothing within {READ_TIMEOUT:?}")
            }
            Err(RecvTimeoutError::Disconnected) => panic!("the server closed its output"),
        }
    }

    /// Reads until the answer to `id`, keeping the notifications seen on the way.
    ///
    /// Keeping them matters: a turn can finish while its interrupt is still being acknowledged, and
    /// a test that discarded `turn/completed` here would wait forever for a second one.
    fn await_response(&mut self, id: i64) -> Value {
        let mut passed = Vec::new();
        loop {
            let frame = self.next_frame();
            if frame.get("id") == Some(&json!(id)) && frame.get("method").is_none() {
                for held in passed.into_iter().rev() {
                    self.held.push_front(held);
                }
                return frame;
            }
            assert!(
                frame.get("method").is_some(),
                "an unexpected answer arrived: {frame}"
            );
            passed.push(frame);
        }
    }

    /// Reads past unrelated notifications to the next `method` frame.
    ///
    /// Usage is reported as soon as a turn reports it, which is before the calls that turn asked
    /// for. Skipping keeps the orderings a test cares about without pinning ones it does not.
    fn skip_to(&mut self, method: &str) -> Value {
        loop {
            let frame = self.next_frame();
            if frame.get("method").and_then(Value::as_str) == Some(method) {
                return frame;
            }
        }
    }

    /// Reads notifications until `method`, returning everything seen including it.
    fn collect_until(&mut self, method: &str) -> Vec<Value> {
        let mut seen = Vec::new();
        loop {
            let frame = self.next_frame();
            let done = frame.get("method").and_then(Value::as_str) == Some(method);
            seen.push(frame);
            if done {
                return seen;
            }
        }
    }

    fn handshake(&mut self) -> Value {
        let response = self.request(
            "initialize",
            &json!({
                "clientInfo": {"name": "test"},
                "capabilities": {"experimentalApi": true},
            }),
        );
        self.notify("initialized", &json!({}));
        response
    }

    /// A handshake without the capability the tool-calling profile requires.
    fn handshake_stable(&mut self) -> Value {
        let response = self.request("initialize", &json!({"clientInfo": {"name": "test"}}));
        self.notify("initialized", &json!({}));
        response
    }

    fn start_thread(&mut self, params: &Value) -> String {
        let response = self.request("thread/start", params);
        let thread_id = response["result"]["thread"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("thread/start answered without an id: {response}"))
            .to_owned();
        let started = self.next_frame();
        assert_eq!(started["method"], json!("thread/started"));
        assert_eq!(started["params"]["thread"]["id"], json!(thread_id));
        thread_id
    }

    fn start_turn(&mut self, thread_id: &str, text: &str) -> String {
        let response = self.request(
            "turn/start",
            &json!({"threadId": thread_id, "input": [{"type": "text", "text": text}]}),
        );
        let turn_id = response["result"]["turn"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("turn/start answered without an id: {response}"))
            .to_owned();
        let started = self.next_frame();
        assert_eq!(started["method"], json!("turn/started"));
        assert_eq!(started["params"]["turn"]["status"], json!("inProgress"));
        turn_id
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn methods(frames: &[Value]) -> Vec<&str> {
    frames
        .iter()
        .filter_map(|frame| frame.get("method").and_then(Value::as_str))
        .collect()
}

fn assert_within_inventory(frames: &[Value]) {
    for method in methods(frames) {
        assert!(
            SERVER_METHODS.contains(&method),
            "`{method}` is outside the pinned server inventory; the bridge would refuse it"
        );
    }
}

#[test]
fn the_handshake_says_which_implementation_answered() {
    let endpoint = Endpoint::start("text");
    let mut bridge = Bridge::start(&endpoint, &[]);
    let response = bridge.handshake();

    let name = response["result"]["userAgent"]["name"]
        .as_str()
        .expect("initialize names its implementation");
    assert_eq!(
        name, "b10x-harness",
        "a server that impersonates the vendor makes an incident unreadable"
    );
    assert_eq!(
        response["result"]["userAgent"]["profile"],
        json!("codex-app-server-stdio-v2-dynamic-operation-tools-experimental")
    );
}

#[test]
fn a_text_turn_produces_the_pinned_notification_sequence() {
    let endpoint = Endpoint::start("text");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({"developerInstructions": "be useful"}));
    let turn_id = bridge.start_turn(&thread_id, "say something");

    let frames = bridge.collect_until("turn/completed");
    assert_within_inventory(&frames);

    let seen = methods(&frames);
    assert!(
        seen.iter()
            .filter(|method| **method == "item/agentMessage/delta")
            .count()
            >= 2,
        "streamed text must arrive as it is produced: {seen:?}"
    );
    assert!(seen.contains(&"thread/tokenUsage/updated"), "{seen:?}");

    let message = frames
        .iter()
        .find(|frame| {
            frame["method"] == json!("item/completed")
                && frame["params"]["item"]["type"] == json!("agentMessage")
        })
        .expect("the answer is completed as an agent message");
    assert_eq!(
        message["params"]["item"]["text"],
        json!("provider emulation passed")
    );

    let completed = frames.last().expect("a terminal frame");
    assert_eq!(completed["params"]["turn"]["id"], json!(turn_id));
    assert_eq!(completed["params"]["turn"]["status"], json!("completed"));

    // Every scoped notification must carry the ids the bridge validates against.
    for frame in &frames {
        if frame["method"] == json!("turn/completed") {
            continue;
        }
        assert_eq!(
            frame["params"]["threadId"],
            json!(thread_id),
            "unscoped frame: {frame}"
        );
    }
}

#[test]
fn a_tool_call_round_trips_back_to_the_client() {
    let endpoint = Endpoint::start("dynamic-tool");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({
        "developerInstructions": "be useful",
        "dynamicTools": [{
            "type": "function",
            "name": "workspace_read",
            "description": "reads one file",
            "deferLoading": false,
            "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}},
        }],
    }));
    let turn_id = bridge.start_turn(&thread_id, "read the readme");

    // The call must be announced before it is requested: the bridge refuses a callback for a call
    // it has not already registered from `item/started`.
    let started = bridge.skip_to("item/started");
    assert_eq!(started["params"]["item"]["type"], json!("dynamicToolCall"));
    let call_item_id = started["params"]["item"]["id"]
        .as_str()
        .expect("the item carries an id")
        .to_owned();

    let call = bridge.next_frame();
    assert_eq!(call["method"], json!("item/tool/call"));
    assert_eq!(call["params"]["tool"], json!("workspace_read"));
    assert_eq!(call["params"]["threadId"], json!(thread_id));
    assert_eq!(call["params"]["turnId"], json!(turn_id));
    assert_eq!(
        call["params"]["callId"],
        json!(call_item_id),
        "the callback must name the call the bridge registered"
    );
    assert_eq!(call["params"]["arguments"], json!({"path": "README.md"}));

    bridge.write(&json!({
        "id": call["id"],
        "result": {
            "contentItems": [{"type": "inputText", "text": "hello harness"}],
            "success": true,
        },
    }));

    let completed = bridge.next_frame();
    assert_eq!(completed["method"], json!("item/completed"));
    assert_eq!(
        completed["params"]["item"]["type"],
        json!("dynamicToolCall")
    );
    assert_eq!(completed["params"]["item"]["id"], json!(call_item_id));
    assert_eq!(completed["params"]["item"]["status"], json!("completed"));

    let frames = bridge.collect_until("turn/completed");
    assert_within_inventory(&frames);
    assert_eq!(
        frames.last().expect("terminal")["params"]["turn"]["status"],
        json!("completed")
    );
}

#[test]
fn a_client_refusal_reaches_the_model_as_a_failed_call() {
    let endpoint = Endpoint::start("dynamic-tool");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({
        "dynamicTools": [{"name": "workspace_read", "inputSchema": {"type": "object"}}],
    }));
    bridge.start_turn(&thread_id, "read the readme");

    bridge.skip_to("item/started");
    let call = bridge.next_frame();
    assert_eq!(call["method"], json!("item/tool/call"));
    bridge.write(&json!({
        "id": call["id"],
        "result": {
            "contentItems": [{"type": "inputText", "text": "not granted"}],
            "success": false,
        },
    }));

    let completed = bridge.next_frame();
    assert_eq!(
        completed["params"]["item"]["status"],
        json!("failed"),
        "a refusal must be reported as one, or the model learns the effect happened"
    );

    // The turn still finishes: the model is told the call failed and answers anyway.
    let frames = bridge.collect_until("turn/completed");
    assert_eq!(
        frames.last().expect("terminal")["params"]["turn"]["status"],
        json!("completed")
    );
}

#[test]
fn pinned_methods_this_server_does_not_implement_are_refused_by_name() {
    let endpoint = Endpoint::start("text");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();

    for method in ["thread/resume", "turn/steer"] {
        let response = bridge.request(method, &json!({}));
        assert_eq!(
            response["error"]["code"],
            json!(-32601),
            "`{method}` must refuse rather than appear to work: {response}"
        );
        assert!(
            response["error"]["message"]
                .as_str()
                .expect("a message")
                .contains(method),
            "the refusal names the method: {response}"
        );
    }
}

#[test]
fn a_turn_for_another_thread_is_refused() {
    let endpoint = Endpoint::start("text");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    bridge.start_thread(&json!({}));
    let response = bridge.request(
        "turn/start",
        &json!({"threadId": "thr-not-mine", "input": [{"type": "text", "text": "hi"}]}),
    );
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");
}

#[test]
fn a_turn_with_no_input_is_refused_rather_than_run_empty() {
    let endpoint = Endpoint::start("text");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));
    let response = bridge.request("turn/start", &json!({"threadId": thread_id, "input": []}));
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");
}

#[test]
fn an_unusable_tool_registration_is_refused_at_thread_start() {
    let endpoint = Endpoint::start("text");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let response = bridge.request(
        "thread/start",
        &json!({"dynamicTools": [{"description": "no name"}]}),
    );
    assert_eq!(
        response["error"]["code"],
        json!(-32602),
        "a tool that could never be called must be refused, not silently dropped: {response}"
    );
}

#[test]
fn a_turn_the_endpoint_fails_is_reported_as_failed_not_answered() {
    let endpoint = Endpoint::start("unauthorized");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));
    bridge.start_turn(&thread_id, "say something");

    let frames = bridge.collect_until("turn/completed");
    let terminal = frames.last().expect("a terminal frame");
    assert_eq!(terminal["params"]["turn"]["status"], json!("failed"));
    assert!(
        terminal["params"]["turn"]["error"]["message"]
            .as_str()
            .expect("a reason")
            .to_lowercase()
            .contains("unauthorized"),
        "the reason travels with the failure: {terminal}"
    );
    assert!(
        !methods(&frames).contains(&"item/completed"),
        "a failed turn must not also deliver an answer: {frames:?}"
    );
}

#[test]
fn a_turn_stopped_by_a_budget_is_failed_rather_than_completed() {
    let endpoint = Endpoint::start("dynamic-tool");
    let mut bridge = Bridge::start(&endpoint, &["--max-turns", "1"]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({
        "dynamicTools": [{"name": "workspace_read", "inputSchema": {"type": "object"}}],
    }));
    bridge.start_turn(&thread_id, "read the readme");

    bridge.skip_to("item/started");
    let call = bridge.next_frame();
    assert_eq!(call["method"], json!("item/tool/call"));
    bridge.write(&json!({
        "id": call["id"],
        "result": {"contentItems": [{"type": "inputText", "text": "hello"}], "success": true},
    }));

    let frames = bridge.collect_until("turn/completed");
    assert_eq!(
        frames.last().expect("terminal")["params"]["turn"]["status"],
        json!("failed"),
        "a bound run is not an answer"
    );
}

#[test]
fn two_turns_run_on_one_thread() {
    let endpoint = Endpoint::start("text");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));

    let first = bridge.start_turn(&thread_id, "one");
    let frames = bridge.collect_until("turn/completed");
    assert_eq!(
        frames.last().expect("terminal")["params"]["turn"]["id"],
        json!(first)
    );

    let second = bridge.start_turn(&thread_id, "two");
    let frames = bridge.collect_until("turn/completed");
    assert_eq!(
        frames.last().expect("terminal")["params"]["turn"]["id"],
        json!(second)
    );
    assert_ne!(first, second, "each turn is addressable on its own");
}

#[test]
fn an_interrupt_stops_a_turn_that_is_blocked_on_the_model() {
    let endpoint = Endpoint::start("slow");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));
    let turn_id = bridge.start_turn(&thread_id, "take your time");

    // Wait until text is actually flowing, so the interrupt lands while the turn is blocked on the
    // model rather than between turns. Cancelling only at a boundary would prove nothing.
    let first = bridge.skip_to("item/agentMessage/delta");
    assert_eq!(first["params"]["turnId"], json!(turn_id));

    let response = bridge.request(
        "turn/interrupt",
        &json!({"threadId": thread_id, "turnId": turn_id}),
    );
    assert!(
        response["error"].is_null(),
        "the interrupt is acknowledged: {response}"
    );

    let frames = bridge.collect_until("turn/completed");
    let terminal = frames.last().expect("a terminal frame");
    assert_eq!(terminal["params"]["turn"]["status"], json!("interrupted"));
    assert!(
        !methods(&frames).contains(&"item/completed"),
        "an interrupted turn must not also deliver the answer it was stopped from giving: {frames:?}"
    );
}

#[test]
fn a_thread_survives_an_interrupted_turn() {
    let endpoint = Endpoint::start("slow");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));
    let first = bridge.start_turn(&thread_id, "take your time");
    bridge.skip_to("item/agentMessage/delta");
    bridge.request(
        "turn/interrupt",
        &json!({"threadId": thread_id, "turnId": first}),
    );
    let frames = bridge.collect_until("turn/completed");
    assert_eq!(
        frames.last().expect("terminal")["params"]["turn"]["status"],
        json!("interrupted")
    );

    // The next turn must start uncancelled. A token left set would end it before it ran, which
    // looks from outside like the connection silently died.
    let second = bridge.start_turn(&thread_id, "again");
    let frames = bridge.collect_until("turn/completed");
    let terminal = frames.last().expect("a terminal frame");
    assert_eq!(terminal["params"]["turn"]["id"], json!(second));
    assert_eq!(
        terminal["params"]["turn"]["status"],
        json!("completed"),
        "a token left set by the previous turn would end this one before it ran"
    );
}

#[test]
fn registering_tools_without_the_negotiated_capability_is_refused_at_thread_start() {
    let endpoint = Endpoint::start("text");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake_stable();

    let response = bridge.request(
        "thread/start",
        &json!({"dynamicTools": [{"name": "workspace_read", "inputSchema": {"type": "object"}}]}),
    );
    // The client's own stable profile refuses `item/tool/call`, so accepting the registration here
    // would strand the turn at its first tool call instead of saying so while it can still act.
    assert_eq!(response["error"]["code"], json!(-32602), "{response}");
    assert!(
        response["error"]["message"]
            .as_str()
            .expect("a message")
            .contains("experimentalApi"),
        "the refusal names what is missing: {response}"
    );
}

#[test]
fn a_text_only_thread_still_works_without_the_capability() {
    let endpoint = Endpoint::start("text");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake_stable();
    let thread_id = bridge.start_thread(&json!({}));
    bridge.start_turn(&thread_id, "say something");

    let frames = bridge.collect_until("turn/completed");
    assert_eq!(
        frames.last().expect("terminal")["params"]["turn"]["status"],
        json!("completed"),
        "registering no tools needs no capability"
    );
}

#[test]
fn an_interrupt_queued_before_the_turn_starts_does_not_crash_the_server() {
    // The regression that mattered: a client pipelining `turn/start` and `turn/interrupt` made the
    // server take a second borrow of its writer while the first was live, and it aborted with no
    // terminal frame at all. A bridge saw only a dead pipe.
    let endpoint = Endpoint::start("slow");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));

    let turn = bridge.request(
        "turn/start",
        &json!({"threadId": thread_id, "input": [{"type": "text", "text": "go"}]}),
    );
    let turn_id = turn["result"]["turn"]["id"]
        .as_str()
        .expect("a turn id")
        .to_owned();
    // Written the instant `turn/start` is answered, with no wait for a streamed event first. That
    // wait used to be here, because the server announced a turn before it installed the control
    // its reading thread cancels through and an interrupt decoded in between was acknowledged and
    // cancelled nothing -- a window microseconds wide that a loaded machine landed in. The control
    // is installed before the announcement now, and an interrupt the reading thread could not
    // reach is counted rather than dropped, so the ordering the client has to keep is gone.
    bridge.write(&json!({"id": 99, "method": "turn/interrupt", "params": {
        "threadId": thread_id, "turnId": turn_id,
    }}));

    let frames = bridge.collect_until("turn/completed");
    assert_eq!(
        frames.last().expect("terminal")["params"]["turn"]["status"],
        json!("interrupted"),
        "the server must survive and report, not die"
    );
}

#[test]
fn an_unknown_request_mid_turn_is_refused_without_killing_the_server() {
    let endpoint = Endpoint::start("slow");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));
    let turn_id = bridge.start_turn(&thread_id, "go");

    // Same nested-borrow path, reached by a different frame, and sent on the strength of
    // `turn/started` alone -- which is now a promise that the turn is interruptible, not one that
    // it is about to be.
    bridge.write(&json!({"id": 98, "method": "thread/resume", "params": {}}));
    bridge.write(&json!({"id": 97, "method": "turn/interrupt", "params": {
        "threadId": thread_id, "turnId": turn_id,
    }}));

    let frames = bridge.collect_until("turn/completed");
    assert_eq!(
        frames.last().expect("terminal")["params"]["turn"]["status"],
        json!("interrupted")
    );
}

#[test]
fn an_acknowledged_interrupt_never_also_delivers_the_answer() {
    // A client that pipelines `turn/start` and `turn/interrupt` — the shape
    // `an_interrupt_queued_before_the_turn_starts_does_not_crash_the_server` names in its own
    // first comment, and the one no test performs since that test began waiting for a streamed
    // delta first.
    //
    // The server answers `turn/start` and notifies `turn/started` before it installs the control
    // its reading thread cancels through (`crates/harness-app-server/src/lib.rs:321-340`). An
    // interrupt decoded in that window is answered `{"result":{}}` — a success — and cancels
    // nothing. Whichever way that window is closed, these two cannot both be true of one turn:
    // the interrupt was acknowledged, and the answer it was meant to stop was delivered anyway.
    // `an_interrupt_stops_a_turn_that_is_blocked_on_the_model` already says so for a turn that is
    // mid-stream; nothing said it for a turn that was only just announced.
    let endpoint = Endpoint::start("slow");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));

    // Both frames written before either answer is read. A pipelining client cannot name the turn
    // it is cancelling — it has not been told the id yet — and this server reads no `turnId` on
    // `turn/interrupt`: the reading thread cancels whichever turn is active.
    bridge.write(&json!({"id": 50, "method": "turn/start", "params": {
        "threadId": thread_id, "input": [{"type": "text", "text": "go"}],
    }}));
    bridge.write(&json!({"id": 99, "method": "turn/interrupt", "params": {
        "threadId": thread_id,
    }}));

    let mut acknowledgement = None;
    let mut frames = Vec::new();
    loop {
        let frame = bridge.next_frame();
        if frame.get("id") == Some(&json!(99)) && frame.get("method").is_none() {
            acknowledgement = Some(frame);
            continue;
        }
        let terminal = frame.get("method").and_then(Value::as_str) == Some("turn/completed");
        frames.push(frame);
        if terminal {
            break;
        }
    }
    let acknowledgement = acknowledgement.expect("the interrupt is answered at all");
    assert!(
        acknowledgement["error"].is_null(),
        "the server acknowledged the interrupt: {acknowledgement}"
    );
    assert!(
        !methods(&frames).contains(&"item/completed"),
        "an acknowledged interrupt must not also deliver the answer it was stopped from giving: \
         {frames:?}"
    );
    assert_eq!(
        frames.last().expect("a terminal frame")["params"]["turn"]["status"],
        json!("interrupted"),
        "an interrupt the server answered with a success must have been acted on"
    );
}

#[test]
fn a_request_arriving_mid_stream_is_refused_before_the_turn_ends() {
    // The path `an_unknown_request_mid_turn_is_refused_without_killing_the_server` is named for:
    // `Wire::drain_control`, which answers control frames *between streamed events* — the only
    // moment a running turn is at the wire (`STATUS.md:17`, "acknowledged between streamed
    // events"). Reaching it needs a frame that does not itself end the turn; an interrupt cancels
    // the stream, so nothing is emitted afterwards and the answer necessarily comes from the main
    // loop once the turn is over, by the other code path and with the other message.
    //
    // Without this case the whole `drain_control` request arm is unreached: re-nesting the two
    // `writer.borrow_mut()` calls in `BridgeSink::notify` — the exact regression its comment
    // warns about — leaves every test in this workspace green.
    let endpoint = Endpoint::start("slow");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));
    let turn_id = bridge.start_turn(&thread_id, "go");
    bridge.skip_to("item/agentMessage/delta");

    bridge.write(&json!({"id": 98, "method": "thread/resume", "params": {}}));

    let mut refusal = None;
    let mut frames = Vec::new();
    loop {
        let frame = bridge.next_frame();
        if frame.get("id") == Some(&json!(98)) && frame.get("method").is_none() {
            refusal = Some(frame);
            continue;
        }
        let terminal = frame.get("method").and_then(Value::as_str) == Some("turn/completed");
        frames.push(frame);
        if terminal {
            break;
        }
    }
    let refusal = refusal.expect("the refusal arrives before the turn ends, not after it");
    assert_eq!(refusal["error"]["code"], json!(-32601), "{refusal}");
    assert!(
        refusal["error"]["message"]
            .as_str()
            .expect("a message")
            .contains("while a turn is running"),
        "the refusal came from the mid-turn path, not from the main loop after the turn: {refusal}"
    );
    assert_eq!(
        frames.last().expect("a terminal frame")["params"]["turn"]["id"],
        json!(turn_id)
    );
    assert_eq!(
        frames.last().expect("a terminal frame")["params"]["turn"]["status"],
        json!("completed"),
        "a refused control frame must not end the turn it arrived during"
    );
}

#[test]
fn an_interrupt_sent_between_turns_does_not_cancel_the_next_one() {
    // The other half of closing the start-of-turn window, and the one that can be closed wrongly.
    // An interrupt the reading thread cannot cancel with is not discarded any more, so what stops
    // it reaching the *next* turn has to be said out loud: the main loop answers it first, because
    // frames leave the queue in the order the client sent them, and an interrupt sent before
    // `turn/start` is dequeued before `turn/start` is. `Watch`'s own comment calls the alternative
    // arming a trap for the next turn, and nothing in this file caught it being armed.
    let endpoint = Endpoint::start("text");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));

    let acknowledgement = bridge.request("turn/interrupt", &json!({"threadId": thread_id}));
    assert!(
        acknowledgement["error"].is_null(),
        "an interrupt with no turn to stop is still answered: {acknowledgement}"
    );

    bridge.start_turn(&thread_id, "say something");
    let frames = bridge.collect_until("turn/completed");
    assert_eq!(
        frames.last().expect("a terminal frame")["params"]["turn"]["status"],
        json!("completed"),
        "an interrupt that was over before this turn was asked for must not end it"
    );
}

// ---------------------------------------------------------------------------------------------
// Adversarial cases. Added by the adversary pass against a4c89b1; each is red on that commit.
// ---------------------------------------------------------------------------------------------

// The start window, decided on the server's behaviour.
// story:bridge-mode-interrupt-cases-are-deterministic and
// story:bridge-interrupt-start-window-under-load.
//
// The cases here pipeline `turn/start` and `turn/interrupt` at a turn that fails before it streams
// anything: an unusable budget, or a model client that will not build. Such a turn ends in
// microseconds, so against the shipped binary the client cannot choose which side of the turn's end
// the interrupt is decoded on. On a loaded machine the reading thread can be descheduled after it
// queues `turn/start` and before it decodes the interrupt, and the turn is over by then.
//
// Until 2026-10-30 these cases asked for 17 (or 18) of 20 attempts to be served inside the window,
// a rate taken from 596 hits in 600 attempts on a quiet machine. That rate failed correct servers:
// 16 of 20 twice, and 17 of 20 against 18 once, on 2026-10-20 while other builds ran; 15 and 16 of
// 20 in 2 of 8 runs on 2026-10-30 with the test pinned to one CPU (`taskset -c 0`, load average
// about 115 on 20 CPUs). Every miss had the late shape: `turn/completed` with `failed`, the
// interrupt answered after it.
//
// So the window is no longer left to the scheduler. The in-process cases drive the same
// `harness_app_server::serve` the binary calls, with three things in the test's hands that the
// binary takes from its process: the client's stdin, the server's stdout and the model factory.
// Those are the seam, and they need no change to the server. The test holds the server's main
// thread at a chosen point, hands the reading thread the interrupt, waits until the reading thread
// has decoded and queued it, and only then lets the main thread go on. Which side of the turn's end
// the interrupt is decoded on is then the test's choice, every attempt, under any load, and every
// remaining difference in the frames is the server's.
//
// The shipped binary keeps one case, which accepts each attempt in one of the only two orders a
// correct server produces and refuses every other, with no rate.

/// Where the interrupt is decoded, relative to the turn it is meant to stop.
#[derive(Debug, Clone, Copy)]
enum Arrival {
    /// Before the turn's control is installed. The reading thread finds no turn to cancel and
    /// leaves a count, which `turn/start` takes over (`Interrupts::stray`).
    BeforeInstall,
    /// While the turn is installed and its model client is being built. The reading thread cancels
    /// the running turn directly (`Watch::interrupted`).
    WhileInstalled,
}

/// How the turn fails. Both fail before a single event streams, so neither reaches the drain
/// between streamed events and only the settle after the loop can answer the interrupt.
#[derive(Debug, Clone, Copy)]
enum Failure {
    /// `Budget::validate` refuses a zero `max_turns`: the binary's `--max-turns 0`.
    BudgetRefused,
    /// The model factory returns an error: the binary's `--base-url ftp://not-a-wire`.
    ClientWillNotBuild,
}

/// What one in-process attempt showed.
struct Served {
    /// The answer to the interrupt arrived before `turn/completed`.
    acknowledged_before_terminal: bool,
    /// The turn was already cancelled when its model client was built.
    cancelled_when_built: bool,
    /// Every frame the server wrote, up to and including `turn/completed`.
    frames: Vec<Value>,
}

impl Served {
    fn status(&self) -> &Value {
        &self.frames.last().expect("a terminal frame")["params"]["turn"]["status"]
    }
}

/// The client's half of the server's stdin, handed over one chunk at a time.
///
/// Each time the reading thread has used up every chunk it was given and asks for more, the number
/// of chunks it has used up goes out on `used`. The reading thread decodes a line, fires its
/// interrupt watch and queues the frame before it reads again (`transport.rs`, `pump`), so a count
/// that covers a chunk proves every frame in that chunk was decoded and queued.
struct ScriptedInput {
    chunks: Receiver<Vec<u8>>,
    current: Vec<u8>,
    position: usize,
    taken: usize,
    used: Sender<usize>,
}

impl Read for ScriptedInput {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let available = self.fill_buf()?;
        let length = available.len().min(out.len());
        out[..length].copy_from_slice(&available[..length]);
        self.consume(length);
        Ok(length)
    }
}

impl BufRead for ScriptedInput {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        while self.position == self.current.len() {
            let _ = self.used.send(self.taken);
            match self.chunks.recv() {
                Ok(chunk) => {
                    self.current = chunk;
                    self.position = 0;
                    self.taken += 1;
                }
                // The client closed its input.
                Err(_) => return Ok(&[]),
            }
        }
        Ok(&self.current[self.position..])
    }

    fn consume(&mut self, amount: usize) {
        self.position += amount;
    }
}

/// The server's stdout, split into frames for the test, able to hold the server's main thread.
///
/// Holding is blocking the write of one named notification until the test releases it. The main
/// thread is the only one that writes and the only one that dequeues, so while it is held it takes
/// nothing off the queue: a `turn/start` already decoded behind it waits.
struct HeldOutput {
    pending: Vec<u8>,
    frames: Sender<Value>,
    hold: Option<(&'static str, Receiver<()>)>,
}

impl Write for HeldOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            let frame: Value = serde_json::from_slice(&line).map_err(std::io::Error::other)?;
            let held = self.hold.as_ref().is_some_and(|(method, _)| {
                frame.get("method").and_then(Value::as_str) == Some(*method)
            });
            let _ = self.frames.send(frame);
            if held && let Some((_, release)) = self.hold.take() {
                let _ = release.recv_timeout(READ_TIMEOUT);
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The model of a turn whose budget is refused before the model is asked anything.
struct Unreached(WireId);

impl ModelPort for Unreached {
    fn wire(&self) -> &WireId {
        &self.0
    }

    fn turn(
        &mut self,
        _request: &TurnRequest,
        _sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        unreachable!("a zero `max_turns` is refused before the model is asked anything")
    }
}

/// The model factory an in-process server is handed: the seam the binary fills with `model_client`.
type Factory = Box<dyn FnMut(Cancel) -> Result<Box<dyn ModelPort>, String> + Send>;

/// The test's end of an in-process connection.
struct InProcess {
    chunks: Sender<Vec<u8>>,
    sent: usize,
    used: Receiver<usize>,
    frames: Receiver<Value>,
    seen: Vec<Value>,
    server: thread::JoinHandle<Result<(), String>>,
}

impl InProcess {
    /// Serves one connection on its own thread, as the binary serves its stdin and stdout.
    ///
    /// `hold` names a notification the server's main thread is held at once it writes it, until
    /// the paired sender sends or is dropped.
    fn serve(budget: Budget, hold: Option<(&'static str, Receiver<()>)>, factory: Factory) -> Self {
        let (chunk_sender, chunks) = channel::<Vec<u8>>();
        let (used_sender, used) = channel::<usize>();
        let (frame_sender, frames) = channel::<Value>();
        let input = ScriptedInput {
            chunks,
            current: Vec::new(),
            position: 0,
            taken: 0,
            used: used_sender,
        };
        let output = HeldOutput {
            pending: Vec::new(),
            frames: frame_sender,
            hold,
        };
        let config = ServerConfig {
            model: "b10x-emulated".to_owned(),
            budget,
            context_window: None,
            version: "test".to_owned(),
        };
        let server = thread::spawn(move || {
            let mut factory = factory;
            harness_app_server::serve(&config, &mut *factory, input, output)
                .map_err(|error| error.to_string())
        });
        Self {
            chunks: chunk_sender,
            sent: 0,
            used,
            frames,
            seen: Vec::new(),
            server,
        }
    }

    /// Hands the reading thread one chunk of frames and returns the chunk's number.
    fn send(&mut self, frames: &[Value]) -> usize {
        let mut chunk = String::new();
        for frame in frames {
            chunk.push_str(&frame.to_string());
            chunk.push('\n');
        }
        self.chunks
            .send(chunk.into_bytes())
            .expect("the server's reading thread is alive");
        self.sent += 1;
        self.sent
    }

    /// Waits until the reading thread has decoded and queued every frame of chunk `number`.
    fn decoded(&self, number: usize, what: &str) -> Result<(), String> {
        loop {
            match self.used.recv_timeout(READ_TIMEOUT) {
                Ok(used) if used >= number => return Ok(()),
                Ok(_) => {}
                Err(_) => {
                    return Err(format!(
                        "the reading thread did not decode {what} within {READ_TIMEOUT:?}"
                    ));
                }
            }
        }
    }

    fn next(&mut self, what: &str) -> Result<Value, String> {
        match self.frames.recv_timeout(READ_TIMEOUT) {
            Ok(frame) => {
                self.seen.push(frame.clone());
                Ok(frame)
            }
            Err(_) => Err(format!(
                "waiting for {what}, the server wrote nothing within {READ_TIMEOUT:?} after {:?}",
                self.seen
            )),
        }
    }

    fn response(&mut self, id: i64) -> Result<Value, String> {
        loop {
            let frame = self.next(&format!("the answer to request {id}"))?;
            if frame.get("id") == Some(&json!(id)) && frame.get("method").is_none() {
                return Ok(frame);
            }
        }
    }

    /// The handshake and `thread/start`, in one chunk. Returns the thread's id.
    fn open_thread(&mut self) -> Result<String, String> {
        self.send(&[
            json!({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "test"}}}),
            json!({"method": "initialized", "params": {}}),
            json!({"id": 2, "method": "thread/start", "params": {}}),
        ]);
        let thread = self.response(2)?;
        thread["result"]["thread"]["id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("thread/start answered without an id: {thread}"))
    }

    /// Reads to `turn/completed`; `true` when the answer to `id` came before it.
    fn read_turn_watching_for(&mut self, id: i64) -> Result<bool, String> {
        let mut answered = false;
        loop {
            let frame = self.next("`turn/completed`")?;
            if frame.get("id") == Some(&json!(id)) && frame.get("method").is_none() {
                answered = true;
            }
            if frame.get("method").and_then(Value::as_str) == Some("turn/completed") {
                return Ok(answered);
            }
        }
    }

    /// Closes the input and requires the connection to end cleanly. Returns every frame written.
    fn finish(self) -> Result<Vec<Value>, String> {
        let Self {
            chunks,
            seen,
            server,
            ..
        } = self;
        // Closing the input ends the connection, and a connection that ends that way is no error.
        drop(chunks);
        let deadline = Instant::now() + READ_TIMEOUT;
        while !server.is_finished() {
            if Instant::now() > deadline {
                return Err(format!(
                    "the server did not end within {READ_TIMEOUT:?} of its input closing: {seen:?}"
                ));
            }
            thread::sleep(Duration::from_millis(1));
        }
        match server.join() {
            Ok(Ok(())) => Ok(seen),
            Ok(Err(error)) => Err(format!(
                "the connection ended with an error: {error}; {seen:?}"
            )),
            Err(_) => Err(format!("the server thread panicked after {seen:?}")),
        }
    }
}

fn turn_start(thread_id: &str) -> Value {
    json!({"id": 50, "method": "turn/start", "params": {
        "threadId": thread_id, "input": [{"type": "text", "text": "go"}],
    }})
}

fn turn_interrupt(thread_id: &str) -> Value {
    json!({"id": 99, "method": "turn/interrupt", "params": {"threadId": thread_id}})
}

/// Pipelines `turn/start` and `turn/interrupt` at an in-process server whose turn fails as
/// `failure` says, with the interrupt decoded at `arrival`, and reads the turn to its terminal
/// frame.
///
/// `Err` names the step the server never reached, with the frames it did write.
fn pipelined_interrupt(failure: Failure, arrival: Arrival) -> Result<Served, String> {
    let (release_output, output_released) = channel::<()>();
    let (entered_sender, entered) = channel::<()>();
    let (release_factory, factory_released) = channel::<()>();
    let (cancelled_sender, cancelled) = channel::<bool>();

    let mut client = InProcess::serve(
        match failure {
            Failure::BudgetRefused => Budget::default().with_max_turns(0),
            Failure::ClientWillNotBuild => Budget::default(),
        },
        matches!(arrival, Arrival::BeforeInstall).then_some(("thread/started", output_released)),
        Box::new(
            move |cancel: Cancel| -> Result<Box<dyn ModelPort>, String> {
                let _ = entered_sender.send(());
                let _ = factory_released.recv_timeout(READ_TIMEOUT);
                let _ = cancelled_sender.send(cancel.is_cancelled());
                match failure {
                    Failure::BudgetRefused => Ok(Box::new(Unreached(
                        WireId::new("openai-responses").expect("a valid wire id"),
                    ))),
                    Failure::ClientWillNotBuild => {
                        Err("the model client will not build".to_owned())
                    }
                }
            },
        ),
    );
    let thread_id = client.open_thread()?;

    match arrival {
        Arrival::BeforeInstall => {
            // The main thread is held writing `thread/started`, before it can dequeue `turn/start`.
            // Both frames are decoded behind it, in the order a pipelining client sends them.
            let _ = release_factory.send(());
            client.send(&[turn_start(&thread_id)]);
            let number = client.send(&[turn_interrupt(&thread_id)]);
            client.decoded(
                number,
                "the interrupt while the server was held at `thread/started`",
            )?;
            let _ = release_output.send(());
        }
        Arrival::WhileInstalled => {
            // The main thread is held building the turn's model client, inside `drive_turn`, so the
            // turn is installed and has not ended.
            client.send(&[turn_start(&thread_id)]);
            entered.recv_timeout(READ_TIMEOUT).map_err(|_| {
                format!(
                    "the server never built the turn's model client: {:?}",
                    client.seen
                )
            })?;
            let number = client.send(&[turn_interrupt(&thread_id)]);
            client.decoded(
                number,
                "the interrupt while the turn's model client was being built",
            )?;
            let _ = release_factory.send(());
        }
    }

    let cancelled_when_built = cancelled
        .recv_timeout(READ_TIMEOUT)
        .map_err(|_| format!("the model factory never returned: {:?}", client.seen))?;
    let acknowledged_before_terminal = client.read_turn_watching_for(99)?;
    let frames = client.seen.clone();
    client.finish()?;

    Ok(Served {
        acknowledged_before_terminal,
        cancelled_when_built,
        frames,
    })
}

/// Runs `failure` at both arrivals and hands each to `check`, naming the arrival in any red.
///
/// `check` returns `Some(cause)` when the server got that attempt wrong.
fn at_both_arrivals(failure: Failure, check: impl Fn(&Served) -> Option<String>) {
    for arrival in [Arrival::BeforeInstall, Arrival::WhileInstalled] {
        let served = pipelined_interrupt(failure, arrival)
            .unwrap_or_else(|cause| panic!("{failure:?}, interrupt decoded {arrival:?}: {cause}"));
        assert!(
            served.cancelled_when_built,
            "{failure:?}, interrupt decoded {arrival:?}: the interrupt was decoded before the \
             turn's model client was built, and the turn was still not cancelled by then \
             (story:bridge-mode-interrupt-cases-are-deterministic). Frames: {:?}",
            served.frames
        );
        if let Some(cause) = check(&served) {
            panic!(
                "{failure:?}, interrupt decoded {arrival:?}: {cause} \
                 (story:bridge-mode-interrupt-cases-are-deterministic). Frames: {:?}",
                served.frames
            );
        }
    }
}

/// Reads until `turn/completed`, returning (the answer to `id` if it came first, the frames seen).
fn read_turn_watching_for(bridge: &mut Bridge, id: i64) -> (Option<Value>, Vec<Value>) {
    let mut answer = None;
    let mut frames = Vec::new();
    loop {
        let frame = bridge.next_frame();
        if frame.get("id") == Some(&json!(id)) && frame.get("method").is_none() {
            answer = Some(frame);
            continue;
        }
        let terminal = frame.get("method").and_then(Value::as_str) == Some("turn/completed");
        frames.push(frame);
        if terminal {
            return (answer, frames);
        }
    }
}

#[test]
fn an_interrupt_is_answered_before_the_terminal_frame_even_when_the_turn_errors() {
    // `Wire::settle_interrupts` was once called after `AgentLoop::run(..).map_err(..)?`, so every
    // `LoopError` skipped the settle and the turn wrote its terminal frame owing an answer.
    //
    // A zero `max_turns` reaches that arm deterministically: `Budget::validate` rejects it in
    // `drive_run`, before `stop_before_turn` looks at the cancel token, so a turn cancelled before
    // it began still ends as `Err(LoopError::Budget)`. The everyday member of the same class is
    // `LoopError::Wire`, an expired credential or a 5xx racing a person's stop.
    //
    // What the settle exists to guarantee, in its own words: "an acknowledgement read after
    // `turn/completed` is a receipt, not an acknowledgement." Decided at both arrivals, every run.
    at_both_arrivals(Failure::BudgetRefused, |served| {
        (!served.acknowledged_before_terminal)
            .then(|| "the interrupt was not answered before the turn's terminal frame".to_owned())
    });
}

#[test]
fn an_interrupt_that_cancelled_a_turn_decides_its_status_even_when_the_turn_errors() {
    // Same arm, second consequence. `drive_turn` once read `control.requested` after the `?`, so a
    // turn the reading thread cancelled was reported `failed` with the error's message, and the
    // client was separately told its interrupt succeeded. "An interrupt that was actually asked for
    // is the reason this turn ended."
    at_both_arrivals(Failure::BudgetRefused, |served| {
        (served.status() != &json!("interrupted")).then(|| {
            format!(
                "a turn the interrupt cancelled reports {} rather than `interrupted`",
                served.status()
            )
        })
    });
}

#[test]
fn stopping_a_turn_and_starting_the_next_in_one_breath_runs_the_next_one() {
    // The "stop and resend" shape a person produces by hitting stop and typing again: three frames
    // on the wire before any answer is read. `Wire::settle_interrupts` pulls frames off the queue
    // until the turn owes nothing, so it is the thing that could eat the second `turn/start` and
    // refuse it with `turn/start cannot be served while a turn is running` -- for a turn that has
    // already ended.
    let endpoint = Endpoint::start("slow");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));

    bridge.write(&json!({"id": 50, "method": "turn/start", "params": {
        "threadId": thread_id, "input": [{"type": "text", "text": "go"}],
    }}));
    bridge.write(&json!({"id": 99, "method": "turn/interrupt", "params": {
        "threadId": thread_id,
    }}));
    bridge.write(&json!({"id": 51, "method": "turn/start", "params": {
        "threadId": thread_id, "input": [{"type": "text", "text": "instead, this"}],
    }}));

    let mut second_turn = None;
    let mut terminals = Vec::new();
    loop {
        let frame = bridge.next_frame();
        if frame.get("id") == Some(&json!(51)) && frame.get("method").is_none() {
            second_turn = Some(frame.clone());
        }
        if frame.get("method").and_then(Value::as_str) == Some("turn/completed") {
            terminals.push(frame);
            if terminals.len() == 2 {
                break;
            }
        }
    }
    let second_turn = second_turn.expect("the second `turn/start` is answered");
    assert!(
        second_turn["error"].is_null(),
        "a `turn/start` sent behind an interrupt must still start a turn: {second_turn}"
    );
    assert_eq!(
        terminals[0]["params"]["turn"]["status"],
        json!("interrupted"),
        "{terminals:?}"
    );
    assert_eq!(
        terminals[1]["params"]["turn"]["status"],
        json!("completed"),
        "the interrupt that stopped the first turn must not reach the second: {terminals:?}"
    );
}

#[test]
fn two_interrupts_sent_between_turns_do_not_reach_the_next_one() {
    // `Interrupts::stray` is a count, not a token, and `Server::request` takes it down by one per
    // frame it answers while `Server::start_turn` takes the whole count over. Two frames before a
    // turn is the smallest case where one decrement is not the same as taking the count to zero.
    // Written without waiting for either acknowledgement, so both frames are on the wire before
    // the server has dequeued either.
    let endpoint = Endpoint::start("text");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));

    bridge.write(&json!({"id": 98, "method": "turn/interrupt", "params": {"threadId": thread_id}}));
    bridge.write(&json!({"id": 99, "method": "turn/interrupt", "params": {"threadId": thread_id}}));
    bridge.write(&json!({"id": 50, "method": "turn/start", "params": {
        "threadId": thread_id, "input": [{"type": "text", "text": "say something"}],
    }}));

    let (_, frames) = read_turn_watching_for(&mut bridge, 50);
    assert_eq!(
        frames.last().expect("a terminal frame")["params"]["turn"]["status"],
        json!("completed"),
        "interrupts that were over before this turn was asked for must not end it: {frames:?}"
    );
}

#[test]
fn a_turn_cancelled_before_it_started_streams_nothing() {
    // What `Interrupts::stray` and its handover in `Server::start_turn` are *for*, and the only
    // thing they do that `Wire::serve_control` does not already do on its own. With the handover,
    // the cancel is set before `drive_turn` is called at all, so `AgentLoop`'s `stop_before_turn`
    // ends the run before the first model request and the client sees no text. Without it, the
    // turn runs until the main thread happens to reach the queue between two streamed events —
    // the client is told its interrupt succeeded *and* handed part of the answer it cancelled,
    // and the model was paid for producing it.
    //
    // Forcing `carried` to zero in `start_turn` leaves every other case in this file green.
    let endpoint = Endpoint::start("slow");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));

    bridge.write(&json!({"id": 50, "method": "turn/start", "params": {
        "threadId": thread_id, "input": [{"type": "text", "text": "go"}],
    }}));
    bridge.write(&json!({"id": 99, "method": "turn/interrupt", "params": {
        "threadId": thread_id,
    }}));

    let (_, frames) = read_turn_watching_for(&mut bridge, 99);
    assert!(
        !methods(&frames).contains(&"item/agentMessage/delta"),
        "a turn cancelled before it was announced must not stream: {frames:?}"
    );
}

#[test]
fn an_interrupted_turn_ends_without_waiting_out_the_settle_bound() {
    // `Wire::settle_interrupts` blocks on `INTERRUPT_SETTLE_TIMEOUT` (10s) whenever
    // `TurnControl::owes` says a frame is still owed, and swallows the timeout into
    // `sink.broken` — which `control.requested` then overrides. So an accounting error in
    // `TurnControl::owed` costs every interrupted turn ten silent seconds and changes no frame.
    //
    // Measured: making `TurnControl::answered` not decrement the count takes
    // `an_acknowledged_interrupt_never_also_delivers_the_answer` and
    // `an_interrupt_stops_a_turn_that_is_blocked_on_the_model` from 1.63s to 21.62s together, and
    // both stay green — the existing `READ_TIMEOUT` of 20s is per frame, not per turn.
    //
    // The bound here is a fifth of that timeout, and the shipped path answers in about a second.
    let endpoint = Endpoint::start("slow");
    let mut bridge = Bridge::start(&endpoint, &[]);
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));
    let turn_id = bridge.start_turn(&thread_id, "take your time");
    bridge.skip_to("item/agentMessage/delta");

    let sent = std::time::Instant::now();
    bridge.write(&json!({"id": 99, "method": "turn/interrupt", "params": {
        "threadId": thread_id, "turnId": turn_id,
    }}));
    let (acknowledgement, frames) = read_turn_watching_for(&mut bridge, 99);
    let elapsed = sent.elapsed();

    assert!(
        acknowledgement.is_some(),
        "the interrupt is answered before the terminal frame: {frames:?}"
    );
    assert_eq!(
        frames.last().expect("a terminal frame")["params"]["turn"]["status"],
        json!("interrupted")
    );
    assert!(
        elapsed < Duration::from_secs(8),
        "an interrupted turn waited {elapsed:?} for a frame it had already been given"
    );
}

#[test]
fn an_interrupt_is_answered_before_the_terminal_frame_when_the_model_client_will_not_build() {
    // The `?` on `AgentLoop::run` had a sibling one line above it, on the model factory, with the
    // same symptom and the same cause: `drive_turn` returned before it settled what the turn owed.
    // The shipped binary reaches it with nothing but a flag (`llm_providers::BaseUrl::new` refuses
    // a base URL that is not absolute http or https, and `model_client` runs per turn), and
    // `the_shipped_binary_answers_a_pipelined_interrupt_in_one_of_two_orders` drives that path.
    //
    // Observed on the binary before that fix, in this order: `turn/completed` with
    // `status: "failed"`, and only then `{"id":99,"result":{}}`.
    at_both_arrivals(Failure::ClientWillNotBuild, |served| {
        if !served.acknowledged_before_terminal {
            Some("the interrupt was not answered before the turn's terminal frame".to_owned())
        } else if served.status() != &json!("interrupted") {
            Some(format!(
                "a turn the interrupt cancelled reports {} rather than `interrupted`",
                served.status()
            ))
        } else {
            None
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Second adversarial pass, against 329e2c3.
// ---------------------------------------------------------------------------------------------

/// How one shape of the shipped binary is started.
type StartBridge = fn(&Endpoint) -> Bridge;

/// One pipelined attempt at the shipped binary, for a turn that fails at once.
///
/// Returns which of the two orders a correct server produces the attempt took, or the cause when it
/// took neither:
///
/// - **in window**: the interrupt was decoded before the turn ended. It is answered before
///   `turn/completed`, and the turn reports `interrupted`.
/// - **late**: the interrupt was decoded after the turn ended. The turn reports how it ended,
///   `failed`, and the interrupt is still answered, after `turn/completed`.
///
/// Every other combination is the server's: an answer before the terminal frame with any status
/// but `interrupted`, an `interrupted` turn whose interrupt was not answered first, or an interrupt
/// never answered at all.
fn binary_attempt(bridge: &mut Bridge) -> Result<&'static str, String> {
    bridge.handshake();
    let thread_id = bridge.start_thread(&json!({}));
    bridge.write_pipelined(
        &json!({"id": 50, "method": "turn/start", "params": {
            "threadId": thread_id, "input": [{"type": "text", "text": "go"}],
        }}),
        &json!({"id": 99, "method": "turn/interrupt", "params": {
            "threadId": thread_id,
        }}),
    );
    let (acknowledgement, frames) = read_turn_watching_for(bridge, 99);
    let status = frames.last().expect("a terminal frame")["params"]["turn"]["status"].clone();
    match (acknowledgement, status.as_str()) {
        (Some(answer), Some("interrupted")) if answer["error"].is_null() => Ok("in window"),
        (Some(answer), Some("interrupted")) => {
            Err(format!("the interrupt was refused: {answer}; {frames:?}"))
        }
        (Some(_), _) => Err(format!(
            "the interrupt was answered before the terminal frame, so it was acted on, yet the \
             turn reports {status}: {frames:?}"
        )),
        (None, Some("failed")) => {
            let late = bridge.next_frame();
            if late.get("id") == Some(&json!(99)) && late["error"].is_null() {
                Ok("late")
            } else {
                Err(format!(
                    "after a turn that ended without its interrupt, the next frame was {late} \
                     rather than the interrupt's answer: {frames:?}"
                ))
            }
        }
        (None, _) => Err(format!(
            "the turn reports {status} while the interrupt was still unanswered at its terminal \
             frame: {frames:?}"
        )),
    }
}

#[test]
fn the_shipped_binary_answers_a_pipelined_interrupt_in_one_of_two_orders() {
    // Was `the_start_window_is_reached_on_almost_every_attempt`, which asked for 18 of 20 attempts
    // in window and failed correct servers under load
    // (story:bridge-mode-interrupt-cases-are-deterministic). Whether an attempt lands in window is
    // the scheduler's; the cases above decide the window itself in process. What the binary still
    // has to show is that both its flags reach the paths those cases drive, and that no attempt,
    // whichever side it lands on, is answered in an order a correct server never produces. No
    // rate: one attempt out of order is red, and the red names it.
    const ATTEMPTS: usize = 20;
    let endpoint = Endpoint::start("text");
    let shapes: [(&str, StartBridge); 2] = [
        ("--max-turns 0", |endpoint| {
            Bridge::start(endpoint, &["--max-turns", "0"])
        }),
        ("--base-url ftp://not-a-wire", |_| {
            Bridge::start_at("ftp://not-a-wire", &[])
        }),
    ];
    for (shape, start) in shapes {
        let mut orders = Vec::new();
        for attempt in 1..=ATTEMPTS {
            let outcome = catch_unwind(AssertUnwindSafe(|| binary_attempt(&mut start(&endpoint))))
                .unwrap_or_else(|panic| {
                    Err(panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                        .unwrap_or_else(|| "the attempt panicked".to_owned()))
                });
            match outcome {
                Ok(order) => orders.push(order),
                Err(cause) => panic!(
                    "{shape}, attempt {attempt} of {ATTEMPTS}: {cause} \
                     (story:bridge-mode-interrupt-cases-are-deterministic; earlier attempts: \
                     {orders:?})"
                ),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The end of the turn. Found by story:bridge-mode-interrupt-cases-are-deterministic on 2026-10-30.
// ---------------------------------------------------------------------------------------------

/// One in-process attempt at a turn whose model client will not build, with the interrupt sent
/// `delay` after `turn/start`.
///
/// Returns which of the two orders a correct server produces the attempt took (see
/// [`binary_attempt`]), or the cause when it took neither.
fn interrupt_after(delay: Duration) -> Result<&'static str, String> {
    let mut client = InProcess::serve(
        Budget::default(),
        None,
        Box::new(|_cancel| Err("the model client will not build".to_owned())),
    );
    let thread_id = client.open_thread()?;
    client.send(&[turn_start(&thread_id)]);
    let sent = Instant::now();
    while sent.elapsed() < delay {
        std::hint::spin_loop();
    }
    client.send(&[turn_interrupt(&thread_id)]);
    let answered_first = client.read_turn_watching_for(99)?;
    let status = client.seen.last().expect("a terminal frame")["params"]["turn"]["status"].clone();
    let order = match (answered_first, status.as_str()) {
        (true, Some("interrupted")) => "in window",
        (true, _) => {
            return Err(format!(
                "the interrupt was answered before the terminal frame, so it was acted on, yet \
                 the turn reports {status}: {:?}",
                client.seen
            ));
        }
        (false, Some("failed")) => {
            let late = client.next("the late interrupt's answer")?;
            if late.get("id") != Some(&json!(99)) || !late["error"].is_null() {
                return Err(format!(
                    "after a turn that ended without its interrupt, the next frame was {late} \
                     rather than the interrupt's answer"
                ));
            }
            "late"
        }
        (false, _) => {
            return Err(format!(
                "the turn reports {status} while its interrupt was still unanswered at its \
                 terminal frame: {:?}",
                client.seen
            ));
        }
    };
    client.finish()?;
    Ok(order)
}

#[test]
fn an_interrupt_decoded_as_its_turn_ends_is_answered_in_one_of_two_orders() {
    // `drive_turn` settled what its turn owed and only then let go of the turn, so the reading
    // thread could still count an interrupt against a turn whose settle was already over. Decoded
    // between the settle's last look at `owed` and the read of `requested`, it made the turn
    // `interrupted` and was answered after `turn/completed`: a receipt, on the one turn its own
    // interrupt decided. Measured on 2026-10-30 with this attempt swept across the end of the
    // turn: 11 and 28 of 20000 attempts in two sweeps on the server before the fix, 0 of 20000
    // after it. The fix lets go of the turn first, under the lock the reading thread counts under,
    // so the count the settle waits for is final before the settle starts.
    //
    // No rate. A correct server takes one of the two orders on every attempt, under any load; the
    // sweep only decides how many attempts land near the end of the turn. Its 100 microseconds
    // cover where that end fell on the machine it was measured on (misses at 27 to 73 µs), and
    // this case was red in 5 of 5 suite runs before the fix, each red naming its attempt.
    const ATTEMPTS: u32 = 4000;
    const STEP: Duration = Duration::from_nanos(25);
    let mut orders = std::collections::BTreeMap::<&str, u32>::new();
    for attempt in 1..=ATTEMPTS {
        let delay = STEP * (attempt % 4000);
        match interrupt_after(delay) {
            Ok(order) => *orders.entry(order).or_default() += 1,
            Err(cause) => panic!(
                "attempt {attempt} of {ATTEMPTS}, interrupt sent {delay:?} after `turn/start`: \
                 {cause} (story:bridge-mode-interrupt-cases-are-deterministic; earlier attempts: \
                 {orders:?})"
            ),
        }
    }
}
