//! story:bridge-turn-start-after-interrupt-on-every-path.
//!
//! A `turn/start` sent after an interrupt is the next turn, whichever reader of the queue meets it:
//! the drain between streamed events (covered by `adversary_d288_u2.rs`), the settle that answers
//! interrupts still owed before a terminal frame, and the wait for a tool call's answer. A
//! `turn/start` sent *before* any interrupt, while a turn runs, is still refused: the order the
//! client wrote the frames in decides, not how far the reading thread had got.
//!
//! Driven in process, as `adversary_d288_u2.rs` drives the server: the test holds the client's
//! stdin, the server's stdout and the model factory, so every case is decided by frame order and by
//! the decode count the reading thread reports, never by a sleep.

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

/// The client's half of the server's stdin, handed over one chunk at a time.
///
/// Each time the reading thread has used up every chunk it was given and asks for more, the number
/// of chunks it has used up goes out on `used`: the reading thread decodes a line, counts an
/// interrupt and queues the frame before it reads again, so a count that covers a chunk proves
/// every frame in it was decoded, counted and queued.
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
/// `hold` matches. The main thread is the only writer and the only consumer of the reading
/// thread's queue, so while it is held inside a write it dequeues nothing.
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

/// Streams one delta and then answers, the shape of every streamed turn.
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

