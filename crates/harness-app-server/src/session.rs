//! The wire-backed halves of one turn: tools that call back to the client, and a sink that
//! projects the loop's events into pinned notifications.
//!
//! `BridgeTools` is the second implementation of [`ToolPort`], and the reason the bridge costs no
//! second loop. In-process a tool call is a function call; here it is a JSON-RPC round trip. The
//! loop cannot tell, so nothing above this file changes.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use harness_loop::{LoopCancel, LoopEvent, LoopSink};
use harness_wire::{ToolCall, ToolOutcome, ToolPort, ToolSpec};
use serde_json::{Value, json};

use crate::inventory::{DYNAMIC_TOOL_ITEM, MAX_TOOL_RESPONSE_BYTES};
use crate::transport::{
    INVALID_PARAMS, Incoming, METHOD_NOT_FOUND, Reader, TransportError, Writer,
};

/// How long this server waits for the client to answer one `item/tool/call`.
///
/// Generous, because a client may be asking a person. Bounded, because a client that never answers
/// would otherwise hold a turn open forever with nothing to show for it.
const TOOL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(600);

/// Shared access to the one connection.
#[derive(Clone)]
pub struct Wire {
    pub writer: Rc<RefCell<Writer>>,
    pub reader: Rc<Reader>,
    /// Frames pulled off the wire early, held for whoever was actually waiting for them.
    stash: Rc<RefCell<VecDeque<Incoming>>>,
}

impl Wire {
    pub fn new(writer: Writer, reader: Reader) -> Self {
        Self {
            writer: Rc::new(RefCell::new(writer)),
            reader: Rc::new(reader),
            stash: Rc::new(RefCell::new(VecDeque::new())),
        }
    }

    /// Answers control frames that arrived while a turn was in flight.
    ///
    /// Called between streamed events, which is the only moment a running turn is at the wire at
    /// all. Without it an interrupt is acknowledged only once the turn it was meant to stop has
    /// already finished — an acknowledgement that arrives after the fact is not one.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] when the connection is gone.
    pub fn drain_control(&self) -> Result<(), TransportError> {
        while let Some(message) = self.reader.try_next()? {
            match message {
                Incoming::Request { id, method, .. } if method == "turn/interrupt" => {
                    self.writer.borrow_mut().respond(&id, &json!({}))?;
                }
                Incoming::Request { id, method, .. } => {
                    self.writer.borrow_mut().respond_error(
                        &id,
                        METHOD_NOT_FOUND,
                        format!("`{method}` cannot be served while a turn is running"),
                    )?;
                }
                // Not ours to answer. Hand it back rather than swallowing it, or whoever is
                // waiting for it waits forever.
                other => self.stash.borrow_mut().push_back(other),
            }
        }
        Ok(())
    }

    /// Returns the next frame, preferring one already pulled off the wire.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::Closed`] once the client's input ends.
    pub fn next_frame(&self) -> Result<Incoming, TransportError> {
        if let Some(message) = self.stash.borrow_mut().pop_front() {
            return Ok(message);
        }
        self.reader.next()
    }

    /// Returns the next frame within `timeout`, preferring one already pulled off the wire.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::TimedOut`] or [`TransportError::Closed`].
    pub fn next_frame_within(&self, timeout: Duration) -> Result<Incoming, TransportError> {
        if let Some(message) = self.stash.borrow_mut().pop_front() {
            return Ok(message);
        }
        self.reader.next_within(timeout)
    }
}

/// Wall-clock milliseconds, as the pinned notifications' `*AtMs` fields mean.
///
/// A real measurement or nothing: a counter dressed as a timestamp would read downstream as when
/// something happened.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Projects loop events into the pinned notification set.
pub struct BridgeSink {
    wire: Wire,
    thread_id: String,
    turn_id: String,
    message_item_id: String,
    cancel: LoopCancel,
    /// Set once writing fails, so the turn ends instead of looping against a closed pipe.
    pub broken: Option<TransportError>,
}

impl BridgeSink {
    pub fn new(
        wire: Wire,
        thread_id: &str,
        turn_id: &str,
        message_item_id: &str,
        cancel: LoopCancel,
    ) -> Self {
        Self {
            wire,
            thread_id: thread_id.to_owned(),
            turn_id: turn_id.to_owned(),
            message_item_id: message_item_id.to_owned(),
            cancel,
            broken: None,
        }
    }

