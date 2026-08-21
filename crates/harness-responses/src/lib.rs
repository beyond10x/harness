#![forbid(unsafe_code)]

//! `OpenAI` Responses wire adapter for Daemonloom's own agent loop.
//!
//! It speaks one documented endpoint, `POST {base}/responses`, in streaming mode, and projects it
//! into [`harness_wire`]'s neutral values. It holds no credential: the bearer is read from an
//! injected source at call time and dropped when the call ends.

mod project;
mod sse;

use std::collections::BTreeMap;
use std::io::{BufReader, Read};
use std::sync::Arc;
use std::time::Duration;

use harness_wire::{
    BearerSource, Cancel, Item, ModelPort, StreamEvent, StreamSink, TurnOutcome, TurnRequest,
    WireError, WireErrorCode, WireId,
};
use serde_json::Value;

pub use project::request_body;
pub use sse::{MAX_EVENT_BYTES, MAX_STREAM_BYTES, SseEvent, SseReader};

/// Identifies this projection. Opaque items carry it and may not be replayed into another wire.
pub const WIRE: &str = "openai-responses";

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Applies to each individual read, not to the turn.
///
/// A streamed turn may legitimately run for minutes; what must not happen is a peer that accepts
/// the connection and then says nothing, which would hold the loop open with no way out.
const DEFAULT_OPERATION_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_ERROR_BODY_BYTES: usize = 2048;

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
    bearer: Arc<dyn BearerSource>,
    http: reqwest::blocking::Client,
    cancel: Cancel,
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
        let http = reqwest::blocking::Client::builder()
            .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
            .timeout(DEFAULT_OPERATION_TIMEOUT)
            .build()
            .map_err(|error| WireError::transport(format!("building the HTTP client: {error}")))?;
        Ok(Self {
            wire: WireId::new(WIRE).expect("the wire id constant is valid"),
            endpoint,
            bearer,
            http,
            cancel: Cancel::new(),
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
        self.cancel.clone()
    }

    /// Adopts a token shared with the rest of a turn, rather than owning a private one.
    #[must_use]
    pub fn with_cancel(mut self, cancel: Cancel) -> Self {
        self.cancel = cancel;
        self
    }

    fn send(&self, body: &Value) -> Result<reqwest::blocking::Response, WireError> {
        if self.cancel.is_cancelled() {
            return Err(WireError::cancelled());
        }
        let bearer = self.bearer.bearer()?;
        if bearer.is_empty() {
            return Err(WireError::unauthorized(
                "the bearer source returned an empty credential",
            ));
        }
        let response = self
            .http
            .post(self.endpoint.responses_url())
            .header("authorization", format!("Bearer {}", bearer.expose()))
            .header("accept", "text/event-stream")
            .header("content-type", "application/json")
            .json(body)
            .send()
            .map_err(|error| {
                WireError::transport(format!(
                    "posting to {}: {error}",
                    self.endpoint.responses_url()
                ))
            })?;
        drop(bearer);
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        Err(status_error(status, &read_bounded_body(response)))
    }
}

fn read_bounded_body(response: reqwest::blocking::Response) -> String {
    let mut body = String::new();
    let _ = response
        .take(MAX_ERROR_BODY_BYTES as u64)
        .read_to_string(&mut body);
    body.trim().to_owned()
}

fn status_error(status: reqwest::StatusCode, body: &str) -> WireError {
    let (code, retriable) = match status.as_u16() {
        401 | 403 => (WireErrorCode::Unauthorized, false),
        429 => (WireErrorCode::RateLimited, true),
        // A gateway that is still starting a backend answers 503; that is worth another attempt.
        500..=599 => (WireErrorCode::Transport, true),
        _ => (WireErrorCode::Refused, false),
    };
    WireError::new(code, format!("{WIRE} answered {status}: {body}"), retriable)
}

/// Accumulates one streamed response into a turn outcome.
struct TurnDecoder<'a> {
    wire: &'a WireId,
    model: &'a str,
    /// `item_id` -> `call_id`, so an arguments delta can name the call a reader is watching.
    calls: BTreeMap<String, harness_wire::CallId>,
    streamed: Vec<Item>,
    terminal: Option<Value>,
}

