#![forbid(unsafe_code)]

//! Serves the b10x loop over the pinned Codex app-server JSON-RPC format.
//!
//! Not a bridge — the opposite end of one. `runtime/agent` already knows how to drive a process
//! speaking this format, and the command it spawns is arbitrary. Speaking the format here means
//! that whole investment — conformance, governed execution, posture attestation, process reaping —
//! drives this harness with no new bridge code and no dependency in either direction.
//!
//! The loop underneath is the same one the embedded and command-line shells run. Only
//! [`session::BridgeTools`] differs: in-process a tool call is a function call, here it is a
//! round trip back to the client.

mod inventory;
mod session;
mod transport;

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use harness_loop::{AgentLoop, ApproveAll, Budget, LoopCancel, LoopConfig, LoopStop};
use harness_wire::ModelPort;
use serde_json::{Value, json};

pub use inventory::{
    CLIENT_METHODS, DYNAMIC_TOOL_ITEM, EXPERIMENTAL_API_CAPABILITY, MAX_DYNAMIC_TOOLS,
    MAX_FRAME_BYTES, MAX_TOOL_RESPONSE_BYTES, PRODUCT, PROFILE, REFUSED_CLIENT_METHODS,
    SERVER_METHODS, TERMINAL_STATUSES,
};
pub use transport::{INTERNAL_ERROR, INVALID_PARAMS, METHOD_NOT_FOUND, TransportError};

use session::{BridgeSink, BridgeTools, Wire, decode_dynamic_tools};
use transport::{Incoming, InterruptWatch, Reader, Writer};

/// Receiver-owned facts, supplied outside the protocol.
///
/// The endpoint and the credential deliberately do not travel over JSON-RPC. A client that could
/// name the model endpoint could redirect inference; one that could supply the credential would
/// make this process a relay for whatever key it was handed.
pub struct ServerConfig {
    /// Exact model identifier sent on every turn.
    pub model: String,
    /// Bounds applied to every turn on this connection.
    pub budget: Budget,
    /// Reported at `initialize`.
    pub version: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
    #[error("the client sent `{method}` before completing initialization")]
    NotInitialized { method: String },
    #[error("building a model client: {0}")]
    Model(String),
}

/// Builds the model client for one turn.
///
/// Per turn, not per connection: a turn that was cancelled leaves its token set, and reusing the
/// client would carry that cancellation into the next turn. The token is passed in so the reader
/// thread can stop a turn that is blocked on the model.
pub type ModelFactory<'a> = dyn FnMut(LoopCancel) -> Result<Box<dyn ModelPort>, String> + 'a;

/// Serves one connection until the client closes its input.
///
/// # Errors
///
/// Returns [`ServeError`] when the connection breaks or the client violates the handshake order.
/// A refused method is answered on the wire and is not an error here.
pub fn serve<R, W>(
    config: &ServerConfig,
    new_model: &mut ModelFactory<'_>,
    input: R,
    output: W,
) -> Result<(), ServeError>
where
    R: BufRead + Send + 'static,
    W: Write + Send + 'static,
{
    // The token belongs to whichever turn is running, so the reader can cancel the right one and
    // an interrupt that arrives between turns cancels nothing rather than the next turn.
    let active: Arc<Mutex<Option<TurnControl>>> = Arc::new(Mutex::new(None));
    let reader = Reader::spawn(input, Box::new(Watch(Arc::clone(&active))));
    let wire = Wire::new(Writer::new(Box::new(output)), reader);
    Server {
        config,
        wire,
        active,
        experimental: false,
        state: State::New,
        thread: None,
        next_id: 0,
    }
    .run(new_model)
}

/// Cancels the running turn the instant an interrupt frame is decoded.
///
/// Firing on the reading thread is what makes the cancel reach a turn blocked on the model. If no
/// turn is running the slot is empty and nothing is cancelled, which is correct: interrupting when
/// there is nothing to interrupt must not arm a trap for the next turn.
struct Watch(Arc<Mutex<Option<TurnControl>>>);

impl InterruptWatch for Watch {
    fn interrupted(&self) {
        if let Ok(active) = self.0.lock()
            && let Some(control) = active.as_ref()
        {
            // Recorded as well as cancelled. Both a person interrupting and a client vanishing end
            // the turn, and the terminal frame has to say which one happened.
            control.requested.store(true, Ordering::SeqCst);
            control.cancel.cancel();
        }
    }
}

