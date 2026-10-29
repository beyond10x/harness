//! Adversarial cases against the corrected `LoopTask` (story `loop-task-awaits-a-run`, pass 2):
//! skipped and abandoned commands, and the panics `shutdown` reports.
//!
//! Each test bounds every wait at [`WAIT`].

use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use b10x_harness_loop as harness_loop;
use harness_loop::{
    ApproveAll, LoopCancel, LoopConfig, LoopEvent, LoopPorts, LoopSink, LoopTask, LoopTaskError,
    NullLoopSink,
};
use harness_wire::{
    Approval, CallId, Envelope, Item, ModelPort, Risk, StopReason, StreamSink, ToolCall, ToolName,
    ToolOutcome, ToolPort, ToolSpec, TurnOutcome, TurnRequest, WireError, WireId,
};
use serde_json::json;

const WAIT: Duration = Duration::from_secs(5);

/// The name `LoopTask` gives its thread.
const TASK_THREAD: &str = "harness-loop-task";

async fn within<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(WAIT, future)
        .await
        .expect("the loop task answered within 5 s")
}

fn config() -> LoopConfig {
    LoopConfig::new("fixture", "Answer briefly.")
}

fn tool_call(id: &str) -> ToolCall {
    ToolCall {
        call_id: CallId::new(id).unwrap(),
        name: ToolName::new("read").unwrap(),
        arguments: json!({}),
    }
}

fn answer(text: &str) -> TurnOutcome {
    TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::AssistantText {
            text: text.to_owned(),
        }],
        usage: None,
    }
}

fn wait_until(ready: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + WAIT;
    while !ready() {
        if Instant::now() > deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(1));
    }
    true
}

type Turn = Box<dyn FnMut() -> Result<TurnOutcome, WireError>>;

/// A model whose turn is a closure, with an optional action when it is dropped.
struct Model {
    wire: WireId,
    turn: Turn,
    on_drop: Option<Box<dyn FnOnce()>>,
}

impl Model {
    fn new(turn: impl FnMut() -> Result<TurnOutcome, WireError> + 'static) -> Self {
        Self {
            wire: WireId::new("fixture").unwrap(),
            turn: Box::new(turn),
            on_drop: None,
        }
    }

    fn scripted(script: Vec<TurnOutcome>) -> Self {
        let mut script = VecDeque::from(script);
        Self::new(move || {
            script
                .pop_front()
                .ok_or_else(|| WireError::protocol("the script is exhausted"))
        })
    }

    fn on_drop(mut self, action: impl FnOnce() + 'static) -> Self {
        self.on_drop = Some(Box::new(action));
        self
    }
}

impl ModelPort for Model {
    fn wire(&self) -> &WireId {
        &self.wire
    }

    fn turn(&mut self, _: &TurnRequest, _: &mut dyn StreamSink) -> Result<TurnOutcome, WireError> {
        (self.turn)()
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        if let Some(action) = self.on_drop.take() {
            action();
        }
    }
}

/// One low-risk `read` tool, recording each call id.
struct Tools {
    specs: Vec<ToolSpec>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Tools {
    fn new(seen: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            specs: vec![ToolSpec {
                name: ToolName::new("read").unwrap(),
                description: "read".to_owned(),
                input_schema: json!({"type": "object"}),
                approval: Approval::NotRequired,
                envelope: Envelope {
                    risk: Risk::Low,
                    ..Envelope::default()
                },
            }],
            seen,
        }
    }
}

impl ToolPort for Tools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        self.seen
            .lock()
            .unwrap()
            .push(call.call_id.as_str().to_owned());
        ToolOutcome::ok(json!("effect"))
    }
}

fn tools() -> Tools {
    Tools::new(Arc::new(Mutex::new(Vec::new())))
}

/// Says when its command reached the first event, then holds the thread until the test opens it.
/// Panics with `message` when dropped on the task's thread, if one is given.
struct Gate {
    gate: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
    explodes: Option<&'static str>,
}

impl Gate {
    fn holding() -> (Self, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (entered, in_command) = mpsc::channel();
        let (open, gated) = mpsc::channel();
        let gate = Self {
            gate: Some((entered, gated)),
            explodes: None,
        };
        (gate, in_command, open)
    }

    fn exploding(message: &'static str) -> Self {
        Self {
            gate: None,
            explodes: Some(message),
        }
    }
}

impl LoopSink for Gate {
    fn emit(&mut self, _: LoopEvent) {
        if let Some((entered, gated)) = self.gate.take() {
            entered.send(()).unwrap();
            gated.recv_timeout(WAIT).unwrap();
        }
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        if let Some(message) = self.explodes {
            assert!(thread::current().name() != Some(TASK_THREAD), "{message}");
        }
    }
}

