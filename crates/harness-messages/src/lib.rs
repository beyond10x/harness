#![forbid(unsafe_code)]

//! Anthropic Messages wire adapter for the b10x agent loop.
//!
//! It speaks one documented endpoint, `POST {base}/messages`, in streaming mode, and projects it
//! into [`harness_wire`]'s neutral values. It holds no credential: the bearer is read from an
//! injected source at call time and dropped when the call ends.
//!
//! # What the second wire cost, and where
//!
//! The projection is in [`project`] and is entirely this wire's own: role-alternating messages
//! instead of a flat input array, content blocks instead of output items, an argument object
//! instead of encoded argument text, disjoint token counts instead of nested ones, and a required
//! output bound where the first wire had an optional one.
//!
//! What is **not** this wire's own, and is a near-copy of `harness-responses`, is everything
//! between the HTTP client and the projection: bounded server-sent-event framing, the retry rule,
//! the back-off, the witnessed sink that makes the retry rule safe, and the status-code mapping.
//! None of that is vendor-shaped — it is *transport*-shaped — and the second wire proved it by
//! needing all of it unchanged. It was copied rather than extracted because extracting it means a
//! third crate and a change to the wire that is already released, and this change is the one that
//! produces the evidence rather than the one that should act on it. That is the finding; a
//! `harness-http` beneath both wires is what it argues for.

mod project;
mod sse;

use std::collections::BTreeMap;
use std::io::{BufReader, Read};
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_wire::{
    Bearer, BearerSource, Cancel, CredentialKind, Item, MAX_TOOL_ARGUMENT_BYTES, ModelPort,
    StreamEvent, StreamSink, TurnOutcome, TurnRequest, WireError, WireErrorCode, WireId,
};
use serde_json::{Value, json};

pub use project::{
    MAX_TEMPERATURE, MAX_TOOL_NAME_BYTES, TOOL_NAME_PATTERN, request_body, usage_from_message,
};
pub use sse::{MAX_EVENT_BYTES, MAX_STREAM_BYTES, SseReader};

/// Identifies this projection. Opaque items carry it and may not be replayed into another wire.
pub const WIRE: &str = "anthropic-messages";

/// The API version this adapter is pinned to. Sent on every request.
///
/// The route requires it and treats it as the contract: an absent version is a 400, and a
/// different version is a different set of bytes. It is a constant rather than a setting because a
/// version the caller could change is a contract this repository would no longer be pinning.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Names the API version header.
pub const VERSION_HEADER: &str = "anthropic-version";

/// Carries a key issued to a program. **Not** `authorization`, which is the other route.
pub const API_KEY_HEADER: &str = "x-api-key";

/// Carries a token obtained on a person's behalf, as `Bearer <token>`.
pub const OAUTH_HEADER: &str = "authorization";

/// Names the beta-feature header.
pub const BETA_HEADER: &str = "anthropic-beta";

/// The beta this route requires before it will accept a subscription token.
///
/// Without it `POST {base}/messages` rejects an otherwise valid `authorization: Bearer` — the
/// requirement is per route, so a token that works elsewhere fails here and the failure names
/// authentication rather than the missing header.
pub const OAUTH_BETA: &str = "oauth-2025-04-20";

/// The output bound sent when the caller named none.
///
/// # The one place absence cannot be preserved
///
/// This route **requires** `max_tokens`, so there is no "leave it out and let the provider
/// decide": leaving it out is a 400. The neutral [`TurnRequest::max_output_tokens`] is an option
/// and stays one, and this number is what an absent one resolves to. It is a property of the
/// [`Endpoint`], so a caller who cares names it rather than discovering it here.
pub const DEFAULT_MAX_OUTPUT_TOKENS: u64 = 8192;

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Applies to each individual read, not to the turn.
///
/// A streamed turn may legitimately run for minutes; what must not happen is a peer that accepts
/// the connection and then says nothing, which would hold the loop open with no way out.
const DEFAULT_OPERATION_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_ERROR_BODY_BYTES: usize = 2048;

/// How many extra attempts a turn gets when the far side failed in a way that may not repeat.
const MAX_ATTEMPTS: u32 = 4;

/// How long to wait before attempt `n`, doubling and capped.
fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64 << attempt.min(4))
}

