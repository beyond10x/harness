#![forbid(unsafe_code)]

//! `OpenAI` Responses wire adapter for the b10x agent loop.
//!
//! It speaks one documented endpoint, `POST {base}/responses`, in streaming mode, and projects it
//! into [`harness_wire`]'s neutral values. It holds no credential: the bearer is read from an
//! injected source at call time and dropped when the call ends.
//!
//! # What is here and what is beneath
//!
//! Everything in this crate names an `OpenAI` field, header or event: the request projection, the
//! stream decoder, the three headers that identify the conversation, and the endpoint's path.
//! Everything *between* the HTTP client and that projection — bounded server-sent-event framing,
//! the retry rule, the back-off, the witnessed sink that makes the retry rule safe and the status
//! mapping — is [`harness_http`], which this crate configures with [`TRANSPORT`] and nothing else.
//! It lived here until the second wire copied all of it unchanged, which is what proved it was
//! transport-shaped rather than vendor-shaped.

mod project;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use harness_http::{Framing, Headers, HttpTransport, Settings, SseReader, StreamingPost};
use harness_wire::{
    BearerSource, Cancel, Item, ModelPort, StreamEvent, StreamSink, TurnOutcome, TurnRequest,
    WireError, WireErrorCode, WireId,
};
use serde_json::Value;

pub use project::request_body;

/// Identifies this projection. Opaque items carry it and may not be replayed into another wire.
pub const WIRE: &str = "openai-responses";

/// Every event discriminator the decoder interprets rather than preserving as unknown.
pub const ACCEPTED_STREAM_EVENTS: &[&str] = &[
    "error",
    "keepalive",
    "response.completed",
    "response.content_part.added",
    "response.content_part.done",
    "response.created",
    "response.failed",
    "response.function_call_arguments.delta",
    "response.function_call_arguments.done",
    "response.in_progress",
    "response.incomplete",
    "response.output_item.added",
    "response.output_item.done",
    "response.output_text.delta",
    "response.output_text.done",
    "response.reasoning_summary_part.added",
    "response.reasoning_summary_part.done",
    "response.reasoning_summary_text.delta",
    "response.reasoning_summary_text.done",
];

/// What this client calls itself on the wire.
///
/// Its own name and not a vendor's. A route that serves several clients is entitled to know which
/// one is calling, and a harness that answered with somebody else's name would be making its runs
/// unexplainable in exactly the way its credential handling exists to prevent.
const ORIGINATOR: &str = "b10x-harness";

/// A name for one conversation, stable for the life of a client and distinct between runs.
///
/// Not a UUID and not from a crate: what the wire needs is a string that is the same on every turn
/// of one run and different across runs, and the clock plus this process is enough for that. It
/// identifies nothing about the machine or the account.
fn new_session() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    format!("b10x-{:x}-{:x}", std::process::id(), nanos)
}

/// What this wire asks of [`harness_http`].
///
/// [`Framing::DoneSentinel`] is the one thing the two wires do not agree on: this route ends its
/// stream with `data: [DONE]`, and the other has no sentinel at all. Everything else — four
/// attempts, the 1 s/2 s/4 s back-off, the two timeouts and the bounded error body — is the
/// shared default, and `crates/harness-messages/tests/transport.rs` fails if the two wires ever
/// stop agreeing about it without saying so.
pub const TRANSPORT: Settings = Settings::streaming(Framing::DoneSentinel);

/// One endpoint serving one model.
///
/// These are exactly the three facts an `OpenAI`-compatible gateway needs, so a route already
/// configured for another client transfers unchanged. The credential is deliberately not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Origin plus API prefix, for example `https://llmgw.dev.babelforce.com/v1`.
    pub base_url: String,
    /// The exact model identifier the endpoint serves.
    pub model: String,
    /// The context window the endpoint serves for that model.
    pub context_window: u64,
}

impl Endpoint {
    /// Builds an endpoint, refusing one that could not serve a request.
    ///
    /// # Errors
    ///
    /// Returns [`WireErrorCode::Protocol`] for a relative base URL, an unnamed model, or a
    /// context window of zero.
    pub fn new(
        base_url: impl Into<String>,
        model: impl Into<String>,
        context_window: u64,
    ) -> Result<Self, WireError> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        let model = model.into();
        if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            return Err(WireError::protocol(
                "the base URL must be an absolute http or https URL",
            ));
        }
        if model.is_empty() {
            return Err(WireError::protocol("an endpoint must name a model"));
        }
        if context_window == 0 {
            return Err(WireError::protocol(
                "a context window of zero admits no request",
            ));
        }
        Ok(Self {
            base_url,
            model,
            context_window,
        })
    }

    fn responses_url(&self) -> String {
        format!("{}/responses", self.base_url)
    }
}