/// What the reading thread may do to the turn that is currently running.
#[derive(Clone)]
struct TurnControl {
    cancel: LoopCancel,
    requested: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    New,
    Initialized,
    Threaded,
}

struct Thread {
    id: String,
    instructions: String,
    tools: Vec<harness_wire::ToolSpec>,
}

struct Server<'a> {
    config: &'a ServerConfig,
    wire: Wire,
    /// The running turn's controls, shared with the reading thread. `None` between turns.
    active: Arc<Mutex<Option<TurnControl>>>,
    /// Whether the client negotiated the capability its own tool-calling profile requires.
    experimental: bool,
    state: State,
    thread: Option<Thread>,
    next_id: u64,
}

impl Server<'_> {
    fn mint(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}-{}", self.next_id)
    }

    fn run(&mut self, new_model: &mut ModelFactory<'_>) -> Result<(), ServeError> {
        loop {
            let message = match self.wire.next_frame() {
                Ok(message) => message,
                // The client closing its input is how a connection ends, not a failure.
                Err(TransportError::Closed) => return Ok(()),
                Err(error) => return Err(error.into()),
            };
            match message {
                Incoming::Request { id, method, params } => {
                    self.request(&id, &method, &params, new_model)?;
                }
                Incoming::Notification { method, .. } => {
                    if method == "initialized" && self.state == State::New {
                        self.state = State::Initialized;
                    }
                }
                Incoming::Response { id, .. } => {
                    // Nothing is outstanding here; a stray answer means the two sides disagree
                    // about what was asked, which is worth saying rather than swallowing.
                    return Err(TransportError::Protocol(format!(
                        "the client answered request {id} with nothing outstanding"
                    ))
                    .into());
                }
                Incoming::Malformed { reason } => {
                    return Err(TransportError::Protocol(reason).into());
                }
            }
        }
    }

    fn request(
        &mut self,
        id: &Value,
        method: &str,
        params: &Value,
        new_model: &mut ModelFactory<'_>,
    ) -> Result<(), ServeError> {
        if REFUSED_CLIENT_METHODS.contains(&method) {
            // Named refusal beats a silent success: a client told a thread resumed, or a turn was
            // steered, would carry on believing something happened that did not.
            return Ok(self.wire.writer.borrow_mut().respond_error(
                id,
                METHOD_NOT_FOUND,
                format!("`{method}` is pinned but not implemented by {PRODUCT}"),
            )?);
        }
        match method {
            "initialize" => self.initialize(id, params),
            "thread/start" => self.start_thread(id, params),
            "turn/start" => self.start_turn(id, params, new_model),
            "turn/interrupt" => {
                // The reader already set the token; this only acknowledges it.
                Ok(self.wire.writer.borrow_mut().respond(id, &json!({}))?)
            }
            other => Ok(self.wire.writer.borrow_mut().respond_error(
                id,
                METHOD_NOT_FOUND,
                format!("`{other}` is outside this server's pinned inventory"),
            )?),
        }
    }

    fn initialize(&mut self, id: &Value, params: &Value) -> Result<(), ServeError> {
        self.experimental = params
            .get("capabilities")
            .and_then(|capabilities| capabilities.get(EXPERIMENTAL_API_CAPABILITY))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        self.wire.writer.borrow_mut().respond(
            id,
            &json!({
                "userAgent": {
                    "name": PRODUCT,
                    "version": self.config.version,
                    "profile": PROFILE,
                },
            }),
        )?;
        Ok(())
    }

    fn start_thread(&mut self, id: &Value, params: &Value) -> Result<(), ServeError> {
        if self.state == State::New {
            return Err(ServeError::NotInitialized {
                method: "thread/start".to_owned(),
            });
        }
        // A client that did not negotiate the capability cannot receive `item/tool/call` — its
        // own profile refuses the method. Accepting the registration and discovering that at the
        // first call would strand the turn; refusing here says so while the client can still act.
        if !self.experimental && params.get("dynamicTools").is_some() {
            return Ok(self.wire.writer.borrow_mut().respond_error(
                id,
                INVALID_PARAMS,
                format!(
                    "registering tools needs `capabilities.{EXPERIMENTAL_API_CAPABILITY}` at \
                     initialize; profile `{PROFILE}` requires it"
                ),
            )?);
        }
        let tools = match decode_dynamic_tools(params.get("dynamicTools")) {
            Ok(tools) => tools,
            Err((code, message)) => {
                return Ok(self
                    .wire
                    .writer
                    .borrow_mut()
                    .respond_error(id, code, message)?);
            }
        };
        let thread_id = self.mint("thr");
        self.thread = Some(Thread {
            id: thread_id.clone(),
            instructions: params
                .get("developerInstructions")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            tools,
        });
        self.state = State::Threaded;
        let mut writer = self.wire.writer.borrow_mut();
        writer.respond(id, &json!({"thread": {"id": thread_id}}))?;
        writer.notify("thread/started", &json!({"thread": {"id": thread_id}}))?;
        Ok(())
    }

    fn start_turn(
        &mut self,
        id: &Value,
        params: &Value,
        new_model: &mut ModelFactory<'_>,
    ) -> Result<(), ServeError> {
        let Some(thread) = &self.thread else {
            return Err(ServeError::NotInitialized {
                method: "turn/start".to_owned(),
            });
        };
        let thread_id = thread.id.clone();
        if params.get("threadId").and_then(Value::as_str) != Some(thread_id.as_str()) {
            return Ok(self.wire.writer.borrow_mut().respond_error(
                id,
                INVALID_PARAMS,
                "`threadId` must name this connection's thread",
            )?);
        }
        let Some(input) = turn_input(params) else {
            return Ok(self.wire.writer.borrow_mut().respond_error(
                id,
                INVALID_PARAMS,
                "`input` must carry at least one text entry",
            )?);
        };

        let turn_id = self.mint("turn");
        let message_item_id = self.mint("msg");
        {
            let mut writer = self.wire.writer.borrow_mut();
            writer.respond(id, &json!({"turn": {"id": turn_id}}))?;
            writer.notify(
                "turn/started",
                &json!({
                    "threadId": thread_id,
                    "turn": {"id": turn_id, "status": "inProgress", "items": []},
                }),
            )?;
        }

        // A fresh token per turn. Reusing one and clearing it would race the reading thread: an
        // interrupt decoded just before the clear would be erased, and the turn it was meant to
        // stop would run to completion while the client held an acknowledgement.
        let control = TurnControl {
            cancel: LoopCancel::new(),
            requested: Arc::new(AtomicBool::new(false)),
        };
        if let Ok(mut active) = self.active.lock() {
            *active = Some(control.clone());
        }
        let outcome = self.drive_turn(
            &thread_id,
            &turn_id,
            &message_item_id,
            &input,
            &control,
            new_model,
        );
        if let Ok(mut active) = self.active.lock() {
            *active = None;
        }

        let mut writer = self.wire.writer.borrow_mut();
        match outcome {
            Ok(TurnResult { text, status }) => {
                if status == "completed" {
                    writer.notify(
                        "item/completed",
                        &json!({
                            "threadId": thread_id,
                            "turnId": turn_id,
                            "item": {
                                "type": "agentMessage",
                                "id": message_item_id,
                                "text": text,
                            },
                        }),
                    )?;
                }
                writer.notify(
                    "turn/completed",
                    &json!({
                        "threadId": thread_id,
                        "turn": {"id": turn_id, "status": status, "items": []},
                    }),
                )?;
            }
            Err(message) => {
                writer.notify(
                    "turn/completed",
                    &json!({
                        "threadId": thread_id,
                        "turn": {
                            "id": turn_id,
                            "status": "failed",
                            "items": [],
                            "error": {"message": message},
                        },
                    }),
                )?;
            }
        }
        Ok(())
    }

    fn drive_turn(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        message_item_id: &str,
        input: &str,
        control: &TurnControl,
        new_model: &mut ModelFactory<'_>,
    ) -> Result<TurnResult, String> {
        let thread = self.thread.as_ref().expect("a turn requires a thread");
        let config = LoopConfig::new(self.config.model.as_str(), thread.instructions.as_str())
            .with_budget(self.config.budget.clone());
        let mut model = new_model(control.cancel.clone())?;
        let mut tools = BridgeTools::new(
            self.wire.clone(),
            thread_id,
            turn_id,
            thread.tools.clone(),
            control.cancel.clone(),
        );
        // Approval belongs to the client here: it registered these tools and mediates each
        // callback, so a second gate on this side would be a decision nobody asked for.
        let mut approvals = ApproveAll;
        let mut sink = BridgeSink::new(
            self.wire.clone(),
            thread_id,
            turn_id,
            message_item_id,
            control.cancel.clone(),
        );

        let outcome = AgentLoop::new(&mut *model, &mut tools, &mut approvals, config)
            .with_cancel(control.cancel.clone())
            .run(input, &mut sink)
            .map_err(|error| error.to_string())?;

        // An interrupt that was actually asked for is the reason this turn ended, even if the
        // connection then dropped. Letting a later write failure overwrite it would report a
        // person's own cancellation as a fault.
        if control.requested.load(Ordering::SeqCst) {
            return Ok(TurnResult {
                status: "interrupted".to_owned(),
                text: outcome.text,
            });
        }
        if let Some(error) = sink.broken.take() {
            return Err(error.to_string());
        }
        if let Some(error) = tools.broken.take() {
            return Err(error.to_string());
        }
        Ok(TurnResult {
            status: terminal_status(&outcome.stop).to_owned(),
            text: outcome.text,
        })
    }
}