/// Sleeps for `duration` unless the caller cancels first.
fn pause(duration: Duration, cancel: &Cancel) {
    let end = Instant::now() + duration;
    let slice = Duration::from_millis(50);
    loop {
        if cancel.is_cancelled() {
            return;
        }
        let now = Instant::now();
        if now >= end {
            return;
        }
        std::thread::sleep(slice.min(end - now));
    }
}

/// A sink that remembers whether anything reached the caller.
///
/// **The whole of the retry rule.** Resending a request is safe on this wire — nothing is retained
/// on the far side and a second identical POST is a fresh turn. What is *not* safe is resending
/// after the caller has already seen part of the first attempt: the text deltas are out, a person
/// has read them, and a second attempt would append a second copy of the same sentence to the
/// record. So an attempt that has emitted **anything** is final, whatever went wrong.
struct WitnessedSink<'a> {
    inner: &'a mut dyn StreamSink,
    emitted: bool,
}

impl StreamSink for WitnessedSink<'_> {
    fn emit(&mut self, event: StreamEvent) {
        self.emitted = true;
        self.inner.emit(event);
    }
}

/// One endpoint serving one model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// Origin plus API prefix, for example `https://api.example/v1`.
    pub base_url: String,
    /// The exact model identifier the endpoint serves.
    pub model: String,
    /// The context window the endpoint serves for that model.
    pub context_window: u64,
    /// What an unnamed per-turn output bound resolves to. See [`DEFAULT_MAX_OUTPUT_TOKENS`].
    pub max_output_tokens: u64,
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
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
        })
    }

    /// Names what an unnamed per-turn output bound resolves to on this endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`WireErrorCode::Protocol`] for zero, which is a request the route refuses.
    pub fn with_max_output_tokens(mut self, limit: u64) -> Result<Self, WireError> {
        if limit == 0 {
            return Err(WireError::protocol(
                "an output bound of zero admits no answer",
            ));
        }
        self.max_output_tokens = limit;
        Ok(self)
    }

    fn messages_url(&self) -> String {
        format!("{}/messages", self.base_url)
    }
}

/// Every header one request carries, in the order they are set.
///
/// **Exposed because the contract pins it.** The credential's presentation is not a detail of this
/// wire — it is the difference between a key issued to a program and a token obtained on a
/// person's behalf, and the two travel under different header names on the same endpoint. A test
/// that read a second list would prove only that the second list was right.
///
/// The returned values include the credential. It is built, written into a request, and dropped;
/// this is a place a secret can escape and every call site is expected to treat it as one.
fn request_headers(credential: Option<(&Bearer, CredentialKind)>) -> Vec<(&'static str, String)> {
    let mut headers = vec![
        ("accept", "text/event-stream".to_owned()),
        ("content-type", "application/json".to_owned()),
        (VERSION_HEADER, ANTHROPIC_VERSION.to_owned()),
    ];
    match credential {
        None => {}
        Some((bearer, CredentialKind::ApiKey)) => {
            headers.push((API_KEY_HEADER, bearer.expose().to_owned()));
        }
        Some((bearer, CredentialKind::Oauth)) => {
            headers.push((OAUTH_HEADER, format!("Bearer {}", bearer.expose())));
            headers.push((BETA_HEADER, OAUTH_BETA.to_owned()));
        }
    }
    headers
}

/// The header names one request carries for a given credential presentation.
///
/// Derived from [`request_headers`] with a placeholder, so the contract pins the names the code
/// actually sends rather than a list beside it. No credential is involved.
pub fn header_names(credential: Option<CredentialKind>) -> Vec<&'static str> {
    let placeholder = Bearer::new("placeholder");
    request_headers(credential.map(|kind| (&placeholder, kind)))
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

pub struct MessagesClient {
    wire: WireId,
    endpoint: Endpoint,
    /// [`None`] sends no credential header at all.
    ///
    /// Not the same as an empty credential, which is refused below: an empty bearer means a source
    /// answered with nothing and the run would fail in a way nobody could explain. `None` means
    /// the caller named no source, which is the right shape for a gateway on this machine that
    /// authenticates nobody — and for a run declared with no credential, whose first request is
    /// expected to be refused by the far end rather than by this client.
    bearer: Option<Arc<dyn BearerSource>>,
    http: reqwest::blocking::Client,
    cancel: Cancel,
}