pub struct ResponsesClient {
    wire: WireId,
    endpoint: Endpoint,
    /// This run's conversation identity.
    ///
    /// One value for the life of the client, which is the life of a run: it names the conversation
    /// on the wire (`session-id`) and routes every turn to the same prompt cache
    /// (`prompt_cache_key`). Both are the same string on purpose — that is the shape the endpoint
    /// was observed honouring, serving `codex` at 85% cached where this loop was getting nothing.
    session: String,
    /// Counts requests, so each carries its own `x-client-request-id`. A retry is a new request
    /// and says so; an id that never changed would name every request of a run the same thing.
    requests: AtomicU64,
    /// [`None`] sends no `authorization` header at all.
    ///
    /// Not the same as an empty credential, which is refused below: an empty bearer means a
    /// credential source answered with nothing and the run would fail in a way nobody could
    /// explain. `None` means the caller named no source, which is the right shape for a gateway on
    /// this machine that authenticates nobody — and for a run declared with no credential, whose
    /// first request is expected to be refused by the far end rather than by this client.
    bearer: Option<Arc<dyn BearerSource>>,
    /// The transport half, configured by [`TRANSPORT`] and shared with the other wire.
    http: HttpTransport,
}

impl std::fmt::Debug for ResponsesClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ResponsesClient")
            .field("wire", &self.wire)
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl ResponsesClient {
    /// Builds a client for one endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`WireErrorCode::Transport`] when the HTTP client cannot be constructed.
    ///
    /// # Panics
    ///
    /// Panics only if [`WIRE`], a source-controlled constant, stops being a valid identifier.
    pub fn new(endpoint: Endpoint, bearer: Arc<dyn BearerSource>) -> Result<Self, WireError> {
        Self::build(endpoint, Some(bearer))
    }

    /// A client that sends no `authorization` header.
    ///
    /// For an endpoint that authenticates nobody — a gateway on this machine — and for a run
    /// declared with no credential, where being refused by the far end is the wanted outcome.
    /// Separate constructor rather than an `Option` on [`Self::new`], so that a run reaching an
    /// endpoint unauthenticated is something a caller wrote down.
    ///
    /// # Errors
    ///
    /// Returns [`WireErrorCode::Transport`] when the HTTP client cannot be constructed.
    pub fn unauthenticated(endpoint: Endpoint) -> Result<Self, WireError> {
        Self::build(endpoint, None)
    }

    fn build(endpoint: Endpoint, bearer: Option<Arc<dyn BearerSource>>) -> Result<Self, WireError> {
        Ok(Self {
            wire: WireId::new(WIRE).expect("the wire id constant is valid"),
            endpoint,
            session: new_session(),
            requests: AtomicU64::new(0),
            bearer,
            http: HttpTransport::new(TRANSPORT)?,
        })
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Shares this client's cancellation token.
    ///
    /// Cancelling it drops the response body being read, so a completion that arrives afterwards
    /// cannot win. A harness whose cancel is advisory is one a person cannot actually stop.
    pub fn cancel_handle(&self) -> Cancel {
        self.http.cancel_handle()
    }

    /// Adopts a token shared with the rest of a turn, rather than owning a private one.
    #[must_use]
    pub fn with_cancel(mut self, cancel: Cancel) -> Self {
        self.http.adopt_cancel(cancel);
        self
    }

    /// One turn, over the shared transport.
    ///
    /// What this wire contributes is the URL, the headers and the decoder; the attempt loop, the
    /// retry rule and the back-off are [`harness_http`]'s
    /// ([`HttpTransport::stream_turn`]).
    fn attempt_turn(
        &self,
        body: &Value,
        model: &str,
        sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        let url = self.endpoint.responses_url();
        let headers = || self.headers();
        let server_delay = harness_wire::retry_after;
        let post = StreamingPost {
            wire: WIRE,
            url: &url,
            body,
            headers: &headers,
            server_delay: &server_delay,
        };
        self.http.stream_turn(&post, sink, |reader, sink| {
            drain(reader, &self.wire, model, sink)
        })
    }

    /// The headers one attempt carries.
    ///
    /// Rebuilt per attempt, which is what keeps two properties true: the credential is fetched at
    /// call time and dropped when the request is built, and a retry is a **new** request with its
    /// own `x-client-request-id` rather than a second send under the first one's name.
    ///
    /// # Errors
    ///
    /// Whatever the bearer source refuses with, or [`WireErrorCode::Unauthorized`] when it
    /// answered with nothing.
    fn headers(&self) -> Result<Headers, WireError> {
        let request = self.requests.fetch_add(1, Ordering::Relaxed);
        let mut headers = request_headers(&self.session, request);
        // Held only for as long as it takes to become a header, and dropped before the send that
        // can block — the same custody the credential had before it became optional.
        if let Some(source) = &self.bearer {
            let bearer = source.bearer()?;
            if bearer.is_empty() {
                return Err(WireError::unauthorized(
                    "the bearer source returned an empty credential",
                ));
            }
            headers.push(("authorization", format!("Bearer {}", bearer.expose())));
            drop(bearer);
        }
        Ok(headers)
    }
}

fn request_headers(session: &str, request: u64) -> Headers {
    vec![
        ("accept", "text/event-stream".to_owned()),
        ("content-type", "application/json".to_owned()),
        ("originator", ORIGINATOR.to_owned()),
        ("session-id", session.to_owned()),
        ("x-client-request-id", format!("{session}-{request}")),
    ]
}

/// Every non-secret header and value a request carries, using caller-fixed request metadata.
///
/// This is the production builder used by [`ResponsesClient`], exposed so the wire contract can
/// pin dynamic values without fetching or representing a credential.
pub fn contract_headers(session: &str, request: u64) -> Headers {
    request_headers(session, request)
}

/// A stream that stopped before the response reached a terminal state.
///
/// **Framing by its code, transport by its cause, and retriable because of the cause.** Nothing
/// malformed arrived: the far side simply stopped sending and closed, which is what a dropped
/// connection looks like from in here and not what a peer speaking a different protocol looks
/// like. The identical request very likely answers, so the flag says so. The code stays
/// [`WireErrorCode::Protocol`] because that is what was *observed* — a response that did not match
/// the pinned subset — and the two fields answer different questions: one names what happened, the
/// other says what to do about it.
///
/// The wire still will not resend once anything has been emitted; see [`WitnessedSink`].
fn ended_before_a_terminal_response() -> WireError {
    WireError::new(
        WireErrorCode::Protocol,
        "the stream ended before the response reached a terminal state",
        true,
    )
}

/// Accumulates one streamed response into a turn outcome.
struct TurnDecoder<'a> {
    wire: &'a WireId,
    model: &'a str,
    /// `item_id` -> `call_id`, so an arguments delta can name the call a reader is watching.
    calls: BTreeMap<String, harness_wire::CallId>,
    streamed: Vec<Item>,
    /// Opaque state that must survive even when a terminal object explicitly replaces the
    /// modelled streamed output.
    unmodelled: Vec<Item>,
    terminal: Option<Value>,
}

