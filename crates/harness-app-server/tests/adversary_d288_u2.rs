//! Adversary pass 1 against wave d288's U2 (`clear_active` moved ahead of the settle).
//!
//! The server is driven in process, as `harness-cli`'s `bridge_mode.rs` drives it: the test holds
//! the client's stdin, the server's stdout and the model factory, so which side of a server step an
//! interrupt is decoded on is the test's choice on every attempt, under any load.

use std::io::{BufRead, Read, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::{Duration, Instant};

use b10x_harness_app_server::{self as harness_app_server, ServerConfig};
use harness_loop::Budget;
use harness_wire::{
    Cancel, Item, ModelPort, StopReason, StreamEvent, StreamSink, TurnOutcome, TurnRequest,
    WireError, WireId,
};
use serde_json::{Value, json};

const READ_TIMEOUT: Duration = Duration::from_secs(20);

/// The client's half of the server's stdin, handed over one chunk at a time.
///
/// Each time the reading thread has used up every chunk it was given and asks for more, the number
/// of chunks it has used up goes out on `used`: `pump` decodes a line, fires its interrupt watch and
/// queues the frame before it reads again, so a count that covers a chunk proves every frame in it
/// was decoded, counted and queued.
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

/// Which written frame holds the server's main thread, and for how many matches to let pass first.
type HoldOn = fn(&Value) -> bool;

/// The server's stdout, split into frames, able to hold the main thread after one chosen frame.
///
/// The main thread is the only writer and the only consumer of the reading thread's queue, so while
/// it is held inside a write it dequeues nothing.
struct HeldOutput {
    pending: Vec<u8>,
    frames: Sender<Value>,
    hold: Option<(HoldOn, usize, Receiver<()>)>,
}

impl Write for HeldOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            let frame: Value = serde_json::from_slice(&line).map_err(std::io::Error::other)?;
            let mut held = false;
            if let Some((matches, skip, _)) = self.hold.as_mut()
                && matches(&frame)
            {
                if *skip == 0 {
                    held = true;
                } else {
                    *skip -= 1;
                }
            }
            let _ = self.frames.send(frame);
            if held && let Some((_, _, release)) = self.hold.take() {
                let _ = release.recv_timeout(READ_TIMEOUT);
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A model that streams one delta and then answers, the shape of every streamed turn.
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

type Factory = Box<dyn FnMut(Cancel) -> Result<Box<dyn ModelPort>, String> + Send>;

struct InProcess {
    chunks: Sender<Vec<u8>>,
    sent: usize,
    used: Receiver<usize>,
    frames: Receiver<Value>,
    seen: Vec<Value>,
    server: thread::JoinHandle<Result<(), String>>,
}

impl InProcess {
    fn serve(
        budget: Budget,
        hold: Option<(HoldOn, usize, Receiver<()>)>,
        factory: Factory,
    ) -> Self {
        let (chunk_sender, chunks) = channel::<Vec<u8>>();
        let (used_sender, used) = channel::<usize>();
        let (frame_sender, frames) = channel::<Value>();
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
            budget,
            context_window: None,
            version: "test".to_owned(),
        };
        let server = thread::spawn(move || {
            let mut factory = factory;
            harness_app_server::serve(&config, &mut *factory, input, output)
                .map_err(|error| error.to_string())
        });
        Self {
            chunks: chunk_sender,
            sent: 0,
            used,
            frames,
            seen: Vec::new(),
            server,
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

    fn open_thread(&mut self) -> String {
        self.send(&[
            json!({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "test"}}}),
            json!({"method": "initialized", "params": {}}),
            json!({"id": 2, "method": "thread/start", "params": {}}),
        ]);
        let thread = self.skip_to("the answer to `thread/start`", |frame| is_answer(frame, 2));
        thread["result"]["thread"]["id"]
            .as_str()
            .expect("thread/start answered with an id")
            .to_owned()
    }

    /// Closes the input; the connection must end cleanly.
    fn finish(self) -> Vec<Value> {
        let Self {
            chunks,
            seen,
            server,
            ..
        } = self;
        drop(chunks);
        let deadline = Instant::now() + READ_TIMEOUT;
        while !server.is_finished() {
            assert!(
                Instant::now() < deadline,
                "the server did not end within {READ_TIMEOUT:?} of its input closing: {seen:?}"
            );
            thread::sleep(Duration::from_millis(1));
        }
        match server.join() {
            Ok(Ok(())) => seen,
            Ok(Err(error)) => panic!("the connection ended with an error: {error}; {seen:?}"),
            Err(panic) => std::panic::resume_unwind(panic),
        }
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

/// The index of the first frame `found` matches in `frames`.
fn position(frames: &[Value], found: impl Fn(&Value) -> bool) -> Option<usize> {
    frames.iter().position(found)
}

#[test]
fn adv_d288_u2_an_interrupt_pipelined_behind_the_second_turn_is_carried_into_it() {
    // Every in-process case in `bridge_mode.rs` drives the first turn of a connection, where the
    // slot `Interrupts::active` is empty before `turn/start` whether or not anything ever empties
    // it. This one drives the second. The first turn ends; the server is held writing its
    // `turn/completed`, which `drive_turn` reaches only after it has let go of the turn
    // (`clear_active`, lib.rs:611). A `turn/start` and a `turn/interrupt` are pipelined and
    // decoded behind it. The interrupt was sent after that `turn/start`, so it is the second
    // turn's: the reading thread has to leave it as `stray`, and `install` has to carry it into
    // the second turn, cancelled before its model client is built.
    //
    // If the slot still held the first turn, the reading thread would count the interrupt against
    // a turn that is over: the second turn streams the answer the client cancelled, or (with this
    // factory) ends `failed` and gets its interrupt answered after its terminal frame.
    let (release, released) = channel::<()>();
    let (cancelled_sender, cancelled) = channel::<bool>();
    let mut built = 0_u32;
    let mut client = InProcess::serve(
        Budget::default(),
        Some((
            |frame: &Value| is_method(frame, "turn/completed"),
            0,
            released,
        )),
        Box::new(
            move |cancel: Cancel| -> Result<Box<dyn ModelPort>, String> {
                built += 1;
                if built == 2 {
                    let _ = cancelled_sender.send(cancel.is_cancelled());
                }
                Err("the model client will not build".to_owned())
            },
        ),
    );
    let thread_id = client.open_thread();

    client.send(&[turn_start(50, &thread_id, "first")]);
    let first = client.skip_to("the first turn's `turn/completed`", |frame| {
        is_method(frame, "turn/completed")
    });
    assert_eq!(
        first["params"]["turn"]["status"],
        json!("failed"),
        "the first turn, never interrupted, fails on its model client: {:?}",
        client.seen
    );

    // The main thread is held inside the first turn's terminal write.
    let number = client.send(&[
        turn_start(51, &thread_id, "second"),
        turn_interrupt(99, &thread_id),
    ]);
    client.decoded(number, "the second turn/start and its interrupt");
    let _ = release.send(());

    let cancelled_when_built = cancelled.recv_timeout(READ_TIMEOUT).unwrap_or_else(|_| {
        panic!(
            "the second turn never built its model client: {:?}",
            client.seen
        )
    });
    let terminal = client.skip_to("the second turn's `turn/completed`", |frame| {
        is_method(frame, "turn/completed")
    });
    let frames = client.finish();
    let start_of_second =
        position(&frames, |frame| is_answer(frame, 51)).expect("turn 51 answered");
    let answered_at = position(&frames, |frame| is_answer(frame, 99));
    let second_terminal = frames.len()
        - 1
        - frames
            .iter()
            .rev()
            .position(|frame| is_method(frame, "turn/completed"))
            .expect("a terminal");

    assert!(
        cancelled_when_built,
        "an interrupt sent after the second `turn/start` and decoded while the first turn's \
         terminal frame was being written did not cancel the second turn before its model client \
         was built: the slot still held the first turn, so the interrupt was counted against a turn \
         that was over (story:bridge-mode-interrupt-cases-are-deterministic). Frames: {frames:?}"
    );
    assert!(
        answered_at.is_some_and(|at| at > start_of_second && at < second_terminal),
        "the second turn's interrupt was not answered between its start and its terminal frame \
         (story:bridge-mode-interrupt-cases-are-deterministic). Frames: {frames:?}"
    );
    assert_eq!(
        terminal["params"]["turn"]["status"],
        json!("interrupted"),
        "a turn its interrupt cancelled must report `interrupted` \
         (story:bridge-mode-interrupt-cases-are-deterministic). Frames: {frames:?}"
    );
}

#[test]
fn adv_d288_u2_stop_and_resend_while_a_turn_streams_runs_the_next_turn() {
    // `bridge_mode.rs`'s `stopping_a_turn_and_starting_the_next_in_one_breath_runs_the_next_one`
    // asserts "a `turn/start` sent behind an interrupt must still start a turn" -- for a turn
    // cancelled before it streamed, where only `Wire::settle_interrupts` reads the queue and it
    // stops at the interrupt. A person who hits stop and types again does it while the answer is
    // streaming. Then `BridgeSink::notify` drains the queue between two events
    // (`Wire::drain_control`, which reads until the queue is empty), answers the interrupt, and
    // then refuses the `turn/start` behind it with "cannot be served while a turn is running" --
    // for a turn it has just cancelled, which writes its `interrupted` terminal frame a moment
    // later.
    //
    // Deterministic: the server is held writing the turn's first delta, so both frames are decoded
    // before the drain that follows that write.
    let (release, released) = channel::<()>();
    let mut client = InProcess::serve(
        Budget::default(),
        Some((
            |frame: &Value| is_method(frame, "item/agentMessage/delta"),
            0,
            released,
        )),
        Box::new(|_cancel: Cancel| -> Result<Box<dyn ModelPort>, String> {
            Ok(Box::new(Streams(
                WireId::new("openai-responses").expect("a valid wire id"),
            )))
        }),
    );
    let thread_id = client.open_thread();

    client.send(&[turn_start(50, &thread_id, "go")]);
    client.skip_to("the first delta", |frame| {
        is_method(frame, "item/agentMessage/delta")
    });
    let number = client.send(&[
        turn_interrupt(99, &thread_id),
        turn_start(51, &thread_id, "instead, this"),
    ]);
    client.decoded(number, "the interrupt and the second turn/start");
    let _ = release.send(());

    let first_terminal = client.skip_to("the first turn's `turn/completed`", |frame| {
        is_method(frame, "turn/completed")
    });
    assert_eq!(
        first_terminal["params"]["turn"]["status"],
        json!("interrupted"),
        "{:?}",
        client.seen
    );
    let second = match position(&client.seen, |frame| is_answer(frame, 51)) {
        Some(at) => client.seen[at].clone(),
        None => client.skip_to("the answer to the second `turn/start`", |frame| {
            is_answer(frame, 51)
        }),
    };
    assert!(
        second["error"].is_null(),
        "a person stopped a streaming turn and asked again in one breath; the interrupt was \
         answered and the turn ended `interrupted`, yet the `turn/start` sent behind the interrupt \
         was refused instead of starting the next turn: {second} \
         (story:bridge-mode-interrupt-cases-are-deterministic, the property \
         `stopping_a_turn_and_starting_the_next_in_one_breath_runs_the_next_one` asserts). \
         Frames: {:?}",
        client.seen
    );
    client.skip_to("the second turn's `turn/completed`", |frame| {
        is_method(frame, "turn/completed")
    });
    client.finish();
}