impl std::fmt::Debug for MessagesClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MessagesClient")
            .field("wire", &self.wire)
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl MessagesClient {
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

    /// A client that sends no credential header.
    ///
    /// # Errors
    ///
    /// Returns [`WireErrorCode::Transport`] when the HTTP client cannot be constructed.
    pub fn unauthenticated(endpoint: Endpoint) -> Result<Self, WireError> {
        Self::build(endpoint, None)
    }

    fn build(endpoint: Endpoint, bearer: Option<Arc<dyn BearerSource>>) -> Result<Self, WireError> {
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
    pub fn cancel_handle(&self) -> Cancel {
        self.cancel.clone()
    }

    /// Adopts a token shared with the rest of a turn, rather than owning a private one.
    #[must_use]
    pub fn with_cancel(mut self, cancel: Cancel) -> Self {
        self.cancel = cancel;
        self
    }

    /// One turn, retried while the far side has not actually answered.
    fn attempt_turn(
        &self,
        body: &Value,
        model: &str,
        sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        let mut attempt = 0;
        loop {
            let mut witnessed = WitnessedSink {
                inner: sink,
                emitted: false,
            };
            let outcome = self.send(body).and_then(|response| {
                let reader =
                    SseReader::new(BufReader::new(response)).with_cancel(self.cancel.clone());
                drain(reader, &self.wire, model, &mut witnessed)
            });
            let error = match outcome {
                Ok(outcome) => return Ok(outcome),
                Err(error) => error,
            };
            attempt += 1;
            let again = error.retriable
                && attempt < MAX_ATTEMPTS
                && !witnessed.emitted
                && !self.cancel.is_cancelled();
            if !again {
                return Err(error);
            }
            // Said out loud. A run that quietly took four times as long as it looks would be a run
            // whose latency numbers mean nothing.
            sink.emit(StreamEvent::Warning {
                code: "turn-retried".to_owned(),
                message: format!(
                    "attempt {attempt} of {MAX_ATTEMPTS} failed before answering and is being \
                     retried: {}",
                    error.message
                ),
            });
            pause(backoff(attempt), &self.cancel);
        }
    }

    fn send(&self, body: &Value) -> Result<reqwest::blocking::Response, WireError> {
        if self.cancel.is_cancelled() {
            return Err(WireError::cancelled());
        }
        // Held only for as long as it takes to become a header, and dropped before the send that
        // can block.
        let fetched = match &self.bearer {
            None => None,
            Some(source) => {
                let bearer = source.bearer()?;
                if bearer.is_empty() {
                    return Err(WireError::unauthorized(
                        "the bearer source returned an empty credential",
                    ));
                }
                Some((bearer, source.kind()))
            }
        };
        let mut request = self.http.post(self.endpoint.messages_url());
        for (name, value) in request_headers(fetched.as_ref().map(|(bearer, kind)| (bearer, *kind)))
        {
            request = request.header(name, value);
        }
        drop(fetched);
        let response = request.json(body).send().map_err(|error| {
            WireError::transport(format!(
                "posting to {}: {error}",
                self.endpoint.messages_url()
            ))
        })?;
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
        // 529 is this route's *overloaded* status and lands here with the rest of the 5xx range: a
        // gateway that is still starting a backend and a route that is momentarily full are both
        // worth another attempt.
        500..=599 => (WireErrorCode::Transport, true),
        _ => (WireErrorCode::Refused, false),
    };
    WireError::new(code, format!("{WIRE} answered {status}: {body}"), retriable)
}

/// One content block being assembled from its deltas.
struct Block {
    value: Value,
    /// Accumulated `input_json_delta` fragments for a tool call, parsed once at the end.
    ///
    /// A half-parsed argument blob must never reach a tool: the tool would act on a value the
    /// model did not send.
    arguments: String,
}

/// Accumulates one streamed message into a turn outcome.
struct TurnDecoder<'a> {
    wire: &'a WireId,
    model: &'a str,
    /// Content-block index -> the block being assembled.
    blocks: BTreeMap<u64, Block>,
    /// Content-block index -> call id, so an arguments delta can name the call a reader is
    /// watching.
    calls: BTreeMap<u64, harness_wire::CallId>,
    items: Vec<Item>,
    /// The usage object as `message_start` reported it, before the output count is final.
    usage: Option<Value>,
    reported_model: Option<String>,
    stop: Option<String>,
    /// Set by `message_stop`. This wire has no `[DONE]`: the terminal marker is a payload.
    complete: bool,
}