impl<'a> TurnDecoder<'a> {
    fn new(wire: &'a WireId, model: &'a str) -> Self {
        Self {
            wire,
            model,
            calls: BTreeMap::new(),
            streamed: Vec::new(),
            unmodelled: Vec::new(),
            terminal: None,
        }
    }

    fn apply(&mut self, event: &Value, sink: &mut dyn StreamSink) -> Result<(), WireError> {
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(text) = event.get("delta").and_then(Value::as_str) {
                    sink.emit(StreamEvent::TextDelta {
                        text: text.to_owned(),
                    });
                }
            }
            Some("response.output_item.added") => {
                self.remember_call(event);
            }
            // **A person watching a long think sees something.** This is the summary the provider
            // chose to show, not the chain of thought, and it is shown and then let go: nothing
            // here is added to `streamed` or replayed on the next turn. What carries the reasoning
            // across a tool round trip is the opaque `reasoning` item the turn ends with, which
            // this wire asks for by name (`include: reasoning.encrypted_content`). A turn with no
            // reasoning summary is silent here and complete anyway.
            Some("response.reasoning_summary_text.delta") => {
                if let Some(text) = event.get("delta").and_then(Value::as_str) {
                    sink.emit(StreamEvent::ReasoningDelta {
                        text: text.to_owned(),
                    });
                }
            }
            Some("response.function_call_arguments.delta") => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str)
                    && let Some(call_id) = self.call_for(event)
                {
                    sink.emit(StreamEvent::ToolArgumentsDelta {
                        call_id,
                        delta: delta.to_owned(),
                    });
                }
            }
            Some("response.output_item.done") => {
                if let Some(item) = event.get("item") {
                    let mut warnings = Vec::new();
                    let decoded =
                        project::output_item_to_item(self.wire, item, &mut |code, message| {
                            warnings.push((code, message));
                        });
                    for (code, message) in warnings {
                        sink.emit(StreamEvent::Warning { code, message });
                    }
                    let decoded = decoded?;
                    if matches!(decoded, Item::Opaque { .. }) {
                        self.unmodelled.push(decoded.clone());
                    }
                    self.streamed.push(decoded);
                }
            }
            Some("response.completed" | "response.incomplete") => {
                self.terminal = event.get("response").cloned();
            }
            Some("response.failed") => {
                return Err(project::response_error(
                    event.get("response").unwrap_or(event),
                ));
            }
            Some("error") => {
                return Err(project::response_error(event));
            }
            // Progress markers the loop does not act on; the terminal object is authoritative.
            //
            // The three reasoning-summary markers are here rather than warned about because each
            // one repeats text a `response.reasoning_summary_text.delta` already carried: `.done`
            // restates the completed summary, and the `part` pair only brackets it. Emitting them
            // would show a reader the same sentence twice.
            Some(
                "keepalive"
                | "response.created"
                | "response.in_progress"
                | "response.content_part.added"
                | "response.content_part.done"
                | "response.output_text.done"
                | "response.function_call_arguments.done"
                | "response.reasoning_summary_text.done"
                | "response.reasoning_summary_part.added"
                | "response.reasoning_summary_part.done",
            ) => {}
            other => {
                let kind = other.unwrap_or("<absent>").to_owned();
                sink.emit(StreamEvent::Warning {
                    code: "unknown-stream-event".to_owned(),
                    message: format!(
                        "stream event `{kind}` is outside the pinned subset and was preserved but not interpreted"
                    ),
                });
                let item = Item::Opaque {
                    wire: self.wire.clone(),
                    payload: event.clone(),
                };
                self.streamed.push(item.clone());
                self.unmodelled.push(item);
            }
        }
        Ok(())
    }

    fn remember_call(&mut self, event: &Value) {
        let Some(item) = event.get("item") else {
            return;
        };
        if item.get("type").and_then(Value::as_str) != Some("function_call") {
            return;
        }
        let (Some(item_id), Some(call_id)) = (
            item.get("id").and_then(Value::as_str),
            item.get("call_id").and_then(Value::as_str),
        ) else {
            return;
        };
        if let Ok(call_id) = harness_wire::CallId::new(call_id) {
            self.calls.insert(item_id.to_owned(), call_id);
        }
    }

    fn call_for(&self, event: &Value) -> Option<harness_wire::CallId> {
        event
            .get("item_id")
            .and_then(Value::as_str)
            .and_then(|item_id| self.calls.get(item_id))
            .cloned()
    }

    fn finish(self, sink: &mut dyn StreamSink) -> Result<TurnOutcome, WireError> {
        let terminal = self.terminal.ok_or_else(ended_before_a_terminal_response)?;
        let mut warn =
            |code: String, message: String| sink.emit(StreamEvent::Warning { code, message });
        // The terminal object is authoritative when it carries output; the streamed items are the
        // fallback for a server that reports completion without repeating them.
        let mut items = match terminal.get("output").and_then(Value::as_array) {
            Some(output) => output
                .iter()
                .map(|value| project::output_item_to_item(self.wire, value, &mut warn))
                .collect::<Result<Vec<_>, _>>()?,
            None => self.streamed,
        };
        for item in self.unmodelled {
            if !items.contains(&item) {
                items.push(item);
            }
        }
        let has_tool_calls = items.iter().any(|item| item.as_tool_call().is_some());
        Ok(TurnOutcome {
            stop_reason: project::stop_reason(&terminal, has_tool_calls),
            usage: project::usage_from_response(&terminal, self.model),
            items,
        })
    }
}

