//! The whole stack against a real socket.
//!
//! Everything below runs the actual HTTP client, the actual SSE reader and the actual loop against
//! a deterministic local endpoint. That is what separates this from the unit tests: those prove the
//! projection in isolation, this proves the pieces agree with each other over a wire.
//!
//! This is `provider_emulated` evidence. It says nothing about how a real provider behaves.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use b10x_harness_responses::{Endpoint, ResponsesClient, WIRE};
use harness_loop::{AgentLoop, ApproveAll, Budget, LoopConfig, LoopEvent, LoopStop, VecLoopSink};
use harness_wire::{
    Envelope, Item, StaticBearer, ToolCall, ToolName, ToolOutcome, ToolPort, ToolSpec,
    WireErrorCode,
};
use serde_json::{Value, json};

/// The fixture endpoint, shut down when this is dropped.
struct Fixture {
    child: Child,
    base_url: String,
    record: PathBuf,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn start(scenario: &str) -> Self {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let record = dir.path().join("requests.jsonl");
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("fake_responses.py");
        let mut child = Command::new("python3")
            .arg(&script)
            .arg("--scenario")
            .arg(scenario)
            .arg("--record")
            .arg(&record)
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
            record,
            _dir: dir,
        }
    }

    fn client(&self) -> ResponsesClient {
        let endpoint = Endpoint::new(&self.base_url, "daemonloom-emulated", 32_000)
            .expect("the fixture endpoint is well formed");
        ResponsesClient::new(endpoint, Arc::new(StaticBearer::new("test-key")))
            .expect("the client builds")
    }

    /// Every request the endpoint received, in order.
    fn requests(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.record)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("each record is JSON"))
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A tool port that answers one known tool and records every call.
#[derive(Default)]
struct TestTools {
    specs: Vec<ToolSpec>,
    calls: Vec<ToolCall>,
}

impl TestTools {
    fn with_read() -> Self {
        Self {
            specs: vec![ToolSpec {
                name: ToolName::new("workspace_read").expect("valid"),
                description: "reads one file".to_owned(),
                envelope: Envelope::default(),
                input_schema: json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"],
                }),
                approval: harness_wire::Approval::NotRequired,
            }],
            calls: Vec::new(),
        }
    }
}

impl ToolPort for TestTools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        self.calls.push(call.clone());
        ToolOutcome::ok(json!({"text": "hello harness"}))
    }
}

fn run(
    scenario: &str,
    tools: &mut TestTools,
) -> (
    Fixture,
    Result<harness_loop::LoopOutcome, harness_loop::LoopError>,
    VecLoopSink,
) {
    let fixture = Fixture::start(scenario);
    let mut client = fixture.client();
    let mut approvals = ApproveAll;
    let mut sink = VecLoopSink::new();
    let config = LoopConfig::new("daemonloom-emulated", "be useful")
        .with_budget(Budget::default().with_max_turns(5));
    let outcome = AgentLoop::new(&mut client, tools, &mut approvals, config)
        .run("read the readme", &mut sink);
    (fixture, outcome, sink)
}

#[test]
fn a_text_answer_arrives_streamed_over_a_real_socket() {
    let mut tools = TestTools::default();
    let (fixture, outcome, sink) = run("text", &mut tools);
    let outcome = outcome.expect("the run completes");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.text, "provider emulation passed");
    assert_eq!(sink.text(), "provider emulation passed");
    // Two deltas were sent; the loop must have seen them separately rather than one blob at the end.
    assert_eq!(
        sink.events()
            .iter()
            .filter(|event| matches!(event, LoopEvent::TextDelta { .. }))
            .count(),
        2
    );
    assert_eq!(outcome.total_tokens(), Some((42, 4)));
    assert_eq!(fixture.requests().len(), 1);
}