impl<'a> TurnDecoder<'a> {
    fn new(wire: &'a WireId, model: &'a str) -> Self {
        Self {
            wire,
            model,
            blocks: BTreeMap::new(),
            calls: BTreeMap::new(),
            items: Vec::new(),
            usage: None,
            reported_model: None,
            stop: None,
            complete: false,
        }
    }

    fn apply(&mut self, event: &Value, sink: &mut dyn StreamSink) -> Result<(), WireError> {
        match event.get("type").and_then(Value::as_str) {
            Some("message_start") => self.start_message(event),
            Some("content_block_start") => self.start_block(event),
            Some("content_block_delta") => self.apply_delta(event, sink)?,
            Some("content_block_stop") => self.stop_block(event, sink)?,
            Some("message_delta") => self.apply_message_delta(event),
            Some("message_stop") => self.complete = true,
            Some("error") => return Err(project::stream_error(event)),
            // A keep-alive. The turn is not advanced by it and nothing is lost by ignoring it.
            Some("ping") => {}
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

    fn start_message(&mut self, event: &Value) {
        let Some(message) = event.get("message") else {
            return;
        };
        self.usage = message.get("usage").cloned();
        self.reported_model = message
            .get("model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
    }

    fn start_block(&mut self, event: &Value) {
        let (Some(index), Some(block)) = (index_of(event), event.get("content_block")) else {
            return;
        };
        if block.get("type").and_then(Value::as_str) == Some("tool_use")
            && let Some(id) = block.get("id").and_then(Value::as_str)
            && let Ok(call_id) = harness_wire::CallId::new(id)
        {
            self.calls.insert(index, call_id);
        }
        self.blocks.insert(
            index,
            Block {
                value: block.clone(),
                arguments: String::new(),
            },
        );
    }

    fn apply_delta(&mut self, event: &Value, sink: &mut dyn StreamSink) -> Result<(), WireError> {
        let (Some(index), Some(delta)) = (index_of(event), event.get("delta")) else {
            return Ok(());
        };
        let call_id = self.calls.get(&index).cloned();
        let Some(block) = self.blocks.get_mut(&index) else {
            // A delta for a block that never started. Preserved as a warning rather than dropped:
            // it means the stream is not the shape this subset pins.
            sink.emit(StreamEvent::Warning {
                code: "unknown-stream-event".to_owned(),
                message: format!("a delta arrived for content block {index}, which never started"),
            });
            return Ok(());
        };
        match delta.get("type").and_then(Value::as_str) {
            Some("text_delta") => {
                let text = delta
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                append(&mut block.value, "text", text);
                sink.emit(StreamEvent::TextDelta {
                    text: text.to_owned(),
                });
            }
            Some("input_json_delta") => {
                let fragment = delta
                    .get("partial_json")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                block.arguments.push_str(fragment);
                if block.arguments.len() > MAX_TOOL_ARGUMENT_BYTES {
                    return Err(WireError::too_large(format!(
                        "streamed arguments of content block {index} are over the \
                         {MAX_TOOL_ARGUMENT_BYTES} byte bound"
                    )));
                }
                if let Some(call_id) = call_id {
                    sink.emit(StreamEvent::ToolArgumentsDelta {
                        call_id,
                        delta: fragment.to_owned(),
                    });
                }
            }
            // Reasoning text and its signature. Accumulated into the block and **never** emitted
            // to the sink: the block is opaque, and a reader shown its contents would be reading
            // something this crate has undertaken not to interpret.
            Some("thinking_delta") => {
                let text = delta
                    .get("thinking")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                append(&mut block.value, "thinking", text);
            }
            Some("signature_delta") => {
                let signature = delta
                    .get("signature")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                append(&mut block.value, "signature", signature);
            }
            other => {
                let kind = other.unwrap_or("<absent>").to_owned();
                sink.emit(StreamEvent::Warning {
                    code: "unknown-stream-event".to_owned(),
                    message: format!(
                        "content block delta `{kind}` is outside the pinned subset and was skipped"
                    ),
                });
            }
        }
        Ok(())
    }

    fn stop_block(&mut self, event: &Value, sink: &mut dyn StreamSink) -> Result<(), WireError> {
        let Some(index) = index_of(event) else {
            return Ok(());
        };
        let Some(mut block) = self.blocks.remove(&index) else {
            return Ok(());
        };
        if !block.arguments.is_empty() {
            let arguments: Value = serde_json::from_str(&block.arguments).map_err(|error| {
                WireError::protocol(format!(
                    "streamed arguments of content block {index} are not JSON: {error}"
                ))
            })?;
            if let Some(object) = block.value.as_object_mut() {
                object.insert("input".to_owned(), arguments);
            }
        }
        let mut warnings = Vec::new();
        let decoded = project::block_to_item(self.wire, &block.value, &mut |code, message| {
            warnings.push((code, message));
        });
        for (code, message) in warnings {
            sink.emit(StreamEvent::Warning { code, message });
        }
        self.items.push(decoded?);
        Ok(())
    }

    fn apply_message_delta(&mut self, event: &Value) {
        if let Some(reason) = event
            .get("delta")
            .and_then(|delta| delta.get("stop_reason"))
            .and_then(Value::as_str)
        {
            self.stop = Some(reason.to_owned());
        }
        // The final output count. `message_start` reports the count so far, which is not it.
        if let Some(output) = event
            .get("usage")
            .and_then(|usage| usage.get("output_tokens"))
            .and_then(Value::as_u64)
            && let Some(usage) = self.usage.as_mut().and_then(Value::as_object_mut)
        {
            usage.insert("output_tokens".to_owned(), json!(output));
        }
    }

    fn finish(self) -> Result<TurnOutcome, WireError> {
        if !self.complete {
            return Err(WireError::protocol(
                "the stream ended before the message reached a terminal state",
            ));
        }
        let has_tool_calls = self.items.iter().any(|item| item.as_tool_call().is_some());
        // Rebuilt into the shape a non-streamed message has, so one function reads usage on both
        // paths rather than two that can disagree about what a cached turn cost.
        let mut reported = serde_json::Map::new();
        if let Some(model) = self.reported_model {
            reported.insert("model".to_owned(), json!(model));
        }
        if let Some(usage) = self.usage {
            reported.insert("usage".to_owned(), usage);
        }
        Ok(TurnOutcome {
            stop_reason: project::stop_reason(self.stop.as_deref(), has_tool_calls),
            usage: project::usage_from_message(&Value::Object(reported), self.model),
            items: self.items,
        })
    }
}

fn index_of(event: &Value) -> Option<u64> {
    event.get("index").and_then(Value::as_u64)
}

/// Appends `text` to a string field, creating it when the block did not carry one.
fn append(block: &mut Value, field: &str, text: &str) {
    let Some(object) = block.as_object_mut() else {
        return;
    };
    let existing = object
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default();
    let joined = format!("{existing}{text}");
    object.insert(field.to_owned(), json!(joined));
}

impl ModelPort for MessagesClient {
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
        // Beside the other pre-flight checks, and for the same reason: a request this wire cannot
        // carry is refused here, naming what is wrong, rather than posted and explained by the far
        // side in terms of its own field names.
        project::check_tool_names(&request.tools)?;
        project::check_sampling(&request.sampling)?;
        project::check_conversation(&request.items)?;
        let body = project::request_body(
            &request.model,
            &request.instructions,
            &request.items,
            &request.tools,
            request
                .max_output_tokens
                .unwrap_or(self.endpoint.max_output_tokens),
            &request.sampling,
        );
        self.attempt_turn(&body, &request.model, sink)
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
/// stream that ends before a terminal message.
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
        decoder.apply(&event, sink)?;
    }
    decoder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_wire::{StaticBearer, StopReason, Usage, VecSink};

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
            None => decoder.finish(),
        };
        (outcome, sink)
    }