/// Asks for the client's `probe` tool once, then answers without streaming.
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

    fn send(&mut self, frames: &[Value]) -> usize {
        let mut chunk = String::new();
        for frame in frames {
            chunk.push_str(&frame.to_string());
            chunk.push('\n');
        }
        self.chunks
            .send(chunk.into_bytes())
            .expect("the server's reading thread is alive");
        self.sent += 1;
        self.sent
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

    /// Waits, after the first turn's terminal frame, for the answer to `turn/start` 51 and, when
    /// it started a turn, for that turn's terminal frame. A refusal fails here, quoting it, rather
    /// than as a wait for a turn that never comes.
    fn second_turn_runs(&mut self) {
        let answer = match position(&self.seen, |frame| is_answer(frame, 51)) {
            Some(at) => self.seen[at].clone(),
            None => self.skip_to("the answer to the turn/start 51", |frame| {
                is_answer(frame, 51)
            }),
        };
        assert!(
            answer["error"].is_null(),
            "{STORY}: a `turn/start` sent after an interrupt was refused rather than starting the \
             next turn: {answer} Frames: {:?}",
            self.seen
        );
        self.skip_to("the second turn's `turn/completed`", |frame| {
            is_method(frame, "turn/completed")
        });
    }

    /// Opens the connection's thread, with `probe` registered when `tools` is set.
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

    /// Closes the input; the connection must end cleanly. Returns every frame the server wrote.
    fn finish(mut self) -> Vec<Value> {
        let (closed, _) = channel::<Vec<u8>>();
        drop(std::mem::replace(&mut self.chunks, closed));
        match self.ended.recv_timeout(READ_TIMEOUT) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => panic!(
                "the connection ended with an error: {error}; {:?}",
                self.seen
            ),
            Err(error) => panic!(
                "the server did not end within {READ_TIMEOUT:?} of its input closing: {error}; {:?}",
                self.seen
            ),
        }
        while let Ok(frame) = self.frames.try_recv() {
            self.seen.push(frame);
        }
        self.seen
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

/// The indices of every `turn/completed` in `frames`.
fn terminals(frames: &[Value]) -> Vec<usize> {
    frames
        .iter()
        .enumerate()
        .filter(|(_, frame)| is_method(frame, "turn/completed"))
        .map(|(at, _)| at)
        .collect()
}

#[test]
fn a_turn_start_between_two_interrupts_owed_to_one_streaming_turn_starts_the_next_turn() {
    // Interrupt, `turn/start`, interrupt, all decoded while one turn streams. The server is held
    // writing the turn's first delta, so all three are counted against that turn before the drain
    // that follows the write. The drain answers the first interrupt and stops; the second is still
    // owed when the turn ends, so `Wire::settle_interrupts` reads the queue up to it -- and meets
    // the `turn/start` on the way.
    let (release, released) = channel::<()>();
    let mut client = InProcess::serve(
        Some((
            |frame: &Value| is_method(frame, "item/agentMessage/delta"),
            released,
        )),
        Box::new(|_cancel: Cancel| -> Result<Box<dyn ModelPort>, String> {
            Ok(Box::new(Streams(wire_id())))
        }),
    );
    let thread_id = client.open_thread(false);

    client.send(&[turn_start(50, &thread_id, "go")]);
    client.skip_to("the first delta", |frame| {
        is_method(frame, "item/agentMessage/delta")
    });
    let number = client.send(&[
        turn_interrupt(98, &thread_id),
        turn_start(51, &thread_id, "instead, this"),
        turn_interrupt(99, &thread_id),
    ]);
    client.decoded(
        number,
        "the interrupt, the turn/start and the second interrupt",
    );
    let _ = release.send(());

    client.skip_to("the first turn's `turn/completed`", |frame| {
        is_method(frame, "turn/completed")
    });
    client.second_turn_runs();
    let frames = client.finish();

    let ends = terminals(&frames);
    let first_end = ends[0];
    assert_eq!(
        frames[first_end]["params"]["turn"]["status"],
        json!("interrupted"),
        "{STORY}: the streaming turn both interrupts were sent to ends `interrupted`: {frames:?}"
    );
    for interrupt in [98, 99] {
        let answered = position(&frames, |frame| is_answer(frame, interrupt));
        assert!(
            answered.is_some_and(|at| at < first_end && frames[at]["error"].is_null()),
            "{STORY}: interrupt {interrupt} was sent to the streaming turn and must be answered \
             with a success before that turn's `turn/completed`: {frames:?}"
        );
    }
    let second = position(&frames, |frame| is_answer(frame, 51))
        .unwrap_or_else(|| panic!("{STORY}: the turn/start 51 was never answered: {frames:?}"));
    assert!(
        frames[second]["error"].is_null(),
        "{STORY}: a `turn/start` sent after an interrupt and queued between two interrupts owed to \
         one turn was refused rather than starting the next turn: {} Frames: {frames:?}",
        frames[second]
    );
    assert!(
        second > first_end,
        "{STORY}: the next turn is announced after the interrupted turn's terminal frame: \
         {frames:?}"
    );
    let second_turn = frames[second]["result"]["turn"]["id"].clone();
    let second_end = ends
        .iter()
        .copied()
        .find(|&at| frames[at]["params"]["turn"]["id"] == second_turn);
    assert!(
        second_end.is_some_and(|at| frames[at]["params"]["turn"]["status"] == json!("completed")),
        "{STORY}: the turn the `turn/start` asked for ran to completion: {frames:?}"
    );
}

#[test]
fn a_turn_start_after_an_interrupt_while_a_tool_call_is_outstanding_starts_the_next_turn() {
    // The first turn asks the client for `probe` and waits in `BridgeTools::await_response`. The
    // client stops it, asks again, and then answers the call it still owes -- one chunk, in that
    // order. The wait answers the interrupt, meets the `turn/start`, and goes on waiting for the
    // call's answer, which comes next.
    let mut client = InProcess::serve(None, probe_then_stream());
    let thread_id = client.open_thread(true);

    client.send(&[turn_start(50, &thread_id, "probe it")]);
    let call = client.skip_to("the `item/tool/call` request", |frame| {
        is_method(frame, "item/tool/call")
    });
    client.send(&[
        turn_interrupt(99, &thread_id),
        turn_start(51, &thread_id, "instead, this"),
        tool_answer(&call["id"]),
    ]);

    client.skip_to("the first turn's `turn/completed`", |frame| {
        is_method(frame, "turn/completed")
    });
    client.second_turn_runs();
    let frames = client.finish();

    let ends = terminals(&frames);
    let first_end = ends[0];
    assert_eq!(
        frames[first_end]["params"]["turn"]["status"],
        json!("interrupted"),
        "{STORY}: the turn whose tool call was outstanding ends `interrupted`: {frames:?}"
    );
    let interrupt = position(&frames, |frame| is_answer(frame, 99));
    assert!(
        interrupt.is_some_and(|at| at < first_end && frames[at]["error"].is_null()),
        "{STORY}: the interrupt is answered with a success before the turn's terminal frame: \
         {frames:?}"
    );
    let second = position(&frames, |frame| is_answer(frame, 51))
        .unwrap_or_else(|| panic!("{STORY}: the turn/start 51 was never answered: {frames:?}"));
    assert!(
        frames[second]["error"].is_null(),
        "{STORY}: a `turn/start` sent after an interrupt while a tool call was outstanding was \
         refused rather than starting the next turn: {} Frames: {frames:?}",
        frames[second]
    );
    assert!(
        second > first_end,
        "{STORY}: the next turn is announced after the interrupted turn's terminal frame: \
         {frames:?}"
    );
    let second_turn = frames[second]["result"]["turn"]["id"].clone();
    assert!(
        ends.iter().any(|&at| {
            frames[at]["params"]["turn"]["id"] == second_turn
                && frames[at]["params"]["turn"]["status"] == json!("completed")
        }),
        "{STORY}: the turn the `turn/start` asked for ran to completion: {frames:?}"
    );
}

#[test]
fn a_turn_start_sent_before_the_interrupt_while_a_tool_call_is_outstanding_is_still_refused() {
    // The order the client wrote the frames in decides, not how far the reading thread got. The
    // server is held writing `item/tool/call`, so the `turn/start` and the interrupt behind it are
    // both decoded -- the turn already marked as asked to stop -- before the wait reads either.
    // The `turn/start` was sent while the turn ran and before anyone stopped it, so it is refused
    // as any request sent to a running turn is; only what follows the interrupt waits for the
    // main loop.
    let (release, released) = channel::<()>();
    let mut client = InProcess::serve(
        Some((|frame: &Value| is_method(frame, "item/tool/call"), released)),
        probe_then_stream(),
    );
    let thread_id = client.open_thread(true);

    client.send(&[turn_start(50, &thread_id, "probe it")]);
    let call = client.skip_to("the `item/tool/call` request", |frame| {
        is_method(frame, "item/tool/call")
    });
    let number = client.send(&[
        turn_start(51, &thread_id, "sent before the stop"),
        turn_interrupt(99, &thread_id),
    ]);
    client.decoded(number, "the turn/start and the interrupt behind it");
    let _ = release.send(());
    let refused = client.skip_to("the answer to the turn/start 51", |frame| {
        is_answer(frame, 51)
    });
    client.send(&[tool_answer(&call["id"])]);
    client.skip_to("the turn's `turn/completed`", |frame| {
        is_method(frame, "turn/completed")
    });
    let frames = client.finish();

    assert_eq!(
        refused["error"]["code"],
        json!(-32601),
        "{STORY}: a `turn/start` sent before the interrupt, while a tool call was outstanding, is \
         refused as before: {refused} Frames: {frames:?}"
    );
    assert!(
        refused["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("while a tool call is outstanding")),
        "{STORY}: the refusal comes from the wait for the tool call's answer: {refused}"
    );
    let ends = terminals(&frames);
    assert_eq!(
        ends.len(),
        1,
        "{STORY}: the refused `turn/start` starts no turn: {frames:?}"
    );
    let interrupt = position(&frames, |frame| is_answer(frame, 99));
    assert!(
        interrupt.is_some_and(|at| at < ends[0]),
        "{STORY}: the interrupt is answered before the turn's terminal frame: {frames:?}"
    );
    assert_eq!(
        frames[ends[0]]["params"]["turn"]["status"],
        json!("interrupted"),
        "{frames:?}"
    );
}