impl<'a> TurnDecoder<'a> {
    fn new(wire: &'a WireId, model: &'a str) -> Self {
        Self {
            wire,
            model,
            calls: BTreeMap::new(),
            streamed: Vec::new(),
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
                    self.streamed.push(decoded?);
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
            Some(
                "response.created"
                | "response.in_progress"
                | "response.content_part.added"
                | "response.content_part.done"
                | "response.output_text.done"
                | "response.function_call_arguments.done"
                | "response.reasoning_summary_text.delta"
                | "response.reasoning_summary_part.added"
                | "response.reasoning_summary_part.done",
            ) => {}
            other => {
                let kind = other.unwrap_or("<absent>").to_owned();
                sink.emit(StreamEvent::Warning {
                    code: "unknown-stream-event".to_owned(),
                    message: format!(
                        "stream event `{kind}` is outside the pinned subset and was skipped"
                    ),
                });
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
        let terminal = self.terminal.ok_or_else(|| {
            WireError::protocol("the stream ended before the response reached a terminal state")
        })?;
        let mut warn =
            |code: String, message: String| sink.emit(StreamEvent::Warning { code, message });
        // The terminal object is authoritative when it carries output; the streamed items are the
        // fallback for a server that reports completion without repeating them.
        let items = match terminal.get("output").and_then(Value::as_array) {
            Some(output) if !output.is_empty() => output
                .iter()
                .map(|value| project::output_item_to_item(self.wire, value, &mut warn))
                .collect::<Result<Vec<_>, _>>()?,
            _ => self.streamed,
        };
        let has_tool_calls = items.iter().any(|item| item.as_tool_call().is_some());
        Ok(TurnOutcome {
            stop_reason: project::stop_reason(&terminal, has_tool_calls),
            usage: project::usage_from_response(&terminal, self.model),
            items,
        })
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
        request.validate()?;
        request.check_opaque_items(&self.wire)?;
        let body = project::request_body(
            &request.model,
            &request.instructions,
            &request.items,
            &request.tools,
            request.max_output_tokens,
            &request.sampling,
        );
        let response = self.send(&body)?;
        let reader = SseReader::new(BufReader::new(response)).with_cancel(self.cancel.clone());
        drain(reader, &self.wire, &request.model, sink)
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
    drain(SseReader::new(reader), &wire, model, sink)
}

fn drain<R: std::io::BufRead>(
    mut reader: SseReader<R>,
    wire: &WireId,
    model: &str,
    sink: &mut dyn StreamSink,
) -> Result<TurnOutcome, WireError> {
    let mut decoder = TurnDecoder::new(wire, model);
    while let Some(event) = reader.next_event()? {
        match event {
            SseEvent::Done => break,
            SseEvent::Payload(payload) => decoder.apply(&payload, sink)?,
        }
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
                "name": "workspace.read", "arguments": "",
            }}),
            json!({"type": "response.function_call_arguments.delta", "item_id": "fc_1", "delta": "{\"p\":1}"}),
            json!({"type": "response.completed", "response": {
                "status": "completed",
                "output": [{"type":"function_call","call_id":"call-1","name":"workspace.read","arguments":"{\"p\":1}"}],
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
    fn a_stream_without_a_terminal_event_refuses() {
        let (outcome, _) = drive(&[json!({"type": "response.output_text.delta", "delta": "x"})]);
        let error = outcome.expect_err("an unterminated stream refuses");
        assert_eq!(error.code, WireErrorCode::Protocol);
    }

    #[test]
    fn a_failed_response_refuses_with_the_provider_reason() {
        let (outcome, _) = drive(&[json!({
            "type": "response.failed",
            "response": {"status": "failed", "error": {"code": "server_error", "message": "boom"}},
        })]);
        let error = outcome.expect_err("a failed response refuses");
        assert_eq!(error.code, WireErrorCode::Refused);
        assert!(error.message.contains("boom"), "{}", error.message);
    }

    #[test]
    fn an_unknown_stream_event_warns_instead_of_vanishing() {
        let (outcome, sink) = drive(&[
            json!({"type": "response.something_new", "data": 1}),
            json!({"type": "response.completed", "response": {"status": "completed", "output": []}}),
        ]);
        assert!(outcome.is_ok());
        assert!(
            sink.events().iter().any(|event| matches!(
                event,
                StreamEvent::Warning { code, .. } if code == "unknown-stream-event"
            )),
            "{:?}",
            sink.events()
        );
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
    fn http_statuses_map_to_actionable_codes() {
        assert_eq!(
            status_error(reqwest::StatusCode::UNAUTHORIZED, "").code,
            WireErrorCode::Unauthorized
        );
        assert_eq!(
            status_error(reqwest::StatusCode::TOO_MANY_REQUESTS, "").code,
            WireErrorCode::RateLimited
        );
        let cold = status_error(reqwest::StatusCode::SERVICE_UNAVAILABLE, "");
        assert_eq!(cold.code, WireErrorCode::Transport);
        assert!(cold.retriable, "a cold gateway is worth another attempt");
        assert_eq!(
            status_error(reqwest::StatusCode::BAD_REQUEST, "").code,
            WireErrorCode::Refused
        );
    }
}