impl ResponsesClient {
    /// One turn, over a shared reference.
    ///
    /// [`ModelPort::turn`] takes `&mut self` because a port may need it; this one never did — the
    /// transport posts on `&self` and the request counter is an atomic. Written here so that
    /// [`ModelPort::fork`] can hand a second agent loop a handle on this same client rather than a
    /// copy of it.
    fn turn_shared(
        &self,
        request: &TurnRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        request.validate()?;
        request.check_opaque_items(&self.wire)?;
        // Beside the other two pre-flight checks, and for the same reason: a request this wire
        // cannot carry is refused here, naming what is wrong, rather than posted and explained by
        // the far side.
        project::check_tool_names(&request.tools)?;
        let body = project::request_body(&self.session, request);
        self.attempt_turn(&body, &request.model, sink)
    }
}

impl ModelPort for ResponsesClient {
    fn wire(&self) -> &WireId {
        &self.wire
    }

    fn turn(
        &mut self,
        request: &TurnRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        self.turn_shared(request, sink)
    }

    /// A second handle on this client, for a loop running two children side by side.
    ///
    /// Borrowed rather than cloned, and here the sharing is load-bearing twice over. The request
    /// counter is what makes every `x-client-request-id` of a run distinct; two clients counting
    /// from zero would name two different requests the same thing. And `session` names this run's
    /// conversation and routes it to one prompt cache — children share the standing instruction
    /// that forms the cached prefix, so they should share the key that finds it.
    fn fork(&self) -> Option<Box<dyn ModelPort + Send + '_>> {
        Some(Box::new(Forked(self)))
    }
}

