#![forbid(unsafe_code)]

//! Serves the b10x loop over the pinned Codex app-server JSON-RPC format.
//!
//! Not a bridge — the opposite end of one. `runtime/agent` already knows how to drive a process
//! speaking this format, and the command it spawns is arbitrary. Speaking the format here means
//! that whole investment — conformance, governed execution, posture attestation, process reaping —
//! drives this harness with no new bridge code and no dependency in either direction.
//!
//! The loop underneath is the same one the embedded and command-line shells run. Only
//! `session::BridgeTools` differs: in-process a tool call is a function call, here it is a
//! round trip back to the client.

mod inventory;
mod session;
mod transport;

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    /// How many tokens the model's context window holds, when the operator knows.
    ///
    /// Reaches `LoopConfig::context_window`, so a bridged thread compacts on the provider's own
    /// reported count rather than on the fixed byte rule. `None` keeps the byte rule.
    pub context_window: Option<u64>,
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
    let interrupts: Arc<Mutex<Interrupts>> = Arc::new(Mutex::new(Interrupts::default()));
    let reader = Reader::spawn(input, Box::new(Watch(Arc::clone(&interrupts))));
    let wire = Wire::new(Writer::new(Box::new(output)), reader);
    Server {
        config,
        wire,
        interrupts,
        experimental: false,
        state: State::New,
        thread: None,
        next_id: 0,
    }
    .run(new_model)
}

/// Cancels the running turn the instant an interrupt frame is decoded.
///
/// Firing on the reading thread is what makes the cancel reach a turn blocked on the model. What
/// it cannot do is reach a turn that does not exist yet, and it may not answer the frame either —
/// only the main thread writes, or the order of frames on the wire stops being the order of
/// events. So when there is no turn to cancel it leaves a count behind rather than dropping the
/// fact that a frame arrived at all.
struct Watch(Arc<Mutex<Interrupts>>);

impl InterruptWatch for Watch {
    fn interrupted(&self) {
        if let Ok(mut interrupts) = self.0.lock() {
            match interrupts.active.as_ref() {
                Some(control) => control.decoded(),
                None => interrupts.stray += 1,
            }
        }
    }
}

/// This connection's interrupt state, read and written under one lock.
///
/// One lock rather than two independent cells: whether an interrupt cancels the running turn or is
/// left for the next one to answer has to be decided against a single instant, or an interrupt
/// decoded exactly as a turn is installed is counted as both and acted on as neither.
#[derive(Default)]
struct Interrupts {
    /// The running turn's controls. `None` between turns.
    active: Option<TurnControl>,
    /// `turn/interrupt` frames decoded with no turn to cancel, still queued for the main thread.
    ///
    /// Which turn — if any — a count belongs to is settled by the order frames leave the queue,
    /// and that is the order the client sent them. An interrupt sent *before* `turn/start` is
    /// dequeued and answered by the main loop before `turn/start` is, taking the count back down,
    /// so it cannot arm a trap for the next turn. One still standing when `turn/start` is handled
    /// was sent after it, and belongs to the turn it asks for.
    stray: usize,
}

/// What the reading thread may do to the turn that is currently running.
#[derive(Clone)]
pub(crate) struct TurnControl {
    pub(crate) cancel: LoopCancel,
    requested: Arc<AtomicBool>,
    /// Interrupt frames this turn was cancelled by that the client has not been answered yet.
    owed: Arc<AtomicUsize>,
}

