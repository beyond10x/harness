//! Adversary pass 1 against story:bridge-turn-start-after-interrupt-on-every-path.
//!
//! The unit holds a request a turn meets after one of its interrupts in a `deferred` queue for the
//! main loop, and documents that every reader serving requests in order takes that queue before
//! the channel, so the client's order is the order frames are served in. These cases drive the
//! readers the unit's own tests do not reach with more than one held request: the settle that
//! answers interrupts owed by the turn a held `turn/start` started, the order of two held requests,
//! and a non-request frame queued behind a held one.
//!
//! Driven in process, as `turn_start_after_interrupt.rs` drives the server: every case is decided
//! by frame order and by the decode count the reading thread reports, never by a sleep.

use std::io::{BufRead, Read, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

use b10x_harness_app_server::{self as harness_app_server, ServerConfig};
use harness_loop::Budget;
use harness_wire::{
    CallId, Cancel, Item, ModelPort, StopReason, StreamEvent, StreamSink, ToolCall, ToolName,
    TurnOutcome, TurnRequest, WireError, WireId,
};
use serde_json::{Value, json};

const READ_TIMEOUT: Duration = Duration::from_secs(20);

const STORY: &str = "story:bridge-turn-start-after-interrupt-on-every-path";

struct ScriptedInput {
    chunks: Receiver<Vec<u8>>,
    current: Vec<u8>,
    position: usize,
    taken: usize,
    used: Sender<usize>,
}

impl Read for ScriptedInput {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let available = self.fill_buf()?;
        let length = available.len().min(out.len());
        out[..length].copy_from_slice(&available[..length]);
        self.consume(length);
        Ok(length)
    }
}

impl BufRead for ScriptedInput {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        while self.position == self.current.len() {
            let _ = self.used.send(self.taken);
            match self.chunks.recv() {
                Ok(chunk) => {
                    self.current = chunk;
                    self.position = 0;
                    self.taken += 1;
                }
                Err(_) => return Ok(&[]),
            }
        }
        Ok(&self.current[self.position..])
    }

    fn consume(&mut self, amount: usize) {
        self.position += amount;
    }
}

type HoldOn = fn(&Value) -> bool;

/// The server's stdout, split into frames, able to hold the main thread after the first frame
/// `hold` matches. While it is held inside a write the main thread dequeues nothing.
struct HeldOutput {
    pending: Vec<u8>,
    frames: Sender<Value>,
    hold: Option<(HoldOn, Receiver<()>)>,
}