/// A borrowed [`ResponsesClient`], published as its own [`ModelPort`].
struct Forked<'a>(&'a ResponsesClient);

impl ModelPort for Forked<'_> {
    fn wire(&self) -> &WireId {
        &self.0.wire
    }

    fn turn(
        &mut self,
        request: &TurnRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        self.0.turn_shared(request, sink)
    }

    /// A fork of a fork is a fork of the same client, not a chain of them.
    fn fork(&self) -> Option<Box<dyn ModelPort + Send + '_>> {
        Some(Box::new(Forked(self.0)))
    }
}

/// Decodes one complete event stream into a turn outcome.
///
/// Exposed because the contract fixtures replay a recorded stream through exactly the code a live
/// turn uses. A conformance suite that decodes through a second path proves only that the second
/// path works.
///
/// # Errors
///
/// Returns the same typed refusals a live turn does: framing, bounds, provider failure, and a
/// stream that ends before a terminal response.
///
/// # Panics
///
/// Panics only if [`WIRE`], a source-controlled constant, stops being a valid identifier.
pub fn decode_stream<R: std::io::BufRead>(
    model: &str,
    reader: R,
    sink: &mut dyn StreamSink,
) -> Result<TurnOutcome, WireError> {
    let wire = WireId::new(WIRE).expect("the wire id constant is valid");
    drain(
        SseReader::new(reader, TRANSPORT.framing),
        &wire,
        model,
        sink,
    )
}

