//! The shipped binary driven the way `runtime/agent`'s bridge drives Codex.
//!
//! This test *is* the client: it writes the pinned client methods to the process's stdin and reads
//! the pinned notifications back. If it passes, the same frames the existing bridge sends produce
//! the frames it expects — which is what makes bridge mode reuse rather than a second integration.
//!
//! It is not a substitute for running the real bridge against this binary. That crosses a component
//! boundary and has not been done; `STATUS.md` says so.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread;
use std::time::Duration;

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
        let mut arguments = vec![
            "app-server",
            "--base-url",
            &endpoint.base_url,
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

    // Same nested-borrow path, reached by a different frame.
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