#[test]
fn the_request_is_stateless_authenticated_and_asks_for_reasoning() {
    let mut tools = TestTools::with_read();
    let (fixture, outcome, _) = run("text", &mut tools);
    outcome.expect("the run completes");

    let requests = fixture.requests();
    let first = &requests[0];
    assert_eq!(first["store"], json!(false), "the conversation is ours");
    assert_eq!(first["stream"], json!(true));
    assert_eq!(first["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(first["accept"], json!("text/event-stream"));
    assert_eq!(first["has_authorization"], json!(true));
    assert_eq!(first["instructions"], json!("be useful"));
    assert_eq!(first["tool_names"], json!(["workspace_read"]));
}

#[test]
fn a_tool_round_trip_completes_over_the_wire() {
    let mut tools = TestTools::with_read();
    // `dynamic-tool`: the emulator calls this test's own tool by name. The provider is the wire, and
    // the wire does not know about catalogues — whatever the caller published is what comes back.
    let (fixture, outcome, sink) = run("dynamic-tool", &mut tools);
    let outcome = outcome.expect("the round trip completes");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.turns, 2);
    assert_eq!(outcome.text, "The file says: hello harness");
    assert_eq!(tools.calls.len(), 1);
    assert_eq!(tools.calls[0].arguments, json!({"path": "README.md"}));

    // The streamed argument fragment must be correlated to the call a reader is watching.
    assert!(
        sink.events().iter().any(|event| matches!(
            event,
            LoopEvent::ToolArgumentsDelta { call_id, .. } if call_id.as_str() == "call_b10x_001"
        )),
        "{:?}",
        sink.events()
    );

    // The second request must carry the call and its result, projected into the wire's shapes.
    let requests = fixture.requests();
    assert_eq!(requests.len(), 2);
    let input = requests[1]["input"].as_array().expect("an input array");
    let kinds: Vec<&str> = input
        .iter()
        .filter_map(|entry| entry["type"].as_str())
        .collect();
    assert!(kinds.contains(&"function_call"), "{kinds:?}");
    assert!(kinds.contains(&"function_call_output"), "{kinds:?}");
    let output = input
        .iter()
        .find(|entry| entry["type"] == json!("function_call_output"))
        .expect("the result is replayed");
    assert_eq!(output["call_id"], json!("call_b10x_001"));
    assert!(
        output["output"]
            .as_str()
            .expect("the output is a string")
            .contains("hello harness"),
        "{output}"
    );
}

#[test]
fn a_reasoning_item_is_replayed_verbatim_on_the_next_turn() {
    let mut tools = TestTools::with_read();
    let (fixture, outcome, _) = run("reasoning", &mut tools);
    let outcome = outcome.expect("the run completes");

    // Held as opaque, tagged with the wire that produced it.
    assert!(
        outcome.items.iter().any(|item| matches!(
            item,
            Item::Opaque { wire, payload }
                if wire.as_str() == WIRE
                    && payload["encrypted_content"] == json!("OPAQUE-REASONING-BLOB")
        )),
        "{:?}",
        outcome.items
    );

    // And actually sent back, byte for byte. Without this the model re-derives its plan every
    // tool call, because nothing is retained provider-side under `store: false`.
    let requests = fixture.requests();
    let replayed = requests[1]["replayed_reasoning"]
        .as_array()
        .expect("an array");
    assert_eq!(replayed.len(), 1, "{:?}", requests[1]);
    assert_eq!(
        replayed[0]["encrypted_content"],
        json!("OPAQUE-REASONING-BLOB")
    );
}

#[test]
fn a_call_to_an_unpublished_tool_is_refused_and_the_run_recovers() {
    let mut tools = TestTools::with_read();
    let (_fixture, outcome, sink) = run("unpublished-tool", &mut tools);
    let outcome = outcome.expect("the run recovers");

    assert!(tools.calls.is_empty(), "an unpublished tool must not run");
    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(
        sink.warnings().map(|(code, _)| code).collect::<Vec<_>>(),
        vec!["unpublished-tool"]
    );
}

#[test]
fn a_rejected_credential_is_reported_as_unauthorized() {
    let mut tools = TestTools::default();
    let (_fixture, outcome, _) = run("unauthorized", &mut tools);
    let harness_loop::LoopError::Wire(error) = outcome.expect_err("401 refuses") else {
        panic!("an HTTP status maps to a wire error");
    };
    assert_eq!(error.code, WireErrorCode::Unauthorized);
    assert!(!error.retriable, "retrying a rejected key changes nothing");
}

#[test]
fn a_cold_gateway_is_retriable_transport_rather_than_a_refusal() {
    let mut tools = TestTools::default();
    let (_fixture, outcome, _) = run("cold", &mut tools);
    let harness_loop::LoopError::Wire(error) = outcome.expect_err("503 refuses") else {
        panic!("an HTTP status maps to a wire error");
    };
    assert_eq!(error.code, WireErrorCode::Transport);
    assert!(
        error.retriable,
        "a starting backend is worth another attempt"
    );
}

#[test]
fn a_malformed_event_refuses_as_protocol() {
    let mut tools = TestTools::default();
    let (_fixture, outcome, _) = run("malformed", &mut tools);
    let harness_loop::LoopError::Wire(error) = outcome.expect_err("bad framing refuses") else {
        panic!("a framing failure maps to a wire error");
    };
    assert_eq!(error.code, WireErrorCode::Protocol);
}

#[test]
fn a_truncated_stream_is_never_read_as_a_completion() {
    let mut tools = TestTools::default();
    let (_fixture, outcome, _) = run("truncated", &mut tools);
    let harness_loop::LoopError::Wire(error) = outcome.expect_err("truncation refuses") else {
        panic!("a truncated stream maps to a wire error");
    };
    assert_eq!(error.code, WireErrorCode::Protocol);
}

#[test]
fn a_provider_failure_carries_its_own_reason() {
    let mut tools = TestTools::default();
    let (_fixture, outcome, _) = run("failed", &mut tools);
    let harness_loop::LoopError::Wire(error) = outcome.expect_err("a failed response refuses")
    else {
        panic!("a provider failure maps to a wire error");
    };
    assert_eq!(error.code, WireErrorCode::Refused);
    assert!(
        error.message.contains("upstream exploded"),
        "{}",
        error.message
    );
}

#[test]
fn a_cut_off_turn_is_reported_rather_than_passed_off_as_an_answer() {
    let mut tools = TestTools::default();
    let (_fixture, outcome, _) = run("incomplete", &mut tools);
    let outcome = outcome.expect("an incomplete turn is still an outcome");
    assert_eq!(
        outcome.stop,
        LoopStop::ProviderIncomplete {
            reason: "max_output_tokens".to_owned()
        }
    );
    assert!(!outcome.stop.is_completed());
}

#[test]
fn an_endpoint_that_reports_no_usage_leaves_usage_unknown() {
    let mut tools = TestTools::default();
    let (_fixture, outcome, sink) = run("no-usage", &mut tools);
    let outcome = outcome.expect("the run completes");
    assert_eq!(outcome.text, "no usage");
    assert_eq!(outcome.total_tokens(), None, "absent is not zero");
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| matches!(event, LoopEvent::Usage(_)))
    );
}