/// Reads one stream to its end and hands the decoder every payload.
///
/// Generic over the reader so a live turn and a pinned fixture go through the same code. The
/// sentinel this route ends with is the framer's business and never reaches the decoder — see
/// [`harness_http::Framing`].
fn drain<R: std::io::BufRead>(
    mut reader: SseReader<R>,
    wire: &WireId,
    model: &str,
    sink: &mut dyn StreamSink,
) -> Result<TurnOutcome, WireError> {
    let mut decoder = TurnDecoder::new(wire, model);
    while let Some(payload) = reader.next_payload()? {
        decoder.apply(&payload, sink)?;
    }
    decoder.finish(sink)
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_wire::{StopReason, VecSink};
    use serde_json::json;

    fn wire() -> WireId {
        WireId::new(WIRE).expect("valid")
    }

    fn drive(events: &[Value]) -> (Result<TurnOutcome, WireError>, VecSink) {
        let wire = wire();
        let model = "test-model".to_owned();
        let mut sink = VecSink::new();
        let mut decoder = TurnDecoder::new(&wire, &model);
        let mut failure = None;
        for event in events {
            if let Err(error) = decoder.apply(event, &mut sink) {
                failure = Some(error);
                break;
            }
        }
        let outcome = match failure {
            Some(error) => Err(error),
            None => decoder.finish(&mut sink),
        };
        (outcome, sink)
    }

    #[test]
    fn endpoints_refuse_a_relative_base_or_an_empty_model() {
        assert!(Endpoint::new("https://gw.example/v1", "m", 8192).is_ok());
        assert!(Endpoint::new("gw.example/v1", "m", 8192).is_err());
        assert!(Endpoint::new("https://gw.example/v1", "", 8192).is_err());
        assert!(Endpoint::new("https://gw.example/v1", "m", 0).is_err());
    }

    #[test]
    fn a_trailing_slash_does_not_double_in_the_url() {
        let endpoint = Endpoint::new("https://gw.example/v1/", "m", 8192).expect("valid");
        assert_eq!(endpoint.responses_url(), "https://gw.example/v1/responses");
    }

    #[test]
    fn streamed_text_reaches_the_sink_and_the_outcome() {
        let (outcome, sink) = drive(&[
            json!({"type": "response.output_text.delta", "delta": "Hel"}),
            json!({"type": "response.output_text.delta", "delta": "lo"}),
            json!({"type": "response.completed", "response": {
                "status": "completed",
                "model": "test-model",
                "output": [{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Hello"}]}],
                "usage": {"input_tokens": 3, "output_tokens": 2, "input_tokens_details": {"cached_tokens": 0}},
            }}),
        ]);
        let outcome = outcome.expect("the turn completes");
        assert_eq!(sink.text(), "Hello");
        assert_eq!(outcome.items, vec![Item::assistant("Hello")]);
        assert_eq!(outcome.stop_reason, StopReason::EndTurn);
        assert_eq!(outcome.usage.expect("usage was reported").output_tokens, 2);
    }

    #[test]
    fn a_tool_call_correlates_its_argument_deltas() {
        let (outcome, sink) = drive(&[
            json!({"type": "response.output_item.added", "item": {
                "id": "fc_1", "type": "function_call", "call_id": "call-1",
                "name": "workspace_read", "arguments": "",
            }}),
            json!({"type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": "{\"p\":1}"}),
            json!({"type": "response.completed", "response": {
                "status": "completed",
                "output": [{"type":"function_call","call_id":"call-1","name":"workspace_read","arguments":"{\"p\":1}"}],
            }}),
        ]);
        let outcome = outcome.expect("the turn completes");
        assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
        assert_eq!(outcome.tool_calls().count(), 1);
        assert!(matches!(
            sink.events().first(),
            Some(StreamEvent::ToolArgumentsDelta { call_id, .. }) if call_id.as_str() == "call-1"
        ));
    }

    #[test]
    fn a_stream_without_a_terminal_event_refuses_and_is_worth_another_attempt() {
        let (outcome, _) = drive(&[json!({"type": "response.output_text.delta", "delta": "x"})]);
        let error = outcome.expect_err("an unterminated stream refuses");
        assert_eq!(error.code, WireErrorCode::Protocol);
        assert!(
            error.retriable,
            "a far side that closed cleanly mid-turn is a dropped connection"
        );
    }

    #[test]
    fn a_stream_that_dies_mid_way_is_reported_as_worth_another_attempt() {
        // Through `decode_stream`, the same entry point a live turn uses, on bytes that stop in
        // the middle of a frame. This is the failure a network blip on turn twenty produces, and
        // the flag is the only thing that tells the loop the run is not lost.
        let mut sink = VecSink::new();
        let error = decode_stream(
            "test-model",
            &b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel"[..],
            &mut sink,
        )
        .expect_err("a stream that stops mid-frame refuses");
        assert_eq!(error.code, WireErrorCode::Protocol);
        assert!(error.retriable, "{error}");
    }

    #[test]
    fn a_malformed_event_is_refused_for_good() {
        let mut sink = VecSink::new();
        let error = decode_stream("test-model", &b"data: {not json}\n\n"[..], &mut sink)
            .expect_err("a malformed event refuses");
        assert_eq!(error.code, WireErrorCode::Protocol);
        assert!(
            !error.retriable,
            "the same bytes would be malformed a second time"
        );
    }

    #[test]
    fn a_failed_response_refuses_with_the_provider_reason() {
        let (outcome, _) = drive(&[json!({
            "type": "response.failed",
            "response": {
                "status": "failed",
                "error": {"code": "invalid_prompt", "message": "boom"},
            },
        })]);
        let error = outcome.expect_err("a failed response refuses");
        assert_eq!(error.code, WireErrorCode::Refused);
        assert!(!error.retriable, "the same prompt is refused identically");
        assert!(error.message.contains("boom"), "{}", error.message);
    }

    #[test]
    fn a_failure_on_the_providers_own_account_ends_the_turn_but_not_the_run() {
        let (outcome, _) = drive(&[json!({
            "type": "response.failed",
            "response": {"status": "failed", "error": {"code": "server_error", "message": "boom"}},
        })]);
        let error = outcome.expect_err("a failed response refuses");
        assert_eq!(error.code, WireErrorCode::Transport);
        assert!(error.retriable, "{error}");
    }

    #[test]
    fn two_reasoning_summary_deltas_reach_a_reader_and_the_turn_still_completes() {
        // Finding #11: a person watching a long think saw nothing at all, because the event that
        // carries the summary was in the ignored list.
        let (outcome, sink) = drive(&[
            json!({"type": "response.reasoning_summary_part.added", "summary_index": 0}),
            json!({"type": "response.reasoning_summary_text.delta", "delta": "Weighing "}),
            json!({"type": "response.reasoning_summary_text.delta", "delta": "the options."}),
            json!({"type": "response.reasoning_summary_text.done", "text": "Weighing the options."}),
            json!({"type": "response.reasoning_summary_part.done", "summary_index": 0}),
            json!({"type": "response.output_text.delta", "delta": "Hello"}),
            json!({"type": "response.completed", "response": {
                "status": "completed",
                "output": [
                    {"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "OPAQUE"},
                    {"type": "message", "role": "assistant",
                     "content": [{"type": "output_text", "text": "Hello"}]},
                ],
            }}),
        ]);
        let outcome = outcome.expect("the turn completes");

        let reasoning: Vec<&str> = sink
            .events()
            .iter()
            .filter_map(|event| match event {
                StreamEvent::ReasoningDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(reasoning, vec!["Weighing ", "the options."]);
        // The markers around them carry no new text, so a reader is not shown the summary twice
        // and the stream is not reported as drifted.
        assert!(
            sink.events()
                .iter()
                .all(|event| !matches!(event, StreamEvent::Warning { .. })),
            "{:?}",
            sink.events()
        );
        assert_eq!(sink.text(), "Hello");
        // What is shown is let go; what is replayed is the opaque item, still there.
        assert!(
            outcome.items.iter().any(|item| matches!(
                item,
                Item::Opaque { payload, .. } if payload["encrypted_content"] == json!("OPAQUE")
            )),
            "{:?}",
            outcome.items
        );
    }

    #[test]
    fn a_summary_delta_with_no_text_emits_nothing_rather_than_an_empty_fragment() {
        let (outcome, sink) = drive(&[
            json!({"type": "response.reasoning_summary_text.delta"}),
            json!({"type": "response.completed", "response": {"status": "completed", "output": []}}),
        ]);
        assert!(outcome.is_ok());
        assert!(sink.events().is_empty(), "{:?}", sink.events());
    }

    #[test]
    fn an_unknown_stream_event_warns_instead_of_vanishing() {
        let (outcome, sink) = drive(&[
            json!({"type": "response.something_new", "data": 1}),
            json!({"type": "response.completed", "response": {"status": "completed", "output": []}}),
        ]);
        let outcome = outcome.expect("unknown state is preserved");
        assert!(
            sink.events().iter().any(|event| matches!(
                event,
                StreamEvent::Warning { code, .. } if code == "unknown-stream-event"
            )),
            "{:?}",
            sink.events()
        );
        assert!(outcome.items.iter().any(|item| matches!(
            item,
            Item::Opaque { payload, .. }
                if payload["type"] == json!("response.something_new")
        )));
    }

    #[test]
    fn a_keepalive_advances_no_turn_and_is_never_replayed() {
        let (outcome, sink) = drive(&[
            json!({"type": "keepalive"}),
            json!({"type": "response.completed", "response": {"status": "completed", "output": []}}),
        ]);
        let outcome = outcome.expect("a keepalive is modeled progress");
        assert!(sink.events().is_empty(), "{:?}", sink.events());
        assert!(outcome.items.is_empty(), "{:?}", outcome.items);
    }

    #[test]
    fn an_explicitly_empty_terminal_output_does_not_resurrect_a_streamed_call() {
        let (outcome, _) = drive(&[
            json!({"type": "response.output_item.done", "item": {
                "type": "function_call", "call_id": "call-1", "name": "read",
                "arguments": "{}"
            }}),
            json!({"type": "response.completed", "response": {
                "status": "completed", "output": []
            }}),
        ]);
        assert!(outcome.expect("terminal is authoritative").items.is_empty());
    }

    #[test]
    fn streamed_items_are_the_fallback_when_the_terminal_object_omits_output() {
        let (outcome, _) = drive(&[
            json!({"type": "response.output_item.done", "item": {
                "type": "message", "role": "assistant",
                "content": [{"type": "output_text", "text": "from the stream"}],
            }}),
            json!({"type": "response.completed", "response": {"status": "completed"}}),
        ]);
        assert_eq!(
            outcome.expect("the turn completes").items,
            vec![Item::assistant("from the stream")]
        );
    }

    #[test]
    fn an_incomplete_response_reports_the_budget_that_cut_it() {
        let (outcome, _) = drive(&[json!({
            "type": "response.incomplete",
            "response": {
                "status": "incomplete",
                "incomplete_details": {"reason": "max_output_tokens"},
                "output": [],
            },
        })]);
        assert_eq!(
            outcome
                .expect("an incomplete turn is still an outcome")
                .stop_reason,
            StopReason::MaxOutputTokens
        );
    }

    #[test]
    fn a_foreign_opaque_item_is_refused_by_name_rather_than_replayed() {
        // AGENTS.md invariant 5, from this side. The symmetric case lives in `harness-messages`;
        // both exist because the failure is asymmetric in practice — a `thinking` block carries a
        // signature the other provider cannot verify, and a `reasoning` blob is encrypted to an
        // account this route knows nothing about. Either one, replayed, is at best a hard error
        // and at worst silently poisoned context.
        use harness_wire::StaticBearer;

        let endpoint = Endpoint::new("https://gw.example/v1", "m", 8192).expect("valid");
        let mut client = ResponsesClient::new(
            endpoint,
            std::sync::Arc::new(StaticBearer::new("synthetic")),
        )
        .expect("the client builds");
        let request = TurnRequest {
            model: "m".to_owned(),
            instructions: String::new(),
            items: vec![
                Item::user("hi"),
                Item::Opaque {
                    wire: WireId::new("anthropic-messages").expect("valid"),
                    payload: json!({"type": "thinking", "signature": "SIG"}),
                },
            ],
            tools: Vec::new(),
            max_output_tokens: None,
            sampling: harness_wire::Sampling::default(),
            tool_choice: harness_wire::ToolChoice::Auto,
        };
        let mut sink = VecSink::new();
        let error = client
            .turn(&request, &mut sink)
            .expect_err("a foreign opaque item refuses");
        assert_eq!(error.code, WireErrorCode::Unsupported);
        assert!(error.message.contains("anthropic-messages"), "{error}");
        assert!(error.message.contains(WIRE), "{error}");
    }

    #[test]
    fn every_header_this_wire_sends_names_the_conversation_and_carries_the_credential() {
        // The transport is handed a list, so the list is what there is to check. A header dropped
        // here is a turn the far end cannot route to the prompt cache, and it would not show up in
        // any decoder test.
        use harness_wire::StaticBearer;

        let endpoint = Endpoint::new("https://gw.example/v1", "m", 8192).expect("valid");
        let client = ResponsesClient::new(endpoint, Arc::new(StaticBearer::new("synthetic")))
            .expect("the client builds");
        let headers = client.headers().expect("the headers build");
        let names: Vec<&str> = headers.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            names,
            vec![
                "accept",
                "content-type",
                "originator",
                "session-id",
                "x-client-request-id",
                "authorization",
            ]
        );
        assert!(headers.contains(&("authorization", "Bearer synthetic".to_owned())));
        // A retry is a new request and says so: the id changes, the conversation does not.
        let second = client.headers().expect("the headers build again");
        let id = |headers: &Headers, wanted: &str| {
            headers
                .iter()
                .find(|(name, _)| *name == wanted)
                .map(|(_, value)| value.clone())
                .expect("the header is present")
        };
        assert_ne!(
            id(&headers, "x-client-request-id"),
            id(&second, "x-client-request-id")
        );
        assert_eq!(id(&headers, "session-id"), id(&second, "session-id"));
    }

    #[test]
    fn a_client_with_no_credential_source_sends_no_authorization_header() {
        let endpoint = Endpoint::new("https://gw.example/v1", "m", 8192).expect("valid");
        let client = ResponsesClient::unauthenticated(endpoint).expect("the client builds");
        let headers = client.headers().expect("the headers build");
        assert!(headers.iter().all(|(name, _)| *name != "authorization"));
    }
}
