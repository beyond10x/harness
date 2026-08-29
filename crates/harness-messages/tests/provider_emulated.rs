//! The whole stack against a real socket.
//!
//! Everything below runs the actual HTTP client, the actual SSE reader and the actual loop against
//! a deterministic local endpoint. That is what separates this from the unit tests: those prove the
//! projection in isolation, this proves the pieces agree with each other over a wire.
//!
//! **This file is `harness-responses`'s suite, case for case, pointed at the second wire.** The
//! roadmap's exit criterion for phase 3 is that both wires pass the same loop suite, and a suite
//! that is only claimed to be the same is one that drifts. The case names match, the scenario names
//! match, and [`the_two_wires_serve_the_same_scenarios`] asserts the second half of that
//! mechanically.
//!
//! This is `provider_emulated` evidence. It says nothing about how a real provider behaves.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use b10x_harness_messages::{Endpoint, MessagesClient, WIRE};
use harness_credential::{NamedSource, SubscriptionToken};
use harness_loop::{AgentLoop, ApproveAll, Budget, LoopConfig, LoopEvent, LoopStop, VecLoopSink};
use harness_wire::{
    Envelope, Item, StaticBearer, ToolCall, ToolName, ToolOutcome, ToolPort, ToolSpec,
    WireErrorCode,
};
use serde_json::{Value, json};

fn emulator(crate_name: &str, script: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(crate_name)
        .join("tests")
        .join("fixtures")
        .join(script)
}

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
        let mut child = Command::new("python3")
            .arg(emulator("harness-messages", "fake_messages.py"))
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

    fn endpoint(&self) -> Endpoint {
        Endpoint::new(&self.base_url, "b10x-emulated", 32_000)
            .expect("the fixture endpoint is well formed")
    }

    fn client(&self) -> MessagesClient {
        MessagesClient::new(
            self.endpoint(),
            Arc::new(StaticBearer::new("synthetic-test-key")),
        )
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
    let config = LoopConfig::new("b10x-emulated", "be useful")
        .with_budget(Budget::default().with_max_turns(5));
    let outcome = AgentLoop::new(&mut client, tools, &mut approvals, config)
        .run("read the readme", &mut sink);
    (fixture, outcome, sink)
}

#[test]
fn the_two_wires_serve_the_same_scenarios() {
    // The roadmap's exit criterion, made mechanical. Both emulators declare their scenario list and
    // this compares the declarations: a case added to one wire's suite and not the other's fails
    // here rather than being noticed a release later.
    let list = |crate_name: &str, script: &str| -> Vec<String> {
        let output = Command::new("python3")
            .arg(emulator(crate_name, script))
            .arg("--list-scenarios")
            .output()
            .expect("the emulator runs; python3 must be available");
        assert!(output.status.success(), "{crate_name} listed no scenarios");
        serde_json::from_slice(&output.stdout).expect("the list is JSON")
    };
    assert_eq!(
        list("harness-messages", "fake_messages.py"),
        list("harness-responses", "fake_responses.py"),
        "the two wires must be exercised by the same set of cases"
    );
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
    // 42 fresh plus 7 read from cache. The neutral total is the sum, not the wire's own figure.
    assert_eq!(outcome.total_tokens(), Some((49, 4)));
    assert_eq!(fixture.requests().len(), 1);
}

#[test]
fn the_request_is_stateless_versioned_and_carries_a_cache_breakpoint() {
    let mut tools = TestTools::with_read();
    let (fixture, outcome, _) = run("text", &mut tools);
    outcome.expect("the run completes");

    let requests = fixture.requests();
    let first = &requests[0];
    assert_eq!(first["stream"], json!(true));
    assert_eq!(first["accept"], json!("text/event-stream"));
    assert_eq!(first["anthropic_version"], json!("2023-06-01"));
    // A key issued to a program travels as `x-api-key`, never as `authorization`.
    assert_eq!(first["credential_header"], json!("x-api-key"));
    assert_eq!(first["anthropic_beta"], json!(null), "no beta was needed");
    // The standing instruction is a block list so it can carry the breakpoint that covers the
    // constant head of every turn of a stateless run.
    assert_eq!(first["system_text"], json!("be useful"));
    assert_eq!(
        first["system_cache_control"],
        json!({"type": "ephemeral"}),
        "without this the constant head is paid for at full rate on every turn"
    );
    assert_eq!(first["first_message_role"], json!("user"));
    assert_eq!(first["tool_names"], json!(["workspace_read"]));
    // Required by this route, so an unnamed bound resolves to the endpoint's own number.
    assert_eq!(first["max_tokens"], json!(8192));
    assert_eq!(first["conversation_id"], json!(null), "nothing is retained");
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
            LoopEvent::ToolArgumentsDelta { call_id, .. } if call_id.as_str() == "toolu_b10x_001"
        )),
        "{:?}",
        sink.events()
    );

    // The second request must carry the call and its result, grouped into role-alternating
    // messages: the call is the model's turn and the result is the person's.
    let requests = fixture.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1]["message_roles"],
        json!(["user", "assistant", "user"]),
        "{:?}",
        requests[1]["messages"]
    );
    let messages = requests[1]["messages"].as_array().expect("an array");
    assert_eq!(messages[1]["content"][0]["type"], json!("tool_use"));
    assert_eq!(messages[1]["content"][0]["id"], json!("toolu_b10x_001"));
    // A structured object, not encoded text: this wire carries arguments as JSON.
    assert_eq!(
        messages[1]["content"][0]["input"],
        json!({"path": "README.md"})
    );
    let result = &messages[2]["content"][0];
    assert_eq!(result["type"], json!("tool_result"));
    assert_eq!(result["tool_use_id"], json!("toolu_b10x_001"));
    assert_eq!(result["is_error"], json!(false));
    assert!(
        result["content"]
            .as_str()
            .expect("the content is a string")
            .contains("hello harness"),
        "{result}"
    );
}

