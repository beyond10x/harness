//! The pinned wire subset, replayed through the code a live turn uses.
//!
//! A contract that is only prose drifts silently. These two fixtures are the wire: change what the
//! harness sends or what it accepts, and one of them stops matching.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use b10x_harness_messages::{
    ANTHROPIC_VERSION, OAUTH_BETA, WIRE, decode_stream, header_names, request_body,
};
use harness_wire::{
    Approval, CallId, CredentialKind, Envelope, Item, Sampling, StopReason, ToolCall, ToolName,
    ToolOutcome, ToolSpec, Usage, VecSink, WireId,
};
use serde_json::{Value, json};

const VERSION: &str = "2026-08-29";

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

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|entry| entry.as_str().expect("a string").to_owned())
        .collect()
}

/// The canonical turn: an instruction, a person's input, a replayed thinking block, one call and
/// its result. Every field the harness ever sends appears here.
fn canonical_request() -> Value {
    let items = vec![
        Item::user("read the readme"),
        Item::Opaque {
            wire: WireId::new(WIRE).expect("valid"),
            payload: json!({
                "type": "thinking",
                "thinking": "OPAQUE-REASONING-BLOB",
                "signature": "OPAQUE-SIGNATURE",
            }),
        },
        Item::assistant("Reading the readme."),
        Item::ToolCall(ToolCall {
            call_id: CallId::new("toolu_1").expect("valid"),
            name: ToolName::new("workspace_read").expect("valid"),
            arguments: json!({"path": "README.md"}),
        }),
        Item::result(
            CallId::new("toolu_1").expect("valid"),
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
        "b10x-emulated",
        "be useful",
        &items,
        &tools,
        // Named, not defaulted: this route requires an output bound, so the fixture pins what one
        // looks like rather than pinning that it can be left out — it cannot.
        4096,
        // Set, not defaulted, for the same reason the first wire's fixture sets them.
        &Sampling {
            temperature: Some(0.2),
            top_p: Some(0.95),
            reasoning_effort: Some("medium".to_owned()),
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
    assert_eq!(sent, strings(&manifest()["request_fields"]));
}

#[test]
fn the_manifest_names_exactly_the_headers_the_harness_sends() {
    // **The half the Python checker cannot see.** The credential's presentation is not in the
    // body, and it is the difference between a key issued to a program and a token obtained on a
    // person's behalf. Sending either under the other's header is a 401 that names authentication
    // and never mentions the header, which is the failure this pin exists to prevent.
    let pinned = &manifest()["request_headers"];
    assert_eq!(strings(&pinned["always"]), header_names(None));
    assert_eq!(
        strings(&pinned["api-key"]),
        header_names(Some(CredentialKind::ApiKey))
    );
    assert_eq!(
        strings(&pinned["oauth"]),
        header_names(Some(CredentialKind::Oauth))
    );
    assert_eq!(pinned["oauth_beta"], json!(OAUTH_BETA));
    assert_eq!(manifest()["api_version"], json!(ANTHROPIC_VERSION));
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
    // 42 fresh, 7 read from cache, 3 written to it. The neutral total is the sum, because
    // `Usage::input_tokens` is the whole and the cache figures are parts of it — this route
    // reports them disjointly and the projection is what reconciles the two.
    assert_eq!(
        outcome.usage,
        Some(Usage {
            model: "b10x-emulated".to_owned(),
            input_tokens: 52,
            output_tokens: 11,
            cached_input_tokens: 7,
            cache_creation_input_tokens: Some(3),
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

    // The thinking block is carried whole, signature and all, and never reinterpreted.
    assert_eq!(
        outcome.items[0],
        Item::Opaque {
            wire: WireId::new(WIRE).expect("valid"),
            payload: json!({
                "type": "thinking",
                "thinking": "OPAQUE-REASONING-BLOB",
                "signature": "OPAQUE-SIGNATURE",
            }),
        }
    );

    let call = outcome
        .tool_calls()
        .next()
        .expect("the pinned stream carries one call");
    assert_eq!(call.call_id.as_str(), "toolu_1");
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
    let seen: BTreeSet<String> = fixture("turn-stream.sse")
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|payload| {
            serde_json::from_str::<Value>(payload).expect("each event is JSON")["type"]
                .as_str()
                .expect("each event has a type")
                .to_owned()
        })
        .collect();
    let pinned: BTreeSet<String> = strings(&manifest()["stream_events"]).into_iter().collect();
    assert_eq!(seen, pinned);
}

#[test]
fn the_manifest_names_exactly_the_content_block_deltas_the_fixture_carries() {
    // A second layer the first wire does not have: on this route the interesting variation is
    // inside `content_block_delta`, so pinning the outer event names alone would pin almost
    // nothing.
    let seen: BTreeSet<String> = fixture("turn-stream.sse")
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|payload| {
            let event: Value = serde_json::from_str(payload).expect("each event is JSON");
            event
                .get("delta")
                .and_then(|delta| delta.get("type"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect();
    let pinned: BTreeSet<String> = strings(&manifest()["content_block_deltas"])
        .into_iter()
        .collect();
    assert_eq!(seen, pinned);
}
