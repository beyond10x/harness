//! What the loop's summariser sends, projected through this wire.
//!
//! This is the route the defect was found on. Sent as items, the fold began with an assistant-side
//! message — and this route requires the first message to be `user` — and carried `tool_use` and
//! `tool_result` blocks while the summary request publishes `tools: []`, which it also rejects. So
//! every compaction on this wire paid for a turn that could not be answered.
//!
//! [`harness_loop::summary_request_items`] renders the fold to text instead, and this projects its
//! output to prove the shape is wire-neutral rather than merely tolerated here.

use b10x_harness_messages::{WIRE, request_body};
use harness_wire::{
    CallId, Item, Sampling, ToolCall, ToolChoice, ToolName, ToolOutcome, TurnRequest, WireId,
};
use serde_json::json;

/// The part of a conversation a compaction would fold: assistant-first, with a whole tool round
/// trip and the reasoning item that preceded it.
fn folded() -> Vec<Item> {
    let call_id = CallId::new("call-1").expect("a valid call id");
    vec![
        Item::assistant("I will read the file first."),
        Item::Opaque {
            wire: WireId::new(WIRE).expect("a valid wire id"),
            payload: json!({
                "type": "thinking",
                "thinking": "opaque",
                "signature": "opaque",
            }),
        },
        Item::ToolCall(ToolCall {
            call_id: call_id.clone(),
            name: ToolName::new("file_read").expect("a valid tool name"),
            arguments: json!({"path": "README.md"}),
        }),
        Item::result(call_id, ToolOutcome::ok(json!({"text": "hello"}))),
        Item::user("and now summarise it"),
    ]
}

fn body(items: &[Item]) -> serde_json::Value {
    request_body(
        &TurnRequest {
            model: "test-model".to_owned(),
            instructions: "the standing instruction".to_owned(),
            items: items.to_vec(),
            tools: Vec::new(),
            max_output_tokens: None,
            sampling: Sampling::default(),
            tool_choice: ToolChoice::Auto,
        },
        1024,
        // This suite is about how a fold reaches the wire, not about the credential; the shape
        // under a subscription token differs only by a leading `system` block the contract pins.
        None,
    )
}

#[test]
fn a_summary_request_projects_to_one_user_message_with_no_tool_blocks() {
    let projected = body(&harness_loop::summary_request_items(&folded()));
    let messages = projected["messages"]
        .as_array()
        .expect("this route carries its conversation in `messages`");

    assert_eq!(
        messages.len(),
        1,
        "one message, so the first one is `user` whatever the fold held: {messages:?}"
    );
    assert_eq!(messages[0]["role"], "user", "{:?}", messages[0]);
    for block in messages[0]["content"]
        .as_array()
        .expect("a message carries a block list")
    {
        assert_eq!(
            block["type"], "text",
            "a request that publishes no tools may carry no tool blocks, and a thinking block \
             belongs only to the conversation that produced it: {block}"
        );
    }
    assert_eq!(
        projected["tools"],
        json!([]),
        "a summary turn has nothing to call"
    );
}

#[test]
fn the_fold_sent_as_items_is_the_request_this_replaces() {
    // Kept as evidence rather than as prose: both refusals are visible in one projection, and a
    // change that put the raw fold back would make this pass again.
    let projected = body(&folded());
    let messages = projected["messages"]
        .as_array()
        .expect("this route carries its conversation in `messages`");

    assert_eq!(
        messages[0]["role"], "assistant",
        "the fold begins after the task, so its first message is assistant-side"
    );
    let rendered = projected.to_string();
    assert!(
        rendered.contains("tool_use") && rendered.contains("tool_result"),
        "and it carries tool blocks while `tools` is empty"
    );
}