impl TurnControl {
    /// A fresh token per turn. Reusing one and clearing it would race the reading thread: an
    /// interrupt decoded just before the clear would be erased, and the turn it was meant to stop
    /// would run to completion while the client held an acknowledgement.
    fn new() -> Self {
        Self {
            cancel: LoopCancel::new(),
            requested: Arc::new(AtomicBool::new(false)),
            owed: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// `frames` interrupts were decoded for this turn and are on their way to the main thread.
    ///
    /// Recorded as well as cancelled, and counted as well as recorded. The record says *why* the
    /// turn ended, because both a person interrupting and a client vanishing end one and the
    /// terminal frame has to say which. The count says the client is still owed an answer to a
    /// frame it sent — and until it has one, this turn's terminal frame must not go out.
    fn decoded_n(&self, frames: usize) {
        if frames == 0 {
            return;
        }
        self.requested.store(true, Ordering::SeqCst);
        self.owed.fetch_add(frames, Ordering::SeqCst);
        self.cancel.cancel();
    }

    /// One was decoded by the reading thread while this turn was running.
    fn decoded(&self) {
        self.decoded_n(1);
    }

    /// The main thread answered one of them.
    ///
    /// It cancels too, and that is not decoration. An interrupt decoded before this turn was
    /// installed found no turn to cancel, so the only place it can be *acted on* is the place it
    /// is answered. Acknowledging without acting is the whole defect: a client told its interrupt
    /// succeeded, and handed the answer it cancelled.
    pub(crate) fn answered(&self) {
        self.requested.store(true, Ordering::SeqCst);
        // Saturating: an interrupt the reading thread never counted — one it could not attribute
        // to any turn — must still be answerable without taking the count below zero.
        let _ = self
            .owed
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |owed| {
                Some(owed.saturating_sub(1))
            });
        self.cancel.cancel();
    }

    /// Whether a frame this turn was cancelled by is still unanswered.
    pub(crate) fn owes(&self) -> bool {
        self.owed.load(Ordering::SeqCst) > 0
    }
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
    /// The running turn's controls and the interrupts no turn could take, shared with the reader.
    interrupts: Arc<Mutex<Interrupts>>,
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
                // Reached from the main loop, which is between turns — a running turn is inside
                // `drive_turn` — so there is no turn here to cancel and the reading thread left a
                // count rather than a cancellation. Taking it back down is what keeps it off the
                // next turn: the client sent this frame before it asked for that turn, and the
                // queue's order is what says so.
                //
                // The count and the frame are not tied to one another, and cannot be: a frame is
                // counted on the reading thread and dequeued on this one. A frame counted into a
                // *turn's* `owed` could be dequeued here and take `stray` down for a count that
                // belongs to a different frame — but only if a turn ended owing one, and there
                // are exactly two ways left to do that. `drive_turn` settles on every path out of
                // itself, so one is a `settle_interrupts` that failed: the connection is gone, or
                // the frame never arrived. The other is an announcement that failed to write, in
                // `start_turn` above, which returns an error straight out of `Server::run` — so
                // this arm is never reached again on that connection.
                //
                // Even then it is the harmless direction. Only [`Watch`] ever raises `stray`, and
                // only with no turn to cancel, so it cannot be counted too high; a decrement that
                // pairs with the wrong frame can only leave it too low, and too low means an
                // interrupt cancels its turn at the drain rather than before it streams. Too
                // high would mean cancelling a turn nobody asked to stop.
                if let Ok(mut interrupts) = self.interrupts.lock() {
                    interrupts.stray = interrupts.stray.saturating_sub(1);
                }
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

    /// Makes `control` the turn the reading thread cancels, and hands it what it already missed.
    ///
    /// Installed before the client is told the turn exists, not after. The gap between the two was
    /// a window in which the reading thread found no turn to cancel, and installing early cannot
    /// cancel the wrong one: this connection runs a single turn at a time and `turn/interrupt`
    /// names no turn, so from here on there is exactly one turn an interrupt could mean. What the
    /// client has not been told yet is the id — which it never needed in order to send the frame,
    /// and which no code path reads.
    ///
    /// The count taken over is interrupts decoded before this turn could be installed. They are
    /// this turn's, by the queue's order: one sent before `turn/start` would have left the queue
    /// before it and taken the count back down. A client that pipelines the two frames reaches
    /// this every time rather than by luck, and it used to be acknowledged and dropped.
    ///
    /// Both under one lock, so the reading thread cannot see a half-installed turn.
    fn install(&self, control: &TurnControl) {
        let carried = match self.interrupts.lock() {
            Ok(mut interrupts) => {
                let carried = std::mem::take(&mut interrupts.stray);
                interrupts.active = Some(control.clone());
                carried
            }
            Err(_) => 0,
        };
        control.decoded_n(carried);
    }

    /// Stops any further interrupt attaching to the turn that just ended.
    fn clear_active(&self) {
        if let Ok(mut interrupts) = self.interrupts.lock() {
            interrupts.active = None;
        }
    }

    /// Tells the client the turn exists: its id, then `turn/started`.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError`] when the client's pipe is gone.
    fn announce(&self, id: &Value, thread_id: &str, turn_id: &str) -> Result<(), TransportError> {
        let mut writer = self.wire.writer.borrow_mut();
        writer.respond(id, &json!({"turn": {"id": turn_id}}))?;
        writer.notify(
            "turn/started",
            &json!({
                "threadId": thread_id,
                "turn": {"id": turn_id, "status": "inProgress", "items": []},
            }),
        )
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
        let control = TurnControl::new();
        self.install(&control);

        // Bound rather than `?`-ed, like the two in `drive_turn` and for the second of the same
        // two reasons. The turn is already installed by the time these are written, so a `?` here
        // would return with `interrupts.active` still holding this turn's control — for the life
        // of the connection, because the clear below is skipped — and with whatever the turn owed
        // never answered.
        //
        // Unlike `drive_turn`'s, these two cannot be *settled*: the only way to answer an
        // interrupt is to write, and the write is what just failed. So the slot is cleared and the
        // error raised. In practice the connection is already gone and `Server::run` is about to
        // end it, which is why this was never visible; the point is that the invariant does not
        // rest on that being true.
        if let Err(error) = self.announce(id, &thread_id, &turn_id) {
            self.clear_active();
            return Err(error.into());
        }

        let outcome = self.drive_turn(
            &thread_id,
            &turn_id,
            &message_item_id,
            &input,
            &control,
            new_model,
        );
        self.clear_active();

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
            .with_budget(self.config.budget.clone())
            .with_context_window(self.config.context_window);
        let model = new_model(control.cancel.clone());
        let mut tools = BridgeTools::new(
            self.wire.clone(),
            thread_id,
            turn_id,
            thread.tools.clone(),
            control.clone(),
        );
        // Approval belongs to the client here: it registered these tools and mediates each
        // callback, so a second gate on this side would be a decision nobody asked for.
        let mut approvals = ApproveAll;
        let mut sink = BridgeSink::new(
            self.wire.clone(),
            thread_id,
            turn_id,
            message_item_id,
            control.clone(),
        );

        // Bound rather than `?`-ed on the spot, and so is the model client above it. A turn that
        // could not proceed at all — an unusable budget, a client that would not build, a wire
        // that failed on the first request — was still cancelled by whatever cancelled it, and
        // still owes the client an answer to the frame that did the cancelling. Returning here
        // would write `turn/completed` with an interrupt unanswered and the status `failed`, for
        // a turn a person stopped: the exact symptom this change exists to remove, surviving on
        // the paths that jumped over the fix. Both errors are carried past the two steps below
        // and raised last, so every way out of this function settles what the turn owes.
        let ended = match model {
            Ok(mut model) => AgentLoop::new(&mut *model, &mut tools, &mut approvals, config)
                .with_cancel(control.cancel.clone())
                .run(input, &mut sink)
                .map_err(|error| error.to_string()),
            Err(error) => Err(error),
        };

        // Every interrupt this turn was cancelled by is a frame the client is still waiting on an
        // answer to, and it has to have one *before* the frame that says the turn ended. An
        // acknowledgement read after `turn/completed` is not an acknowledgement, it is a receipt.
        // `BridgeSink` drains between streamed events, which covers a turn that streamed anything;
        // a turn cancelled before its first event never reaches that drain, one cancelled before
        // it began never starts a stream to drain between, and one that failed outright reaches
        // neither.
        if let Err(error) = self.wire.settle_interrupts(control) {
            sink.broken.get_or_insert(error);
        }

        // An interrupt that was actually asked for is the reason this turn ended, even if the
        // connection then dropped or the run could not proceed at all. Letting a later write
        // failure — or a budget this run could never have satisfied — overwrite it would report a
        // person's own cancellation as a fault.
        if control.requested.load(Ordering::SeqCst) {
            return Ok(TurnResult {
                status: "interrupted".to_owned(),
                // Whatever the run had when it stopped, and nothing when it never started. An
                // interrupted turn's text is not delivered either way: `start_turn` writes
                // `item/completed` only for a turn that completed.
                text: ended.map(|outcome| outcome.text).unwrap_or_default(),
            });
        }
        let outcome = ended?;
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
        | LoopStop::BudgetUnobservable { .. }
        | LoopStop::Deadline { .. }
        | LoopStop::AwaitingApproval { .. }
        | LoopStop::ProviderIncomplete { .. }
        | LoopStop::Unstructured { .. } => "failed",
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