    fn notify(&mut self, method: &str, params: &Value) {
        if self.broken.is_some() {
            return;
        }
        // The write guard must be released before draining: `drain_control` borrows the same
        // writer to answer control frames, and holding both borrows across one statement aborts
        // the process the moment a client sends anything while a turn is running.
        let written = self.wire.writer.borrow_mut().notify(method, params);
        if let Err(error) = written.and_then(|()| self.wire.drain_control()) {
            self.fail(error);
        }
    }

    /// Ends the turn when the connection is gone.
    ///
    /// Cancelling matters as much as recording: without it the loop keeps calling the model for a
    /// reader that will never see the answer, spending inference on nobody.
    fn fail(&mut self, error: TransportError) {
        self.cancel.cancel();
        self.broken = Some(error);
    }
}

impl LoopSink for BridgeSink {
    fn emit(&mut self, event: LoopEvent) {
        match event {
            LoopEvent::TextDelta { text } => self.notify("item/agentMessage/delta", &json!({
                    "threadId": self.thread_id,
                    "turnId": self.turn_id,
                    "itemId": self.message_item_id,
                    "delta": text,
                }),
            ),
            LoopEvent::Usage(usage) => {
                let counts = json!({
                    "inputTokens": usage.input_tokens,
                    "cachedInputTokens": usage.cached_input_tokens,
                    "outputTokens": usage.output_tokens,
                    "reasoningOutputTokens": 0,
                    "totalTokens": usage.input_tokens.saturating_add(usage.output_tokens),
                });
                self.notify("thread/tokenUsage/updated", &json!({
                        "threadId": self.thread_id,
                        "turnId": self.turn_id,
                        "tokenUsage": {"total": counts, "last": counts},
                    }),
                );
            }
            // A warning has no home in the pinned notification set that this server may safely
            // fill, so it goes to stderr rather than being invented onto the wire or dropped.
            LoopEvent::Warning { code, message } => {
                eprintln!("warning [{code}] {message}");
            }
            // The tool round trip is owned by `BridgeTools`, which has to interleave a request and
            // its response; announcing it twice would desynchronise the client's call bookkeeping.
            LoopEvent::ToolRequested(_)
            | LoopEvent::ToolCompleted { .. }
            | LoopEvent::ToolArgumentsDelta { .. }
            // Bridged tools carry no approval posture, so these never fire here.
            | LoopEvent::ApprovalRequired { .. }
            | LoopEvent::ApprovalResolved { .. }
            // The turn's own frames are written by the server, which holds the outcome.
            | LoopEvent::Started { .. }
            | LoopEvent::TurnStarted { .. }
            | LoopEvent::Finished { .. }
            // The pinned notification set has no field for a price, and this server may not invent
            // one. A bridged run is priced by whoever started it, from the token counts above.
            | LoopEvent::Rates { .. }
            | LoopEvent::Cost { .. } => {}
        }
    }
}

/// Tools the client registered, called back over the wire.
pub struct BridgeTools {
    wire: Wire,
    thread_id: String,
    turn_id: String,
    specs: Vec<ToolSpec>,
    cancel: LoopCancel,
    /// Set once the connection fails, so the loop stops rather than retrying into a closed pipe.
    pub broken: Option<TransportError>,
}

impl BridgeTools {
    pub fn new(
        wire: Wire,
        thread_id: &str,
        turn_id: &str,
        specs: Vec<ToolSpec>,
        cancel: LoopCancel,
    ) -> Self {
        Self {
            wire,
            thread_id: thread_id.to_owned(),
            turn_id: turn_id.to_owned(),
            specs,
            cancel,
            broken: None,
        }
    }

    fn item(call: &ToolCall, status: &str) -> Value {
        json!({
            "type": DYNAMIC_TOOL_ITEM,
            "id": call.call_id.as_str(),
            "tool": call.name.as_str(),
            "status": status,
        })
    }

