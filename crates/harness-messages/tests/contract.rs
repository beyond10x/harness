//! The pinned wire subset, replayed through the code a live turn uses.
//!
//! A contract that is only prose drifts silently. These two fixtures are the wire: change what the
//! harness sends or what it accepts, and one of them stops matching.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use b10x_harness_messages::{
    ACCEPTED_CONTENT_BLOCK_DELTAS, ACCEPTED_STREAM_EVENTS, SUBSCRIPTION_CLIENT_PREAMBLE, WIRE,
    contract_headers, decode_stream, request_body,
};
use harness_wire::{
    Approval, CallId, CredentialKind, Envelope, Item, Sampling, StopReason, ToolCall, ToolChoice,
    ToolName, ToolOutcome, ToolSpec, TurnRequest, Usage, VecSink, WireId,
};
use serde_json::{Value, json};

/// The cut that added the subscription client preamble. `2026-08-30` is the same wire without it,
/// `2026-08-29b` the one before `tool_choice`, and `2026-08-29` the one before the rolling cache
/// breakpoint; all three stay pinned as they were released.
const VERSION: &str = "2026-08-31";

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

fn fixture_bytes(name: &str) -> Vec<u8> {
    fs::read(contract_dir().join("fixtures").join(name)).expect("readable fixture")
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
    canonical_request_as(None)
}

/// The same turn, projected under a named credential presentation.
///
/// **The presentation is an argument because on this route it changes the body.** A subscription
/// token is served only when `system` opens with the client preamble as its own block, so the
/// contract pins two request fixtures rather than one and this is the single place either is
/// built — a second builder would prove only that the second builder works.
fn canonical_request_as(credential: Option<CredentialKind>) -> Value {
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
        &TurnRequest {
            model: "b10x-emulated".to_owned(),
            instructions: "be useful".to_owned(),
            items,
            tools,
            // Resolved by the caller on this route, and passed beside the turn below.
            max_output_tokens: None,
            // Set, not defaulted, for the same reason the first wire's fixture sets them.
            sampling: Sampling {
                temperature: Some(0.2),
                top_p: Some(0.95),
                reasoning_effort: Some("medium".to_owned()),
            },
            // Held to the turn's own tool, because the fixture's job is to carry **every** field
            // the harness sends and this one is only sent when a caller holds a turn. `auto` is
            // what the other turns of a run send, and what they send is nothing.
            tool_choice: ToolChoice::Named(ToolName::new("workspace_read").expect("valid")),
        },
        // Named, not defaulted: this route requires an output bound, so the fixture pins what one
        // looks like rather than pinning that it can be left out — it cannot.
        4096,
        credential,
    )
}

#[test]
fn the_request_the_harness_sends_matches_the_pinned_fixture() {
    let expected = fixture_bytes("turn-request.json");
    let actual = harness_http::encode_json_body(&canonical_request()).expect("encodes");
    assert_eq!(
        actual, expected,
        "the exact request bytes changed; cut a contract and say so in the changelog"
    );
}

#[test]
fn the_request_a_subscription_token_sends_matches_its_own_pinned_fixture() {
    let expected = fixture_bytes("turn-request-oauth.json");
    let actual = harness_http::encode_json_body(&canonical_request_as(Some(CredentialKind::Oauth)))
        .expect("encodes");
    assert_eq!(
        actual, expected,
        "the exact subscription request bytes changed; cut a contract"
    );
}

#[test]
fn every_header_name_and_non_secret_value_is_pinned_for_each_presentation() {
    let rows = |kind| {
        Value::Array(
            contract_headers(kind)
                .into_iter()
                .map(|(name, value)| json!({"name":name, "value":value}))
                .collect(),
        )
    };
    let pinned = &manifest()["request_headers"];
    assert_eq!(rows(None), pinned["none"]);
    assert_eq!(rows(Some(CredentialKind::ApiKey)), pinned["api-key"]);
    assert_eq!(rows(Some(CredentialKind::Oauth)), pinned["oauth"]);
}

#[test]
fn the_manifest_pins_both_production_event_inventories_and_terminal_policy() {
    let accepted: Value =
        serde_json::from_str(&fixture("accepted-events.json")).expect("inventory JSON");
    assert_eq!(accepted["stream_events"], json!(ACCEPTED_STREAM_EVENTS));
    assert_eq!(
        accepted["content_block_deltas"],
        json!(ACCEPTED_CONTENT_BLOCK_DELTAS)
    );
    assert_eq!(manifest()["stream_events"], accepted["stream_events"]);
    assert_eq!(
        manifest()["content_block_deltas"],
        accepted["content_block_deltas"]
    );
    assert_eq!(manifest()["terminal_sentinel"]["required"], json!(false));
    assert_eq!(
        manifest()["terminal_sentinel"]["terminal_event"],
        "message_stop"
    );
}

