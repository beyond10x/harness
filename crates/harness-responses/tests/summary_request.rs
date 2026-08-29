//! What the loop's summariser sends, projected through this wire.
//!
//! The summary turn is a request like any other, and it used to be one this route would refuse: the
//! fold it carried began with an assistant-side item, carried `function_call` and
//! `function_call_output` entries while publishing no tools, and replayed opaque reasoning items
//! into a request that was not the conversation they came from.
//!
//! [`harness_loop::summary_request_items`] renders the fold to text instead, and this projects its
//! output to prove the shape is wire-neutral rather than merely tolerated here.

use b10x_harness_responses::{WIRE, request_body};
use harness_wire::{CallId, Item, Sampling, ToolCall, ToolName, ToolOutcome, WireId};
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
                "type": "reasoning",
                "id": "rs_1",
                "encrypted_content": "opaque",
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
        "b10x-session",
        "test-model",
        "the standing instruction",
        items,
        &[],
        None,
        &Sampling::default(),
    )
}

#[test]
fn a_summary_request_projects_to_one_user_message_and_no_tool_or_reasoning_entries() {
    let projected = body(&harness_loop::summary_request_items(&folded()));
    let input = projected["input"]
        .as_array()
        .expect("this route carries its conversation in `input`");

    assert_eq!(
        input.len(),
        2,
        "the standing instruction and one user message, and nothing else: {input:?}"
    );
    assert_eq!(input[0]["role"], "developer", "{:?}", input[0]);
    assert_eq!(input[1]["type"], "message", "{:?}", input[1]);
    assert_eq!(input[1]["role"], "user", "{:?}", input[1]);
    for entry in input {
        let kind = entry["type"].as_str().unwrap_or_default();
        assert!(
            !matches!(kind, "function_call" | "function_call_output" | "reasoning"),
            "a turn that publishes no tools may carry no tool entries, and a reasoning item \
             belongs only to the conversation that produced it: {entry}"
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
    // Kept as evidence rather than as prose: the defect was real on this route too, and a change
    // that put the raw fold back would make this pass again.
    let projected = body(&folded());
    let input = projected["input"]
        .as_array()
        .expect("this route carries its conversation in `input`");

    assert_eq!(
        input[1]["role"], "assistant",
        "the fold begins after the task, so its first item is assistant-side"
    );
    assert!(
        input
            .iter()
            .any(|entry| entry["type"] == "function_call" || entry["type"] == "reasoning"),
        "and it carries tool and reasoning entries while `tools` is empty"
    );
}