#[test]
fn unknown_events_and_items_are_warned_about_and_preserved() {
    let mut tools = TestTools::default();
    let (_fixture, outcome, sink) = run("unknown-events", &mut tools);
    let outcome = outcome.expect("the run completes");

    let codes: Vec<&str> = sink.warnings().map(|(code, _)| code).collect();
    assert!(codes.contains(&"unknown-stream-event"), "{codes:?}");
    assert!(codes.contains(&"unknown-output-item"), "{codes:?}");
    assert!(
        outcome.items.iter().any(|item| matches!(
            item,
            Item::Opaque { payload, .. } if payload["type"] == json!("web_search_call")
        )),
        "an item we do not model is kept, not dropped: {:?}",
        outcome.items
    );
}

#[test]
fn arguments_that_are_not_json_never_reach_a_tool() {
    let mut tools = TestTools::with_read();
    let (_fixture, outcome, _) = run("bad-arguments", &mut tools);
    let harness_loop::LoopError::Wire(error) = outcome.expect_err("bad arguments refuse") else {
        panic!("undecodable arguments map to a wire error");
    };
    assert_eq!(error.code, WireErrorCode::Protocol);
    assert!(tools.calls.is_empty(), "the tool must not have run");
}

#[test]
fn a_cancel_placed_before_the_run_stops_it_before_it_reaches_the_endpoint() {
    let fixture = Fixture::start("text");
    let mut client = fixture.client();
    client.cancel_handle().cancel();

    let mut tools = TestTools::default();
    let mut approvals = ApproveAll;
    let mut sink = VecLoopSink::new();
    let outcome = AgentLoop::new(
        &mut client,
        &mut tools,
        &mut approvals,
        LoopConfig::new("daemonloom-emulated", "be useful"),
    )
    .run("read the readme", &mut sink);

    // A cancelled read is the caller getting what they asked for, so it is an outcome rather than
    // an error. Reporting it as a failure would tell a person who cancelled that something broke.
    let outcome = outcome.expect("cancellation is an outcome");
    assert!(matches!(outcome.stop, LoopStop::Cancelled { .. }));
    assert!(
        sink.text().is_empty(),
        "no answer may arrive after a cancel"
    );
    assert!(
        fixture.requests().is_empty(),
        "a cancelled turn must not reach the endpoint"
    );
}

#[test]
fn a_cancel_during_a_stream_stops_it_mid_answer() {
    // The case that matters. Cancelling before the run starts proves only that the pre-flight
    // check works; this one proves the reader actually abandons a response it is part-way through.
    let fixture = Fixture::start("slow");
    let mut client = fixture.client();
    let cancel = client.cancel_handle();

    let mut tools = TestTools::default();
    let mut approvals = ApproveAll;
    let mut sink = VecLoopSink::new();

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(700));
        cancel.cancel();
    });

    let outcome = AgentLoop::new(
        &mut client,
        &mut tools,
        &mut approvals,
        LoopConfig::new("daemonloom-emulated", "be useful"),
    )
    .run("take your time", &mut sink)
    .expect("cancellation is an outcome");

    assert!(matches!(outcome.stop, LoopStop::Cancelled { .. }));
    assert_eq!(
        fixture.requests().len(),
        1,
        "the turn did reach the endpoint: {:?}",
        fixture.requests()
    );
    assert!(
        !sink.text().contains("never be delivered"),
        "the full answer must not arrive after a cancel: {:?}",
        sink.text()
    );
    assert!(
        outcome.text.is_empty(),
        "a cancelled turn has no answer: {:?}",
        outcome.text
    );
}