    /// Announces the call, asks the client to run it, and waits for exactly its answer.
    fn round_trip(&mut self, call: &ToolCall) -> Result<ToolOutcome, TransportError> {
        // The client registers the call from `item/started`, and refuses a callback for a call it
        // has not seen. This ordering is load-bearing, not decoration.
        self.wire.writer.borrow_mut().notify(
            "item/started",
            &json!({
                "threadId": self.thread_id,
                "turnId": self.turn_id,
                "startedAtMs": now_ms(),
                "item": Self::item(call, "inProgress"),
            }),
        )?;

        let request_id = self.wire.writer.borrow_mut().take_request_id();
        self.wire.writer.borrow_mut().request(
            request_id,
            "item/tool/call",
            &json!({
                "arguments": call.arguments,
                "callId": call.call_id.as_str(),
                "threadId": self.thread_id,
                "tool": call.name.as_str(),
                "turnId": self.turn_id,
            }),
        )?;

        let outcome = self.await_response(request_id)?;
        self.wire.writer.borrow_mut().notify(
            "item/completed",
            &json!({
                "threadId": self.thread_id,
                "turnId": self.turn_id,
                "completedAtMs": now_ms(),
                "item": Self::item(call, if outcome.failed { "failed" } else { "completed" }),
            }),
        )?;
        Ok(outcome)
    }

    /// Waits for the answer to `request_id`, staying responsive to an interrupt meanwhile.
    ///
    /// An interrupt arriving here is answered and recorded, but the wait continues: the client owes
    /// this call an answer, and abandoning it would leave both sides waiting on each other.
    fn await_response(&mut self, request_id: i64) -> Result<ToolOutcome, TransportError> {
        let deadline = Instant::now() + TOOL_RESPONSE_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(TransportError::TimedOut(TOOL_RESPONSE_TIMEOUT));
            }
            match self.wire.next_frame_within(remaining)? {
                Incoming::Response { id, result, error } if id == json!(request_id) => {
                    if let Some(error) = error {
                        return Ok(ToolOutcome::failed(format!(
                            "the client refused this call: {error}"
                        )));
                    }
                    return Ok(decode_tool_result(result.as_ref()));
                }
                Incoming::Request { id, method, .. } if method == "turn/interrupt" => {
                    self.cancel.cancel();
                    self.wire.writer.borrow_mut().respond(&id, &json!({}))?;
                }
                Incoming::Request { id, method, .. } => {
                    self.wire.writer.borrow_mut().respond_error(
                        &id,
                        METHOD_NOT_FOUND,
                        format!("`{method}` cannot be served while a tool call is outstanding"),
                    )?;
                }
                Incoming::Response { id, .. } => {
                    return Err(TransportError::Protocol(format!(
                        "the client answered request {id} while {request_id} was outstanding"
                    )));
                }
                Incoming::Malformed { reason } => return Err(TransportError::Protocol(reason)),
                Incoming::Notification { .. } => {}
            }
        }
    }
}

/// Reads the client's answer, preserving a refusal as a refusal.
fn decode_tool_result(result: Option<&Value>) -> ToolOutcome {
    let Some(result) = result else {
        return ToolOutcome::failed("the client answered without a result");
    };
    let success = result
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text: String = result
        .get("contentItems")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    let text = if text.len() > MAX_TOOL_RESPONSE_BYTES {
        format!("the client's answer passed the {MAX_TOOL_RESPONSE_BYTES} byte bound")
    } else {
        text
    };
    ToolOutcome {
        output: Value::String(text),
        failed: !success,
    }
}

impl ToolPort for BridgeTools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        if let Some(error) = &self.broken {
            return ToolOutcome::failed(format!("the connection is gone: {error}"));
        }
        match self.round_trip(call) {
            Ok(outcome) => outcome,
            Err(error) => {
                let message = format!("the tool call could not be delivered: {error}");
                self.broken = Some(error);
                // Cancel so the loop stops rather than issuing more calls into a dead connection.
                self.cancel.cancel();
                ToolOutcome::failed(message)
            }
        }
    }
}

/// Decodes the tools a client registered on `thread/start`.
///
/// # Errors
///
/// Returns a JSON-RPC code and message when the registration is unusable, so the client learns the
/// tool it registered will never be called rather than discovering it silently absent.
pub fn decode_dynamic_tools(value: Option<&Value>) -> Result<Vec<ToolSpec>, (i64, String)> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let entries = value
        .as_array()
        .ok_or((INVALID_PARAMS, "`dynamicTools` must be an array".to_owned()))?;
    if entries.len() > crate::inventory::MAX_DYNAMIC_TOOLS {
        return Err((
            INVALID_PARAMS,
            format!(
                "`dynamicTools` holds {} entries, over the {} bound",
                entries.len(),
                crate::inventory::MAX_DYNAMIC_TOOLS
            ),
        ));
    }
    entries.iter().map(decode_one_tool).collect()
}