/// A run whose future is dropped while it runs cancels the token; a port that panics on that
/// cancel retires the ports, and the `Panicked` answer goes to a future that is gone. The
/// documentation says a panic "nobody is waiting to hear about … is caught too and kept: … and
/// `shutdown` resolves to it". Here `shutdown` resolves `Ok(())` and the panic is lost.
#[tokio::test]
async fn a_panic_in_a_run_whose_future_was_dropped_is_reported_at_shutdown() {
    let (entered, in_turn) = mpsc::channel();
    let task = LoopTask::spawn(config(), move |cancel: &LoopCancel| {
        let token = cancel.clone();
        Ok::<_, String>(LoopPorts::new(
            Model::new(move || {
                entered.send(()).unwrap();
                assert!(wait_until(|| token.is_cancelled()), "never cancelled");
                panic!("the model port panicked on cancel");
            }),
            tools(),
            ApproveAll,
        ))
    });
    let running = task.run("go", NullLoopSink);
    in_turn
        .recv_timeout(WAIT)
        .expect("the model entered its turn");
    drop(running);
    let ended = within(task.shutdown()).await;
    assert!(
        matches!(&ended, Err(LoopTaskError::Panicked(message)) if message.contains("panicked on cancel")),
        "the port's panic reached nobody: shutdown resolved to {ended:?}"
    );
}

/// "A caught panic is reported at `shutdown` as `Panicked`, keeping the first one": two skipped
/// commands whose sinks panic on drop, then a port whose drop panics at shutdown.
#[tokio::test]
async fn shutdown_reports_the_first_unreported_panic() {
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Model::scripted(Vec::new()).on_drop(|| panic!("third: the model port's drop")),
            tools(),
            ApproveAll,
        ))
    });
    let (gate, in_first, open) = Gate::holding();
    let first = task.call(tool_call("first"), gate);
    let one = task.run("go", Gate::exploding("first: a skipped command's sink"));
    let two = task.run("go", Gate::exploding("second: a skipped command's sink"));
    in_first
        .recv_timeout(WAIT)
        .expect("the first command started");
    drop(one);
    drop(two);
    open.send(()).unwrap();
    within(first).await.unwrap();
    let ended = within(task.shutdown()).await;
    assert!(
        matches!(&ended, Err(LoopTaskError::Panicked(message)) if message.starts_with("first:")),
        "{ended:?}"
    );
}

/// "After a panic in a command the ports are dropped and every later command resolves to that
/// same error". No implementor test runs a second command after a panic.
#[tokio::test]
async fn after_a_panicking_port_the_ports_are_gone_and_later_commands_answer_the_panic() {
    let dropped = Arc::new(AtomicBool::new(false));
    let mark = Arc::clone(&dropped);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Model::new(|| panic!("the model port exploded"))
                .on_drop(move || mark.store(true, Ordering::SeqCst)),
            Tools::new(record),
            ApproveAll,
        ))
    });
    let first = within(task.run("go", NullLoopSink)).await;
    assert!(
        matches!(&first, Err(LoopTaskError::Panicked(m)) if m.contains("exploded")),
        "{first:?}"
    );
    assert!(
        dropped.load(Ordering::SeqCst),
        "the ports outlived the panic"
    );
    let later = within(task.call(tool_call("later"), NullLoopSink)).await;
    assert_eq!(
        later.map(|(outcome, _)| outcome),
        Err(first.unwrap_err()),
        "a later command did not answer the same panic"
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "a later command reached the ports"
    );
    within(task.shutdown()).await.unwrap();
}

/// The drop path pass 1 measured, now reached after the command started: the answer is unread,
/// its sink panics on drop, and `shutdown` reports it.
#[tokio::test]
async fn a_panicking_sink_of_an_unread_answer_is_reported_at_shutdown() {
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Model::scripted(vec![answer("done")]),
            tools(),
            ApproveAll,
        ))
    });
    let (mut gate, in_run, open) = Gate::holding();
    gate.explodes = Some("the unread answer's sink exploded");
    let running = task.run("go", gate);
    in_run.recv_timeout(WAIT).expect("the run started");
    drop(running);
    open.send(()).unwrap();
    let ended = within(task.shutdown()).await;
    assert!(
        matches!(&ended, Err(LoopTaskError::Panicked(m)) if m.contains("unread answer's sink")),
        "{ended:?}"
    );
}

/// Dropping a future right after sending its command races the thread's start. Whichever side
/// wins, a skipped command never cancels the token, and the task never hangs.
#[tokio::test]
async fn a_future_dropped_at_its_start_either_skips_or_cancels() {
    for round in 0..200_u32 {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&seen);
        let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
            Ok::<_, String>(LoopPorts::new(
                Model::scripted(Vec::new()),
                Tools::new(record),
                ApproveAll,
            ))
        });
        let token = task.cancel_handle();
        let racing = task.call(tool_call("racing"), NullLoopSink);
        for _ in 0..(round % 50) * 200 {
            std::hint::spin_loop();
        }
        drop(racing);
        within(task.shutdown()).await.unwrap();
        let ran = !seen.lock().unwrap().is_empty();
        assert!(
            ran || !token.is_cancelled(),
            "round {round}: a command that never ran cancelled the token"
        );
    }
}
