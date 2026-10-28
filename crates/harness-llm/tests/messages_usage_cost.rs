//! `story:harness-pins-llm-0-16`: a streamed Messages turn whose `message_delta.usage`
//! carries `cost` completes, and the extension reaches the caller's sink as one `usage-extension`
//! warning instead of being dropped (`AGENTS.md` invariant 7).
//!
//! On llm 0.15.0 the same stream ended the turn with `Unsupported: Messages field is outside the
//! declared subset: message_delta.usage.cost`, after the answer had already streamed.
//!
//! Every secret and every endpoint is synthetic and loopback (`AGENTS.md` invariant 17); evidence
//! from this endpoint is `provider_emulated` (invariant 18).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

use harness_llm::{Protocol, Source, Target, client};
use harness_wire::{
    Cancel, Item, Sampling, StopReason, StreamEvent, ToolChoice, TurnOutcome, TurnRequest, VecSink,
    WireError,
};

/// The warning code llm 0.16.0 carries a usage `cost` under (`llm_messages::USAGE_EXTENSION`).
const CODE: &str = "usage-extension";

/// A loopback Messages endpoint that answers every request with one SSE stream: a text block
/// saying `answer`, then a `message_delta` whose `usage` is `end_usage`. It keeps nothing a
/// request carried.
struct Streaming {
    base_url: String,
}

impl Streaming {
    fn start(end_usage: &str) -> Self {
        let body = stream(end_usage);
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let base_url = format!("http://{}/v1", listener.local_addr().expect("bound"));
        std::thread::spawn(move || {
            for connection in listener.incoming() {
                let Ok(connection) = connection else { return };
                answer(connection, &body);
            }
        });
        Self { base_url }
    }

    fn target(&self) -> Target<'_> {
        Target {
            protocol: Protocol::Messages,
            base_url: &self.base_url,
            model: "m",
            context_window: 1000,
            max_output_tokens: None,
        }
    }
}

fn stream(end_usage: &str) -> String {
    let events = [
        (
            "message_start",
            r#"{"type":"message_start","message":{"id":"msg_b10x_cost","type":"message","role":"assistant","model":"m","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":11,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":1}}}"#.to_owned(),
        ),
        (
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_owned(),
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"forty-two"}}"#.to_owned(),
        ),
        (
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#.to_owned(),
        ),
        (
            "message_delta",
            format!(
                r#"{{"type":"message_delta","delta":{{"stop_reason":"end_turn","stop_sequence":null}},"usage":{end_usage}}}"#
            ),
        ),
        ("message_stop", r#"{"type":"message_stop"}"#.to_owned()),
    ];
    let mut body = String::new();
    for (name, data) in &events {
        for part in ["event: ", *name, "\ndata: ", data.as_str(), "\n\n"] {
            body.push_str(part);
        }
    }
    body
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

fn request() -> TurnRequest {
    TurnRequest {
        model: "m".to_owned(),
        instructions: "Answer.".to_owned(),
        items: vec![Item::UserText {
            text: "What is six times seven?".to_owned(),
        }],
        tools: Vec::new(),
        max_output_tokens: None,
        sampling: Sampling::default(),
        tool_choice: ToolChoice::Auto,
    }
}

/// One turn against a fresh endpoint whose `message_delta.usage` is `end_usage`.
fn one_turn(end_usage: &str) -> (Result<TurnOutcome, WireError>, Vec<StreamEvent>) {
    let endpoint = Streaming::start(end_usage);
    let mut port = client(
        &endpoint.target(),
        Some(Source::key("synthetic-key").expect("a source")),
        &Cancel::new(),
    )
    .expect("a client");
    let mut sink = VecSink::new();
    let outcome = port.turn(&request(), &mut sink);
    (outcome, sink.events().to_vec())
}

fn is_extension(event: &StreamEvent) -> bool {
    matches!(event, StreamEvent::Warning { code, .. } if code == CODE)
}

const PLAIN_USAGE: &str = r#"{"output_tokens":8}"#;

/// A turn whose usage carries `cost` has exactly the outcome and the events of the same turn
/// without it, plus one `usage-extension` warning naming the path and carrying `reported`.
fn carried_like_the_turn_without_it(costed_usage: &str, reported: &str) {
    let (plain, plain_events) = one_turn(PLAIN_USAGE);
    let plain = plain.expect("the turn without a cost completes");
    assert_eq!(
        plain.items,
        vec![Item::AssistantText {
            text: "forty-two".to_owned()
        }]
    );
    assert_eq!(plain.stop_reason, StopReason::EndTurn);
    let usage = plain.usage.as_ref().expect("the turn reports usage");
    assert_eq!(
        usage.output_tokens, 8,
        "the message_delta figure: {usage:?}"
    );

    let (costed, costed_events) = one_turn(costed_usage);
    let costed = costed.unwrap_or_else(|error| {
        panic!("a cost on message_delta.usage completes the turn: {error:?}")
    });
    assert_eq!(
        costed, plain,
        "the cost reads into nothing: text, stop reason and usage are the plain turn's"
    );
    let warnings: Vec<&StreamEvent> = costed_events.iter().filter(|e| is_extension(e)).collect();
    assert_eq!(
        warnings.len(),
        1,
        "exactly one usage-extension warning reaches the sink: {costed_events:?}"
    );
    assert_eq!(
        warnings[0],
        &StreamEvent::Warning {
            code: CODE.to_owned(),
            message: format!("message_delta.usage.cost: {reported}"),
        },
        "the warning names the path and carries the value"
    );
    let others: Vec<StreamEvent> = costed_events
        .into_iter()
        .filter(|e| !is_extension(e))
        .collect();
    assert_eq!(
        others, plain_events,
        "nothing else the sink saw changed: the text delta still streamed"
    );
}

#[test]
fn the_code_matched_here_is_llms_own_constant() {
    assert_eq!(CODE, llm_messages::USAGE_EXTENSION);
}

#[test]
fn a_numeric_cost_on_message_delta_usage_completes_the_turn_with_one_usage_extension_warning() {
    carried_like_the_turn_without_it(r#"{"output_tokens":8,"cost":0.0123}"#, "0.0123");
}

#[test]
fn an_object_cost_on_message_delta_usage_completes_the_turn_with_one_usage_extension_warning() {
    carried_like_the_turn_without_it(
        r#"{"output_tokens":8,"cost":{"total":0.0123,"currency":"USD"}}"#,
        r#"{"currency":"USD","total":0.0123}"#,
    );
}