impl Write for HeldOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            let frame: Value = serde_json::from_slice(&line).map_err(std::io::Error::other)?;
            let held = self
                .hold
                .as_ref()
                .is_some_and(|(matches, _)| matches(&frame));
            let _ = self.frames.send(frame);
            if held && let Some((_, release)) = self.hold.take() {
                let _ = release.recv_timeout(READ_TIMEOUT);
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn wire_id() -> WireId {
    WireId::new("openai-responses").expect("a valid wire id")
}

struct Streams(WireId);

impl ModelPort for Streams {
    fn wire(&self) -> &WireId {
        &self.0
    }

    fn turn(
        &mut self,
        _request: &TurnRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        sink.emit(StreamEvent::TextDelta {
            text: "partial".to_owned(),
        });
        Ok(TurnOutcome {
            stop_reason: StopReason::EndTurn,
            items: vec![Item::assistant("partial")],
            usage: None,
        })
    }
}

struct CallsProbe {
    wire: WireId,
    called: bool,
}

impl ModelPort for CallsProbe {
    fn wire(&self) -> &WireId {
        &self.wire
    }

    fn turn(
        &mut self,
        _request: &TurnRequest,
        _sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        if self.called {
            return Ok(TurnOutcome {
                stop_reason: StopReason::EndTurn,
                items: vec![Item::assistant("done")],
                usage: None,
            });
        }
        self.called = true;
        Ok(TurnOutcome {
            stop_reason: StopReason::ToolCalls,
            items: vec![Item::ToolCall(ToolCall {
                call_id: CallId::new("call-1").expect("a valid call id"),
                name: ToolName::new("probe").expect("a valid tool name"),
                arguments: json!({}),
            })],
            usage: None,
        })
    }
}

type Factory = Box<dyn FnMut(Cancel) -> Result<Box<dyn ModelPort>, String> + Send>;

/// The first turn's model calls `probe`; every later turn's streams.
fn probe_then_stream() -> Factory {
    let mut built = 0_u32;
    Box::new(
        move |_cancel: Cancel| -> Result<Box<dyn ModelPort>, String> {
            built += 1;
            if built == 1 {
                Ok(Box::new(CallsProbe {
                    wire: wire_id(),
                    called: false,
                }))
            } else {
                Ok(Box::new(Streams(wire_id())))
            }
        },
    )
}

fn streams_always() -> Factory {
    Box::new(|_cancel: Cancel| -> Result<Box<dyn ModelPort>, String> {
        Ok(Box::new(Streams(wire_id())))
    })
}

struct InProcess {
    chunks: Sender<Vec<u8>>,
    sent: usize,
    used: Receiver<usize>,
    frames: Receiver<Value>,
    seen: Vec<Value>,
    ended: Receiver<Result<(), String>>,
}

impl InProcess {
    fn serve(hold: Option<(HoldOn, Receiver<()>)>, factory: Factory) -> Self {
        let (chunk_sender, chunks) = channel::<Vec<u8>>();
        let (used_sender, used) = channel::<usize>();
        let (frame_sender, frames) = channel::<Value>();
        let (ended_sender, ended) = channel::<Result<(), String>>();
        let input = ScriptedInput {
            chunks,
            current: Vec::new(),
            position: 0,
            taken: 0,
            used: used_sender,
        };
        let output = HeldOutput {
            pending: Vec::new(),
            frames: frame_sender,
            hold,
        };
        let config = ServerConfig {
            model: "b10x-emulated".to_owned(),
            budget: Budget::default(),
            context_window: None,
            version: "test".to_owned(),
        };
        thread::spawn(move || {
            let mut factory = factory;
            let result = harness_app_server::serve(&config, &mut *factory, input, output)
                .map_err(|error| error.to_string());
            let _ = ended_sender.send(result);
        });
        Self {
            chunks: chunk_sender,
            sent: 0,
            used,
            frames,
            seen: Vec::new(),
            ended,
        }
    }

    fn send_lines(&mut self, lines: &[String]) -> usize {
        let mut chunk = String::new();
        for line in lines {
            chunk.push_str(line);
            chunk.push('\n');
        }
        self.chunks
            .send(chunk.into_bytes())
            .expect("the server's reading thread is alive");
        self.sent += 1;
        self.sent
    }

    fn send(&mut self, frames: &[Value]) -> usize {
        let lines: Vec<String> = frames.iter().map(Value::to_string).collect();
        self.send_lines(&lines)
    }

    fn decoded(&self, number: usize, what: &str) {
        loop {
            match self.used.recv_timeout(READ_TIMEOUT) {
                Ok(used) if used >= number => return,
                Ok(_) => {}
                Err(error) => {
                    panic!(
                        "the reading thread did not decode {what} within {READ_TIMEOUT:?}: {error}"
                    )
                }
            }
        }
    }

    fn next(&mut self, what: &str) -> Value {
        match self.frames.recv_timeout(READ_TIMEOUT) {
            Ok(frame) => {
                self.seen.push(frame.clone());
                frame
            }
            Err(error) => panic!(
                "waiting for {what}, the server wrote nothing within {READ_TIMEOUT:?} after {:?}: {error}",
                self.seen
            ),
        }
    }

    fn skip_to(&mut self, what: &str, found: impl Fn(&Value) -> bool) -> Value {
        loop {
            let frame = self.next(what);
            if found(&frame) {
                return frame;
            }
        }
    }

    /// Reads frames until every request id in `ids` has been answered.
    fn answers(&mut self, ids: &[i64]) {
        while !ids
            .iter()
            .all(|&id| self.seen.iter().any(|frame| is_answer(frame, id)))
        {
            self.next(&format!("answers to {ids:?}"));
        }
    }

    fn open_thread(&mut self, tools: bool) -> String {
        let params = if tools {
            json!({"dynamicTools": [{"name": "probe", "inputSchema": {"type": "object"}}]})
        } else {
            json!({})
        };
        self.send(&[
            json!({"id": 1, "method": "initialize", "params": {
                "clientInfo": {"name": "test"},
                "capabilities": {"experimentalApi": true},
            }}),
            json!({"method": "initialized", "params": {}}),
            json!({"id": 2, "method": "thread/start", "params": params}),
        ]);
        let thread = self.skip_to("the answer to `thread/start`", |frame| is_answer(frame, 2));
        thread["result"]["thread"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("thread/start answered with an id: {thread}"))
            .to_owned()
    }

    /// Closes the input and returns how the connection ended and every frame the server wrote.
    fn end(mut self) -> (Result<(), String>, Vec<Value>) {
        let (closed, _) = channel::<Vec<u8>>();
        drop(std::mem::replace(&mut self.chunks, closed));
        let ended = match self.ended.recv_timeout(READ_TIMEOUT) {
            Ok(result) => result,
            Err(error) => panic!(
                "the server did not end within {READ_TIMEOUT:?} of its input closing: {error}; {:?}",
                self.seen
            ),
        };
        while let Ok(frame) = self.frames.try_recv() {
            self.seen.push(frame);
        }
        (ended, self.seen)
    }

    fn finish(self) -> Vec<Value> {
        let (ended, frames) = self.end();
        if let Err(error) = ended {
            panic!("the connection ended with an error: {error}; {frames:?}");
        }
        frames
    }
}

fn is_answer(frame: &Value, id: i64) -> bool {
    frame.get("id") == Some(&json!(id)) && frame.get("method").is_none()
}

fn is_method(frame: &Value, method: &str) -> bool {
    frame.get("method").and_then(Value::as_str) == Some(method)
}

fn turn_start(id: i64, thread_id: &str, text: &str) -> Value {
    json!({"id": id, "method": "turn/start", "params": {
        "threadId": thread_id, "input": [{"type": "text", "text": text}],
    }})
}

fn turn_interrupt(id: i64, thread_id: &str) -> Value {
    json!({"id": id, "method": "turn/interrupt", "params": {"threadId": thread_id}})
}

fn tool_answer(id: &Value) -> Value {
    json!({"id": id, "result": {
        "success": true,
        "contentItems": [{"type": "inputText", "text": "probed"}],
    }})
}

fn position(frames: &[Value], found: impl Fn(&Value) -> bool) -> Option<usize> {
    frames.iter().position(found)
}

fn answered_at(frames: &[Value], id: i64) -> usize {
    position(frames, |frame| is_answer(frame, id))
        .unwrap_or_else(|| panic!("request {id} was never answered: {frames:?}"))
}

#[test]
fn an_interrupt_sent_after_a_held_request_is_not_answered_ahead_of_it() {
    // Turn 1 waits on `probe`. The client stops it (98), asks twice (51, 52) and answers the
    // call: the wait answers 98 and holds 51 and 52 in `deferred`. The main thread is held writing
    // turn 1's terminal frame -- the turn already let go of -- while the client stops again (99),
    // so 99 is counted for whichever turn comes next. The main loop starts turn 2 from 51; turn 2
    // takes the count, ends before it drains anything, and its settle reads the channel for 99.
    //
    // The client sent 52 before 99. Every reader that serves requests in order is documented to
    // take `deferred` before the channel (`Wire::serve_control`, fact 3); the settle does not, so
    // 99 is answered ahead of the 52 still held, and 52 then starts a third turn after the
    // interrupt the client sent behind it.
    let (release, released) = channel::<()>();
    let mut client = InProcess::serve(
        Some((|frame: &Value| is_method(frame, "turn/completed"), released)),
        probe_then_stream(),
    );
    let thread_id = client.open_thread(true);

    client.send(&[turn_start(50, &thread_id, "probe it")]);
    let call = client.skip_to("the `item/tool/call` request", |frame| {
        is_method(frame, "item/tool/call")
    });
    client.send(&[
        turn_interrupt(98, &thread_id),
        turn_start(51, &thread_id, "instead, this"),
        turn_start(52, &thread_id, "and then this"),
        tool_answer(&call["id"]),
    ]);
    client.skip_to("the first turn's `turn/completed`", |frame| {
        is_method(frame, "turn/completed")
    });
    let number = client.send(&[turn_interrupt(99, &thread_id)]);
    client.decoded(number, "the second interrupt");
    let _ = release.send(());

    client.answers(&[51, 52, 99]);
    let frames = client.finish();

    let held = answered_at(&frames, 52);
    let interrupt = answered_at(&frames, 99);
    assert!(
        held < interrupt,
        "{STORY}: request 52 was sent before interrupt 99 and was held in `deferred`, yet \
         `Wire::settle_interrupts` read the channel past it and answered 99 first (52 at frame \
         {held}, 99 at frame {interrupt}); a held request must be served before any frame the \
         client sent after it. Frames: {frames:?}"
    );
}

#[test]
fn requests_held_behind_an_interrupt_are_served_in_the_order_they_were_sent() {
    // Two requests held while a tool call is outstanding: a `turn/start` (51), then a method this
    // server does not serve (52). The `turn/start` was sent first, so it is the next turn; the
    // other is met by that turn's drain. Served the other way round, 52 is answered by the main
    // loop before 51 is even started. The unit's own cases hold one request each, so a `deferred`
    // drained newest-first passes all of them.
    let mut client = InProcess::serve(None, probe_then_stream());
    let thread_id = client.open_thread(true);

    client.send(&[turn_start(50, &thread_id, "probe it")]);
    let call = client.skip_to("the `item/tool/call` request", |frame| {
        is_method(frame, "item/tool/call")
    });
    client.send(&[
        turn_interrupt(98, &thread_id),
        turn_start(51, &thread_id, "instead, this"),
        json!({"id": 52, "method": "example/unknown", "params": {}}),
        tool_answer(&call["id"]),
    ]);

    client.answers(&[51, 52]);
    let frames = client.finish();

    let started = answered_at(&frames, 51);
    let other = answered_at(&frames, 52);
    assert!(
        frames[started]["error"].is_null(),
        "{STORY}: the `turn/start` held behind the interrupt starts the next turn: {frames:?}"
    );
    assert!(
        started < other,
        "{STORY}: two requests held in `deferred` are served in the order the client sent them: \
         51 was sent first and answered at frame {started}, 52 at frame {other}. Frames: \
         {frames:?}"
    );
}

#[test]
fn a_request_held_behind_an_interrupt_is_answered_before_a_later_malformed_frame_ends_the_connection()
 {
    // Interrupt, `turn/start`, a line that is not JSON, interrupt: all decoded while one turn
    // streams. The settle holds the `turn/start` in `deferred` and stashes the malformed frame.
    // `next_frame` prefers the stash, so the main loop meets the malformed frame -- sent after
    // the `turn/start` -- first and ends the connection, and the `turn/start` is never answered.
    // On the base the settle refused it on the spot, so the client had an answer to every request
    // it sent before the frame that broke the connection.
    let (release, released) = channel::<()>();
    let mut client = InProcess::serve(
        Some((
            |frame: &Value| is_method(frame, "item/agentMessage/delta"),
            released,
        )),
        streams_always(),
    );
    let thread_id = client.open_thread(false);

    client.send(&[turn_start(50, &thread_id, "go")]);
    client.skip_to("the first delta", |frame| {
        is_method(frame, "item/agentMessage/delta")
    });
    let number = client.send_lines(&[
        turn_interrupt(98, &thread_id).to_string(),
        turn_start(51, &thread_id, "instead, this").to_string(),
        "this line is not json".to_owned(),
        turn_interrupt(99, &thread_id).to_string(),
    ]);
    client.decoded(
        number,
        "the interrupt, the turn/start, the malformed line and the second interrupt",
    );
    let _ = release.send(());
    client.skip_to("the first turn's `turn/completed`", |frame| {
        is_method(frame, "turn/completed")
    });

    let (ended, frames) = client.end();
    assert!(
        ended.is_err(),
        "the malformed frame ends the connection with an error: {frames:?}"
    );
    assert!(
        position(&frames, |frame| is_answer(frame, 51)).is_some(),
        "{STORY}: request 51 was sent before the malformed frame, but `Wire::next_frame` served \
         the stashed malformed frame ahead of the held request, so the connection ended with 51 \
         unanswered. Ended: {ended:?} Frames: {frames:?}"
    );
}
