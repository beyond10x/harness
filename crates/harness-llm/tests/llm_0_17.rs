//! `story:verbs-survives-a-bare-entry-call` and `story:responses-replays-no-stream-metadata`, on
//! llm 0.17.0.
//!
//! Both were found live on 2026-10-29 (`measurements/README.md` § *The live runs of 2026-10-29*):
//! on llm 0.16.0 a model that called an entry by its own name under `--surface verbs` lost the
//! whole turn to `model output contains a duplicate call or unpublished tool`, so the loop could
//! not answer the call as a refusal (invariant 9); and a Codex stream's `response.metadata` event
//! came back as an opaque item that the next request replayed, which the endpoint refused with
//! HTTP 400. These tests hold the port's half: the call reaches the loop, and the event does not
//! become conversation state.
//!
//! Every secret and every endpoint is synthetic and loopback (`AGENTS.md` invariant 17); evidence
//! from this endpoint is `provider_emulated` (invariant 18).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

use harness_llm::{Protocol, Source, Target, client};
use harness_wire::{
    Approval, Cancel, Envelope, Item, Sampling, StopReason, ToolChoice, ToolName, ToolSpec,
    TurnOutcome, TurnRequest, VecSink, WireError,
};
use serde_json::json;

/// A loopback endpoint that answers every request with `body` as an event stream and keeps
/// nothing a request carried.
fn serve(body: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let base_url = format!("http://{}/v1", listener.local_addr().expect("bound"));
    std::thread::spawn(move || {
        for connection in listener.incoming() {
            let Ok(connection) = connection else { return };
            answer(connection, &body);
        }
    });
    base_url
}

fn answer(connection: TcpStream, body: &str) {
    let mut reader = BufReader::new(connection);
    let mut length = 0;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    let mut request = vec![0; length];
    if reader.read_exact(&mut request).is_err() {
        return;
    }
    let mut connection = reader.into_inner();
    let _ = write!(
        connection,
        "HTTP/1.0 200 OK\r\ncontent-type: text/event-stream\r\n\r\n{body}"
    );
}

fn sse(events: &[(&str, serde_json::Value)]) -> String {
    use std::fmt::Write as _;
    let mut body = String::new();
    for (name, data) in events {
        write!(body, "event: {name}\ndata: {data}\n\n").expect("writing to a String");
    }
    body
}

/// The one tool the verbs surface would publish for discovery; `find` is not published.
fn published() -> ToolSpec {
    ToolSpec {
        name: ToolName::new("tool_search").expect("a tool name"),
        description: "List the catalogue.".to_owned(),
        input_schema: json!({"type": "object", "properties": {}}),
        approval: Approval::NotRequired,
        envelope: Envelope::default(),
    }
}

fn request() -> TurnRequest {
    TurnRequest {
        model: "m".to_owned(),
        instructions: "Answer.".to_owned(),
        items: vec![Item::UserText {
            text: "How many files are there?".to_owned(),
        }],
        tools: vec![published()],
        max_output_tokens: None,
        sampling: Sampling::default(),
        tool_choice: ToolChoice::Auto,
    }
}

fn one_turn(protocol: Protocol, body: String) -> Result<TurnOutcome, WireError> {
    let base_url = serve(body);
    let target = Target {
        protocol,
        base_url: &base_url,
        model: "m",
        context_window: 100_000,
        max_output_tokens: None,
    };
    let mut port = client(
        &target,
        Some(Source::key("synthetic-key").expect("a source")),
        &Cancel::new(),
    )
    .expect("a client");
    port.turn(&request(), &mut VecSink::new())
}

/// The Messages stream a live Haiku turn sent on 2026-10-29 under `--surface verbs`, reduced: a
/// `tool_use` block naming `find`, which the request did not publish.
fn messages_calling_an_unpublished_entry() -> String {
    sse(&[
        (
            "message_start",
            json!({"type":"message_start","message":{"id":"msg_b10x_verbs","type":"message","role":"assistant","model":"m","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":1}}}),
        ),
        (
            "content_block_start",
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_b10x_find","name":"find","input":{}}}),
        ),
        (
            "content_block_delta",
            json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"glob\":\"**/*\"}"}}),
        ),
        (
            "content_block_stop",
            json!({"type":"content_block_stop","index":0}),
        ),
        (
            "message_delta",
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":9}}),
        ),
        ("message_stop", json!({"type":"message_stop"})),
    ])
}

#[test]
fn a_call_to_an_unpublished_entry_reaches_the_loop_instead_of_ending_the_turn() {
    let outcome = one_turn(Protocol::Messages, messages_calling_an_unpublished_entry())
        .unwrap_or_else(|error| {
            panic!("an unpublished call is the loop's to answer, not a lost turn: {error:?}")
        });
    let calls: Vec<&str> = outcome
        .items
        .iter()
        .filter_map(|item| match item {
            Item::ToolCall(call) => Some(call.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        calls,
        vec!["find"],
        "the call arrives as the model made it: {outcome:?}"
    );
    assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
}

fn in_progress() -> serde_json::Value {
    json!({"id":"resp_b10x_0001","object":"response","status":"in_progress","model":"m","output":[],"usage":null})
}

/// A Codex-shaped text turn with the `response.metadata` event the live run streamed before its
/// terminal event. Synthetic identifiers and text.
fn responses_with_metadata() -> String {
    let message = json!({"id":"msg_b10x_0001","type":"message","status":"completed","role":"assistant","content":[{"type":"output_text","text":"four","annotations":[]}]});
    sse(&[
        (
            "response.created",
            json!({"type":"response.created","response":in_progress()}),
        ),
        (
            "response.output_item.added",
            json!({"type":"response.output_item.added","output_index":0,"item":{"id":"msg_b10x_0001","type":"message","status":"in_progress","role":"assistant","content":[]}}),
        ),
        (
            "response.output_text.delta",
            json!({"type":"response.output_text.delta","item_id":"msg_b10x_0001","output_index":0,"content_index":0,"delta":"four"}),
        ),
        (
            "response.output_item.done",
            json!({"type":"response.output_item.done","output_index":0,"item":message}),
        ),
        (
            "response.metadata",
            json!({"type":"response.metadata","metadata":{"fixture":"synthetic"}}),
        ),
        (
            "response.completed",
            json!({"type":"response.completed","response":{"id":"resp_b10x_0001","object":"response","status":"completed","model":"m","output":[message],"usage":{"input_tokens":12,"input_tokens_details":{"cached_tokens":0},"output_tokens":3,"output_tokens_details":{"reasoning_tokens":0},"total_tokens":15}}}),
        ),
    ])
}

#[test]
fn a_codex_metadata_event_does_not_become_an_item_the_next_turn_replays() {
    let outcome = one_turn(Protocol::Responses, responses_with_metadata())
        .unwrap_or_else(|error| panic!("the turn completes: {error:?}"));
    assert!(
        outcome
            .items
            .iter()
            .any(|item| matches!(item, Item::AssistantText { text } if text == "four")),
        "the answer is carried: {outcome:?}"
    );
    let replayed = serde_json::to_string(&outcome.items).expect("items serialize");
    assert!(
        !replayed.contains("response.metadata"),
        "nothing the next request replays carries the metadata event: {replayed}"
    );
}