    fn message_start(usage: &Value) -> Value {
        json!({"type": "message_start", "message": {
            "id": "msg_1", "type": "message", "role": "assistant",
            "model": "test-model", "content": [], "usage": usage.clone(),
        }})
    }

    #[test]
    fn endpoints_refuse_a_relative_base_or_an_empty_model() {
        assert!(Endpoint::new("https://api.example/v1", "m", 8192).is_ok());
        assert!(Endpoint::new("api.example/v1", "m", 8192).is_err());
        assert!(Endpoint::new("https://api.example/v1", "", 8192).is_err());
        assert!(Endpoint::new("https://api.example/v1", "m", 0).is_err());
    }

    #[test]
    fn a_trailing_slash_does_not_double_in_the_url() {
        let endpoint = Endpoint::new("https://api.example/v1/", "m", 8192).expect("valid");
        assert_eq!(endpoint.messages_url(), "https://api.example/v1/messages");
    }

    #[test]
    fn an_output_bound_of_zero_is_refused_rather_than_sent() {
        let endpoint = Endpoint::new("https://api.example/v1", "m", 8192).expect("valid");
        assert_eq!(
            endpoint
                .clone()
                .with_max_output_tokens(0)
                .expect_err("refused")
                .code,
            WireErrorCode::Protocol
        );
        assert_eq!(
            endpoint
                .with_max_output_tokens(2048)
                .expect("a real bound")
                .max_output_tokens,
            2048
        );
    }

