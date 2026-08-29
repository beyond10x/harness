//! The pinned wire subset, replayed through the code a live turn uses.
//!
//! A contract that is only prose drifts silently. These two fixtures are the wire: change what the
//! harness sends or what it accepts, and one of them stops matching.

use std::fs;
use std::path::PathBuf;

use b10x_harness_responses::{WIRE, decode_stream, request_body};
use harness_wire::{
    Approval, CallId, Envelope, Item, Sampling, StopReason, ToolCall, ToolChoice, ToolName,
    ToolOutcome, ToolSpec, TurnRequest, Usage, VecSink, WireId,
};
use serde_json::{Value, json};

/// The cut that added `tool_choice`. `2026-08-22` is the same wire without it, and `2026-08-21`
/// the one before the prompt-cache key; both stay pinned as they were released.
const VERSION: &str = "2026-08-30";

fn contract_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("contracts")
        .join("provider-wires")
        .join(WIRE)
        .join(VERSION)
}

fn fixture(name: &str) -> String {
    let path = contract_dir().join("fixtures").join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading `{}`: {error}", path.display()))
}

fn manifest() -> Value {
    serde_json::from_str(
        &fs::read_to_string(contract_dir().join("manifest.json")).expect("readable"),
    )
    .expect("the manifest is JSON")
}

/// The canonical turn: an instruction, a person's input, a replayed reasoning item, one call and
/// its result. Every field the harness ever sends appears here.
fn canonical_request() -> Value {
    let items = vec![
        Item::user("read the readme"),
        Item::Opaque {
            wire: WireId::new(WIRE).expect("valid"),
            payload: json!({
                "id": "rs_1",
                "type": "reasoning",
                "summary": [],
                "encrypted_content": "OPAQUE",
            }),
        },
        Item::ToolCall(ToolCall {
            call_id: CallId::new("call_1").expect("valid"),
            name: ToolName::new("workspace_read").expect("valid"),
            arguments: json!({"path": "README.md"}),
        }),
        Item::result(
            CallId::new("call_1").expect("valid"),
            ToolOutcome::ok(json!({"text": "hello harness"})),
        ),
    ];
    let tools = vec![ToolSpec {
        name: ToolName::new("workspace_read").expect("valid"),
        description: "Read one text file inside the workspace.".to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"],
            "additionalProperties": false,
        }),
        approval: Approval::NotRequired,
        envelope: Envelope::default(),
    }];
    request_body(
        // Fixed here so the pinned fixture stays byte-stable; a real run's is per-conversation.
        "b10x-session-fixture",
        &TurnRequest {
            model: "b10x-emulated".to_owned(),
            instructions: "be useful".to_owned(),
            items,
            tools,
            max_output_tokens: Some(4096),
            // Set, not defaulted. A fixture with the sampling fields absent would pin only that
            // they can be left out, which is what the previous version already pinned.
            sampling: Sampling {
                temperature: Some(0.2),
                top_p: Some(0.95),
                reasoning_effort: Some("medium".to_owned()),
            },
            // Held to the turn's own tool, because the fixture's job is to carry **every** field
            // the harness sends and this one is only sent when a caller holds a turn. What every
            // other turn of a run sends for it is nothing.
            tool_choice: ToolChoice::Named(ToolName::new("workspace_read").expect("valid")),
        },
    )
}

#[test]
fn the_request_the_harness_sends_matches_the_pinned_fixture() {
    let expected: Value =
        serde_json::from_str(&fixture("turn-request.json")).expect("the request fixture is JSON");
    assert_eq!(
        canonical_request(),
        expected,
        "the request shape changed; re-pin the fixture and say so in the changelog"
    );
}

#[test]
fn the_manifest_names_exactly_the_request_fields_the_harness_sends() {
    let sent: Vec<String> = canonical_request()
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect();
    let pinned: Vec<String> = manifest()["request_fields"]
        .as_array()
        .expect("an array")
        .iter()
        .map(|value| value.as_str().expect("a string").to_owned())
        .collect();
    assert_eq!(sent, pinned);
}

#[test]
fn the_pinned_stream_decodes_into_the_expected_turn() {
    let mut sink = VecSink::new();
    let outcome = decode_stream(
        "b10x-emulated",
        fixture("turn-stream.sse").as_bytes(),
        &mut sink,
    )
    .expect("the pinned stream decodes");

    assert_eq!(sink.text(), "Reading the readme.");
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
    assert_eq!(
        outcome.usage,
        Some(Usage {
            model: "b10x-emulated".to_owned(),
            input_tokens: 42,
            output_tokens: 11,
            cached_input_tokens: 7,
            cache_creation_input_tokens: None,
        })
    );

    let kinds: Vec<&str> = outcome
        .items
        .iter()
        .map(|item| match item {
            Item::Opaque { .. } => "opaque",
            Item::ToolCall(_) => "tool-call",
            Item::AssistantText { .. } => "assistant-text",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, vec!["opaque", "tool-call", "assistant-text"]);

    let call = outcome
        .tool_calls()
        .next()
        .expect("the pinned stream carries one call");
    assert_eq!(call.call_id.as_str(), "call_1");
    assert_eq!(call.arguments, json!({"path": "README.md"}));

    // Nothing in the pinned stream may be unrecognized: a warning here means the subset drifted.
    assert!(
        sink.events()
            .iter()
            .all(|event| !matches!(event, harness_wire::StreamEvent::Warning { .. })),
        "{:?}",
        sink.events()
    );
}

#[test]
fn the_manifest_names_exactly_the_stream_events_the_fixture_carries() {
    let seen: std::collections::BTreeSet<String> = fixture("turn-stream.sse")
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|payload| *payload != "[DONE]")
        .map(|payload| {
            serde_json::from_str::<Value>(payload).expect("each event is JSON")["type"]
                .as_str()
                .expect("each event has a type")
                .to_owned()
        })
        .collect();
    let pinned: std::collections::BTreeSet<String> = manifest()["stream_events"]
        .as_array()
        .expect("an array")
        .iter()
        .map(|value| value.as_str().expect("a string").to_owned())
        .collect();
    assert_eq!(seen, pinned);
}