fn decode_one_tool(entry: &Value) -> Result<ToolSpec, (i64, String)> {
    let name = entry
        .get("name")
        .and_then(Value::as_str)
        .ok_or((INVALID_PARAMS, "a registered tool has no `name`".to_owned()))?;
    let name = harness_wire::ToolName::new(name)
        .map_err(|error| (INVALID_PARAMS, format!("tool name `{name}`: {error}")))?;
    Ok(ToolSpec {
        description: entry
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        input_schema: entry
            .get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({"type": "object"})),
        name,
        // A registration carries no approval posture, so nothing here can claim a person gates it.
        // Approval in bridge mode is the client's business, on its side of the callback.
        approval: harness_wire::Approval::NotRequired,
        // Nor does it carry an envelope. A client's dynamic tool is described by the client, and
        // inventing effects for one would be this process asserting something about code it has
        // never seen. The default is the safest description available, and the honest position is
        // that in bridge mode the gate is on the other side of the wire — where the tool actually
        // runs.
        envelope: harness_wire::Envelope::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_registration_becomes_a_publishable_tool() {
        let tools = decode_dynamic_tools(Some(&json!([{
            "type": "function",
            "name": "b10x_operation_search",
            "description": "search",
            "deferLoading": false,
            "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}}},
        }])))
        .expect("a well formed registration decodes");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name.as_str(), "b10x_operation_search");
        assert_eq!(tools[0].approval, harness_wire::Approval::NotRequired);
        assert_eq!(tools[0].input_schema["type"], json!("object"));
    }

    #[test]
    fn no_registration_means_no_tools_rather_than_an_error() {
        assert!(
            decode_dynamic_tools(None)
                .expect("absent is fine")
                .is_empty()
        );
    }

    #[test]
    fn an_unusable_registration_refuses_so_the_client_learns_of_it() {
        assert_eq!(
            decode_dynamic_tools(Some(&json!("not an array")))
                .expect_err("a non-array refuses")
                .0,
            INVALID_PARAMS
        );
        assert_eq!(
            decode_dynamic_tools(Some(&json!([{"description": "no name"}])))
                .expect_err("a nameless tool refuses")
                .0,
            INVALID_PARAMS
        );
        assert_eq!(
            decode_dynamic_tools(Some(&json!([{"name": "has space"}])))
                .expect_err("an unaddressable name refuses")
                .0,
            INVALID_PARAMS
        );
    }

    #[test]
    fn too_many_registered_tools_refuse() {
        let many: Vec<Value> = (0..=crate::inventory::MAX_DYNAMIC_TOOLS)
            .map(|index| json!({"name": format!("t{index}")}))
            .collect();
        assert_eq!(
            decode_dynamic_tools(Some(&Value::Array(many)))
                .expect_err("over the bound refuses")
                .0,
            INVALID_PARAMS
        );
    }

    #[test]
    fn a_client_refusal_stays_a_failure() {
        let outcome = decode_tool_result(Some(&json!({
            "success": false,
            "contentItems": [{"type": "inputText", "text": "not granted"}],
        })));
        assert!(outcome.failed);
        assert_eq!(outcome.output, json!("not granted"));
    }

    #[test]
    fn a_missing_result_is_a_failure_rather_than_an_empty_success() {
        assert!(decode_tool_result(None).failed);
        // A reply that simply omits `success` is not a success: assuming otherwise would tell the
        // model an effect happened on the strength of a missing field.
        let unstated = decode_tool_result(Some(&json!({"contentItems": []})));
        assert!(unstated.failed);
        assert_eq!(unstated.output, json!(""));
    }

    #[test]
    fn content_items_are_concatenated() {
        let outcome = decode_tool_result(Some(&json!({
            "success": true,
            "contentItems": [
                {"type": "inputText", "text": "one "},
                {"type": "inputText", "text": "two"},
            ],
        })));
        assert!(!outcome.failed);
        assert_eq!(outcome.output, json!("one two"));
    }

    #[test]
    fn an_oversized_answer_is_replaced_rather_than_forwarded() {
        let outcome = decode_tool_result(Some(&json!({
            "success": true,
            "contentItems": [{"type": "inputText", "text": "x".repeat(MAX_TOOL_RESPONSE_BYTES + 1)}],
        })));
        let text = outcome.output.as_str().expect("text");
        assert!(text.contains("bound"), "{text}");
        assert!(text.len() < MAX_TOOL_RESPONSE_BYTES);
    }
}