    #[test]
    fn a_key_and_a_subscription_token_travel_under_different_header_names() {
        // The whole reason `CredentialKind` reached the neutral layer. Sending a subscription
        // token as `x-api-key` is a 401 that names authentication and not the header, and sending
        // an API key as `authorization` is the same failure the other way round.
        let bearer = Bearer::new("synthetic-secret");
        let key = request_headers(Some((&bearer, CredentialKind::ApiKey)));
        assert!(key.contains(&(API_KEY_HEADER, "synthetic-secret".to_owned())));
        assert!(key.iter().all(|(name, _)| *name != OAUTH_HEADER));
        assert!(key.iter().all(|(name, _)| *name != BETA_HEADER));

        let oauth = request_headers(Some((&bearer, CredentialKind::Oauth)));
        assert!(oauth.contains(&(OAUTH_HEADER, "Bearer synthetic-secret".to_owned())));
        // The route rejects a bearer token without it, and the failure names authentication.
        assert!(oauth.contains(&(BETA_HEADER, OAUTH_BETA.to_owned())));
        assert!(oauth.iter().all(|(name, _)| *name != API_KEY_HEADER));

        // The version is on every request, credential or not, and no credential header is sent
        // when the caller named no source.
        for headers in [request_headers(None), key.clone(), oauth.clone()] {
            assert!(headers.contains(&(VERSION_HEADER, ANTHROPIC_VERSION.to_owned())));
        }
        assert_eq!(
            header_names(None),
            vec!["accept", "content-type", VERSION_HEADER]
        );
    }