/// The whole of what this version cut a directory for.
///
/// Asserted field by field rather than only by whole-body equality above, because a reader of the
/// contract is entitled to find the rule stated: **block 0 is the preamble, exactly, alone**, and
/// the run's own instruction is the block after it. Every other shape measured on 2026-08-30
/// answered `429` with no rate-limit headers, which reads downstream as an exhausted quota.
#[test]
fn a_subscription_token_opens_the_system_with_the_client_preamble_and_nothing_else() {
    let oauth = canonical_request_as(Some(CredentialKind::Oauth));
    let system = oauth["system"].as_array().expect("an array");

    assert_eq!(system.len(), 2, "{oauth}");
    assert_eq!(system[0]["text"], json!(SUBSCRIPTION_CLIENT_PREAMBLE));
    assert_eq!(system[0]["type"], json!("text"));
    // Alone: the preamble block carries the string and no breakpoint, so nothing can be appended
    // to it later without this failing.
    assert_eq!(
        system[0].as_object().expect("an object").len(),
        2,
        "the preamble block carries text and type and nothing else: {oauth}"
    );
    assert_eq!(system[1]["text"], json!("be useful"));

    // The breakpoint moves to the last block so the constant head stays cached — see the
    // projection's own note. Under a key issued to a program there is one block and it is both.
    assert_eq!(
        system[1]["cache_control"],
        json!({"type": "ephemeral"}),
        "{oauth}"
    );
    assert!(system[0].get("cache_control").is_none(), "{oauth}");

    let key = canonical_request();
    assert_eq!(key["system"].as_array().expect("an array").len(), 1);
    assert_eq!(key["system"][0]["text"], json!("be useful"));
}

#[test]
fn the_pinned_request_carries_both_cache_breakpoints_and_nothing_opaque_is_marked() {
    // The whole of what `2026-08-29b` cut a version for. Asserted against the **fixture** rather
    // than against the projection, because the projection is what the test above already compares:
    // this one says what a reader of the contract is entitled to find in it.
    let request: Value =
        serde_json::from_str(&fixture("turn-request.json")).expect("the request fixture is JSON");

    // The constant head: `tools` then `system` is everything before the conversation.
    assert_eq!(
        request["system"][0]["cache_control"],
        json!({"type": "ephemeral"})
    );

    // The rolling one: the last content block of the last message, which is where the conversation
    // grows. Without it the growth is re-charged in full on every remaining turn.
    let messages = request["messages"].as_array().expect("an array");
    let last = messages.last().expect("a last message");
    assert_eq!(last["role"], json!("user"));
    let blocks = last["content"].as_array().expect("an array");
    assert_eq!(
        blocks.last().expect("a last block")["cache_control"],
        json!({"type": "ephemeral"})
    );

    // Two, against a cap of four. A third would have to be argued for.
    assert_eq!(
        request.to_string().matches("cache_control").count(),
        2,
        "{request}"
    );

    // And never on a replayed thinking block: its signature covers the block as the model produced
    // it, so an added key is a rejected turn (AGENTS.md invariant 5).
    let thinking = messages
        .iter()
        .flat_map(|message| message["content"].as_array().expect("an array"))
        .find(|block| block["type"] == json!("thinking"))
        .expect("the canonical turn replays one");
    assert_eq!(
        *thinking,
        json!({
            "type": "thinking",
            "thinking": "OPAQUE-REASONING-BLOB",
            "signature": "OPAQUE-SIGNATURE",
        })
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
    // 31 fresh, 7 read from cache, 5 written to it. The neutral total is the sum, because
    // `Usage::input_tokens` is the whole and the cache figures are parts of it — this route
    // reports them disjointly and the projection is what reconciles the two.
    assert_eq!(
        outcome.usage,
        Some(Usage {
            model: "b10x-emulated".to_owned(),
            input_tokens: 43,
            output_tokens: 11,
            cached_input_tokens: 7,
            cache_creation_input_tokens: Some(5),
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
    assert_eq!(kinds, vec!["opaque", "assistant-text", "tool-call"]);

    // The thinking block is carried whole, signature and all, and never reinterpreted.
    assert_eq!(
        outcome.items[0],
        Item::Opaque {
            wire: WireId::new(WIRE).expect("valid"),
            payload: json!({
                "type": "thinking",
                "thinking": "Checking.",
                "signature": "SIG",
            }),
        }
    );

    // And shown while it arrived. The pinned stream carries one `thinking_delta` and one
    // `signature_delta`; only the first is reasoning a person should see, and the signature is
    // never shown. An event count here is what catches the summary being emitted twice.
    let reasoning: Vec<&str> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            harness_wire::StreamEvent::ReasoningDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(reasoning, vec!["Checking."]);

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
    assert!(seen.is_subset(&pinned));
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

#[test]
fn the_pinned_error_event_takes_the_typed_retryable_path() {
    let mut sink = VecSink::new();
    let error = decode_stream(
        "b10x-emulated",
        fixture("error-stream.sse").as_bytes(),
        &mut sink,
    )
    .expect_err("the error is terminal");
    assert!(error.retriable, "{error:?}");
}