struct TurnResult {
    text: String,
    status: String,
}

/// Maps a stop onto one of the three statuses the pinned client accepts.
///
/// A budget that binds is reported as `failed`, not `completed`: the model did not finish, and a
/// client told otherwise would treat a truncated run as an answer.
fn terminal_status(stop: &LoopStop) -> &'static str {
    match stop {
        LoopStop::Completed => "completed",
        LoopStop::Cancelled { .. } => "interrupted",
        LoopStop::MaxTurns { .. }
        | LoopStop::MaxInputTokens { .. }
        | LoopStop::MaxOutputTokens { .. }
        | LoopStop::MaxCost { .. }
        | LoopStop::Deadline { .. }
        | LoopStop::ProviderIncomplete { .. } => "failed",
    }
}

/// Reads the person's text out of a `turn/start` input list.
fn turn_input(params: &Value) -> Option<String> {
    let text: String = params
        .get("input")?
        .as_array()?
        .iter()
        .filter(|entry| entry.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|entry| entry.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_terminal_status_is_one_the_pinned_client_accepts() {
        for stop in [
            LoopStop::Completed,
            LoopStop::Cancelled {
                reason: String::new(),
            },
            LoopStop::MaxTurns { limit: 1 },
            LoopStop::MaxInputTokens {
                limit: 1,
                reported: 2,
            },
            LoopStop::MaxOutputTokens {
                limit: 1,
                reported: 2,
            },
            LoopStop::Deadline { limit_ms: 1 },
            LoopStop::ProviderIncomplete {
                reason: String::new(),
            },
        ] {
            let status = terminal_status(&stop);
            assert!(
                TERMINAL_STATUSES.contains(&status),
                "`{status}` is outside the pinned terminal set"
            );
        }
    }

    #[test]
    fn a_bound_run_is_reported_as_failed_rather_than_answered() {
        assert_eq!(terminal_status(&LoopStop::MaxTurns { limit: 2 }), "failed");
        assert_eq!(
            terminal_status(&LoopStop::Cancelled {
                reason: String::new()
            }),
            "interrupted"
        );
        assert_eq!(terminal_status(&LoopStop::Completed), "completed");
    }

    #[test]
    fn turn_input_joins_text_entries_and_refuses_an_empty_one() {
        assert_eq!(
            turn_input(&json!({"input": [{"type": "text", "text": "hello"}]})),
            Some("hello".to_owned())
        );
        assert_eq!(
            turn_input(&json!({"input": [
                {"type": "text", "text": "one"},
                {"type": "image", "url": "ignored"},
                {"type": "text", "text": "two"},
            ]})),
            Some("one\ntwo".to_owned())
        );
        assert_eq!(turn_input(&json!({"input": []})), None);
        assert_eq!(turn_input(&json!({})), None);
    }
}