    #[test]
    fn a_secret_never_lands_in_a_header_name() {
        // `header_names` is what the contract pins, so it must be provably free of the value.
        for kind in [
            None,
            Some(CredentialKind::ApiKey),
            Some(CredentialKind::Oauth),
        ] {
            assert!(
                header_names(kind)
                    .iter()
                    .all(|name| !name.contains("placeholder")),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn streamed_text_reaches_the_sink_and_the_outcome() {
        let (outcome, sink) = drive(&[
            message_start(&json!({"input_tokens": 3, "output_tokens": 1})),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Hel"}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "lo"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 2}}),
            json!({"type": "message_stop"}),
        ]);
        let outcome = outcome.expect("the turn completes");
        assert_eq!(sink.text(), "Hello");
        assert_eq!(outcome.items, vec![Item::assistant("Hello")]);
        assert_eq!(outcome.stop_reason, StopReason::EndTurn);
        assert_eq!(
            outcome.usage,
            Some(Usage {
                model: "test-model".to_owned(),
                input_tokens: 3,
                output_tokens: 2,
                cached_input_tokens: 0,
                cache_creation_input_tokens: None,
            })
        );
    }

    #[test]
    fn a_tool_call_correlates_its_argument_deltas_and_is_parsed_once() {
        let (outcome, sink) = drive(&[
            message_start(&json!({"input_tokens": 3, "output_tokens": 1})),
            json!({"type": "content_block_start", "index": 0, "content_block": {
                "type": "tool_use", "id": "toolu_1", "name": "workspace_read", "input": {},
            }}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{\"path\":"}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "\"README.md\"}"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 9}}),
            json!({"type": "message_stop"}),
        ]);
        let outcome = outcome.expect("the turn completes");
        assert_eq!(outcome.stop_reason, StopReason::ToolCalls);
        let call = outcome.tool_calls().next().expect("one call");
        assert_eq!(call.call_id.as_str(), "toolu_1");
        assert_eq!(call.arguments, json!({"path": "README.md"}));
        assert!(
            sink.events().iter().any(|event| matches!(
                event,
                StreamEvent::ToolArgumentsDelta { call_id, .. } if call_id.as_str() == "toolu_1"
            )),
            "{:?}",
            sink.events()
        );
    }

    #[test]
    fn arguments_that_never_became_json_never_reach_a_tool() {
        let (outcome, _) = drive(&[
            message_start(&json!({"input_tokens": 1, "output_tokens": 1})),
            json!({"type": "content_block_start", "index": 0, "content_block": {
                "type": "tool_use", "id": "toolu_1", "name": "t", "input": {},
            }}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": "{not json"}}),
            json!({"type": "content_block_stop", "index": 0}),
        ]);
        let error = outcome.expect_err("half-parsed arguments refuse");
        assert_eq!(error.code, WireErrorCode::Protocol);
    }

    #[test]
    fn streamed_arguments_are_bounded_and_the_bound_refuses_by_name() {
        // The bound exists because the far side decides how many fragments to send, and each one
        // is appended to a buffer this process owns. It is checked *while* accumulating rather
        // than at the end — a check after the loop is a check the peer has already outgrown.
        let mut events = vec![
            message_start(&json!({"input_tokens": 1, "output_tokens": 1})),
            json!({"type": "content_block_start", "index": 0, "content_block": {
                "type": "tool_use", "id": "toolu_1", "name": "t", "input": {},
            }}),
        ];
        // Sixteen fragments of 8 kB: individually unremarkable, and over the bound together.
        let fragment = "x".repeat(MAX_TOOL_ARGUMENT_BYTES / 16);
        for _ in 0..=16 {
            events.push(json!({"type": "content_block_delta", "index": 0, "delta": {
                "type": "input_json_delta", "partial_json": fragment,
            }}));
        }
        let (outcome, _) = drive(&events);
        let error = outcome.expect_err("oversized streamed arguments refuse");
        assert_eq!(error.code, WireErrorCode::TooLarge);
        // Named, so a reader knows which bound bit and where.
        assert!(
            error.message.contains("content block 0"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains(&MAX_TOOL_ARGUMENT_BYTES.to_string()),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_thinking_block_is_assembled_and_kept_opaque_without_being_streamed_to_a_reader() {
        let (outcome, sink) = drive(&[
            message_start(&json!({"input_tokens": 1, "output_tokens": 1})),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "step "}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "one"}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "SIG"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 4}}),
            json!({"type": "message_stop"}),
        ]);
        let outcome = outcome.expect("the turn completes");
        assert_eq!(
            outcome.items,
            vec![Item::Opaque {
                wire: wire(),
                payload: json!({"type": "thinking", "thinking": "step one", "signature": "SIG"}),
            }]
        );
        // Opaque means opaque: the reasoning text is carried, not shown.
        assert_eq!(sink.text(), "");
    }

    #[test]
    fn a_stream_without_a_terminal_event_refuses() {
        let (outcome, _) = drive(&[message_start(
            &json!({"input_tokens": 1, "output_tokens": 1}),
        )]);
        let error = outcome.expect_err("an unterminated stream refuses");
        assert_eq!(error.code, WireErrorCode::Protocol);
        assert!(error.message.contains("terminal"), "{}", error.message);
    }

    #[test]
    fn a_stream_error_refuses_with_the_provider_reason() {
        let (outcome, _) = drive(&[json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": "boom"},
        })]);
        let error = outcome.expect_err("a stream error refuses");
        assert_eq!(error.code, WireErrorCode::Refused);
        assert!(error.message.contains("boom"), "{}", error.message);
    }

    #[test]
    fn an_unknown_stream_event_warns_instead_of_vanishing() {
        let (outcome, sink) = drive(&[
            message_start(&json!({"input_tokens": 1, "output_tokens": 1})),
            json!({"type": "message_something_new", "data": 1}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
            json!({"type": "message_stop"}),
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
    fn a_keep_alive_is_not_reported_as_an_unknown_event() {
        let (outcome, sink) = drive(&[
            message_start(&json!({"input_tokens": 1, "output_tokens": 1})),
            json!({"type": "ping"}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
            json!({"type": "message_stop"}),
        ]);
        assert!(outcome.is_ok());
        assert!(sink.events().is_empty(), "{:?}", sink.events());
    }

    #[test]
    fn an_endpoint_that_reports_no_usage_leaves_usage_unknown() {
        let (outcome, _) = drive(&[
            json!({"type": "message_start", "message": {"id": "msg_1", "model": "test-model"}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}}),
            json!({"type": "message_stop"}),
        ]);
        assert_eq!(
            outcome.expect("the turn completes").usage,
            None,
            "absent is not zero"
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
        let overloaded = status_error(
            reqwest::StatusCode::from_u16(529).expect("a valid status"),
            "",
        );
        assert_eq!(overloaded.code, WireErrorCode::Transport);
        assert!(overloaded.retriable, "an overloaded route answers later");
        assert_eq!(
            status_error(reqwest::StatusCode::BAD_REQUEST, "").code,
            WireErrorCode::Refused
        );
    }

    /// A client pointed at an address nothing answers on.
    ///
    /// Every case below refuses **before** a byte goes out, which is the property under test: if a
    /// pre-flight check stops being called, the request escapes and the failure is a transport
    /// error instead of the named refusal.
    fn unreachable_client() -> MessagesClient {
        let endpoint = Endpoint::new("http://127.0.0.1:1/v1", "m", 8192).expect("valid");
        MessagesClient::new(endpoint, Arc::new(StaticBearer::new("synthetic-secret")))
            .expect("the client builds")
    }

    fn request_of(items: Vec<Item>, tools: Vec<harness_wire::ToolSpec>) -> TurnRequest {
        TurnRequest {
            model: "m".to_owned(),
            instructions: String::new(),
            items,
            tools,
            max_output_tokens: None,
            sampling: harness_wire::Sampling::default(),
        }
    }

    #[test]
    fn every_pre_flight_check_is_actually_reached_before_a_request_goes_out() {
        // The checks are unit-tested one layer down; this is the wiring. A `turn` that stopped
        // calling one of them would still pass every one of those tests, and would fail at the far
        // side with a vendor error naming its own field names instead of the caller's item.
        let spec = |name: &str| harness_wire::ToolSpec {
            name: harness_wire::ToolName::new(name).expect("a printable identifier"),
            description: "d".to_owned(),
            input_schema: json!({"type": "object"}),
            approval: harness_wire::Approval::NotRequired,
            envelope: harness_wire::Envelope::default(),
        };
        let cases: Vec<(&str, TurnRequest, &str)> = vec![
            (
                "a conversation that opens with the model",
                request_of(vec![Item::assistant("hi")], Vec::new()),
                "item 0",
            ),
            (
                "a tool name this wire cannot publish",
                request_of(vec![Item::user("hi")], vec![spec("workspace.read")]),
                "workspace.read",
            ),
            (
                "a temperature outside this wire's range",
                TurnRequest {
                    sampling: harness_wire::Sampling {
                        temperature: Some(1.5),
                        ..harness_wire::Sampling::default()
                    },
                    ..request_of(vec![Item::user("hi")], Vec::new())
                },
                "temperature",
            ),
        ];
        for (name, request, expected) in cases {
            let mut client = unreachable_client();
            let mut sink = VecSink::new();
            let Err(error) = client.turn(&request, &mut sink) else {
                panic!("{name} must refuse before anything is sent");
            };
            assert_eq!(
                error.code,
                WireErrorCode::Protocol,
                "{name} reached the network instead of being refused here: {error}"
            );
            assert!(error.message.contains(expected), "{name}: {error}");
        }
    }

    #[test]
    fn a_foreign_opaque_item_is_refused_by_name_rather_than_replayed() {
        // AGENTS.md invariant 5, from this side: a reasoning blob the Responses wire produced is
        // meaningless here, and a wire that silently dropped it would poison the conversation.
        let endpoint = Endpoint::new("https://api.example/v1", "m", 8192).expect("valid");
        let mut client =
            MessagesClient::new(endpoint, Arc::new(StaticBearer::new("synthetic-secret")))
                .expect("the client builds");
        let request = TurnRequest {
            model: "m".to_owned(),
            instructions: String::new(),
            items: vec![
                Item::user("hi"),
                Item::Opaque {
                    wire: WireId::new("openai-responses").expect("valid"),
                    payload: json!({"type": "reasoning"}),
                },
            ],
            tools: Vec::new(),
            max_output_tokens: None,
            sampling: harness_wire::Sampling::default(),
        };
        let mut sink = VecSink::new();
        let error = client
            .turn(&request, &mut sink)
            .expect_err("a foreign opaque item refuses");
        assert_eq!(error.code, WireErrorCode::Unsupported);
        assert!(error.message.contains("openai-responses"), "{error}");
        assert!(error.message.contains(WIRE), "{error}");
    }
}