#[test]
fn a_thinking_block_is_replayed_verbatim_on_the_next_turn() {
    let mut tools = TestTools::with_read();
    let (fixture, outcome, _) = run("reasoning", &mut tools);
    let outcome = outcome.expect("the run completes");

    // Held as opaque, tagged with the wire that produced it, and assembled from its deltas.
    assert!(
        outcome.items.iter().any(|item| matches!(
            item,
            Item::Opaque { wire, payload }
                if wire.as_str() == WIRE
                    && payload["thinking"] == json!("OPAQUE-REASONING-BLOB")
                    && payload["signature"] == json!("OPAQUE-SIGNATURE")
        )),
        "{:?}",
        outcome.items
    );

    // And actually sent back, byte for byte, at the head of the assistant message — where the
    // model put it. The signature is what the provider verifies, so an edited or reordered block
    // is a rejected turn.
    let requests = fixture.requests();
    let replayed = requests[1]["replayed_thinking"]
        .as_array()
        .expect("an array");
    assert_eq!(replayed.len(), 1, "{:?}", requests[1]);
    assert_eq!(replayed[0]["thinking"], json!("OPAQUE-REASONING-BLOB"));
    assert_eq!(replayed[0]["signature"], json!("OPAQUE-SIGNATURE"));
    let messages = requests[1]["messages"].as_array().expect("an array");
    assert_eq!(
        messages[1]["content"][0]["type"],
        json!("thinking"),
        "a thinking block must stay first in its message: {:?}",
        messages[1]
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
    let harness_loop::LoopError::Wire(error) = outcome.expect_err("a failed message refuses")
    else {
        panic!("a provider failure maps to a wire error");
    };
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
            Item::Opaque { payload, .. } if payload["type"] == json!("server_tool_use")
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
        LoopConfig::new("b10x-emulated", "be useful"),
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
        LoopConfig::new("b10x-emulated", "be useful"),
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

#[test]
fn a_turn_that_failed_before_answering_is_retried_and_the_run_recovers() {
    let mut tools = TestTools::default();
    let (_fixture, outcome, sink) = run("cold-once", &mut tools);
    let outcome = outcome.expect("the second attempt answers");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.text, "provider emulation passed");
    assert!(
        sink.warnings().any(|(code, _)| code == "turn-retried"),
        "and it says so: a run that quietly took four times as long is one whose latency means \
         nothing"
    );
}

#[test]
fn a_turn_that_had_already_answered_is_never_retried() {
    // The whole of the retry rule. Resending is safe on this wire — nothing is retained on the far
    // side — but not once the caller has seen part of the first attempt: the text is out, a person
    // has read it, and a second attempt would append a second copy of the same sentence.
    let mut tools = TestTools::default();
    let (_fixture, outcome, sink) = run("truncated", &mut tools);

    assert!(
        outcome.is_err(),
        "a stream that stopped mid-answer is final"
    );
    assert!(
        !sink.warnings().any(|(code, _)| code == "turn-retried"),
        "nothing was retried after the caller had seen output"
    );
}

#[test]
fn a_subscription_token_reaches_the_route_under_its_own_headers() {
    // Phase 4, end to end over a real socket: the same secret a key would have travelled as, sent
    // the way a token obtained on a person's behalf has to be sent. Getting this wrong is a 401
    // that names authentication and never mentions the header.
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("credentials.json");
    // Synthetic, and named by this test rather than found anywhere (AGENTS.md invariant 17).
    std::fs::write(
        &path,
        r#"{"store": {"accessToken": "synthetic-oauth-token", "expiresAt": 0}}"#,
    )
    .expect("write");

    let fixture = Fixture::start("text");
    let source = SubscriptionToken::new(NamedSource::file(&path)).at_pointer("/store/accessToken");
    let mut client =
        MessagesClient::new(fixture.endpoint(), Arc::new(source)).expect("the client builds");
    let mut tools = TestTools::default();
    let mut approvals = ApproveAll;
    let mut sink = VecLoopSink::new();
    let outcome = AgentLoop::new(
        &mut client,
        &mut tools,
        &mut approvals,
        LoopConfig::new("b10x-emulated", "be useful"),
    )
    .run("read the readme", &mut sink)
    .expect("the run completes");
    assert_eq!(outcome.stop, LoopStop::Completed);

    let requests = fixture.requests();
    assert_eq!(requests[0]["credential_header"], json!("authorization"));
    // The route rejects a bearer token without this, and says only that authentication failed.
    assert_eq!(requests[0]["anthropic_beta"], json!("oauth-2025-04-20"));
    assert_eq!(
        requests[0]["credential_length"],
        json!("Bearer synthetic-oauth-token".len()),
        "the token travelled whole, prefixed by its scheme"
    );
}
