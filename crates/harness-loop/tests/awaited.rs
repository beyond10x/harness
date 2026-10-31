//! `LoopTask`: the synchronous loop awaited from an async program (story
//! `loop-task-awaits-a-run`, epic decisions D1–D6).
//!
//! Every test that waits on the task's thread bounds the wait at [`WAIT`] and fails when it runs
//! out, so a task that never answers is a red test and never a hung one.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use b10x_harness_loop as harness_loop;
use harness_loop::{
    AgentLoop, ApprovalDecision, ApprovalPort, ApproveAll, DenyAll, LoopCancel, LoopConfig,
    LoopEvent, LoopPorts, LoopSink, LoopStop, LoopTask, LoopTaskError, NullLoopSink, RunLedger,
    VecLoopSink,
};
use harness_wire::{
    Approval, CallId, Envelope, Item, ModelPort, Risk, StopReason, StreamEvent, StreamSink,
    ToolCall, ToolName, ToolOutcome, ToolPort, ToolSpec, TurnOutcome, TurnRequest, Usage,
    WireError, WireId,
};
use serde_json::json;

/// The longest any test waits on the task's thread.
const WAIT: Duration = Duration::from_secs(5);

async fn within<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(WAIT, future)
        .await
        .expect("the loop task answered within 5 s")
}

fn config() -> LoopConfig {
    LoopConfig::new("fixture", "Answer briefly.")
}

fn usage(input: u64, output: u64) -> Usage {
    Usage {
        model: "fixture".to_owned(),
        input_tokens: input,
        output_tokens: output,
        cached_input_tokens: 0,
        cache_creation_input_tokens: None,
    }
}

fn answer(text: &str) -> TurnOutcome {
    TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::AssistantText {
            text: text.to_owned(),
        }],
        usage: Some(usage(10, 5)),
    }
}

fn tool_call(id: &str, name: &str) -> ToolCall {
    ToolCall {
        call_id: CallId::new(id).unwrap(),
        name: ToolName::new(name).unwrap(),
        arguments: json!({}),
    }
}

fn asks(id: &str, name: &str) -> TurnOutcome {
    TurnOutcome {
        stop_reason: StopReason::ToolCalls,
        items: vec![Item::ToolCall(tool_call(id, name))],
        usage: Some(usage(7, 3)),
    }
}

type Turn = Box<dyn FnMut(&TurnRequest, &mut dyn StreamSink) -> Result<TurnOutcome, WireError>>;

/// A model port whose every turn is a closure, with an optional action when it is dropped.
struct Model {
    wire: WireId,
    turn: Turn,
    on_drop: Option<Box<dyn FnOnce()>>,
}

impl Model {
    fn new(
        turn: impl FnMut(&TurnRequest, &mut dyn StreamSink) -> Result<TurnOutcome, WireError> + 'static,
    ) -> Self {
        Self {
            wire: WireId::new("fixture").unwrap(),
            turn: Box::new(turn),
            on_drop: None,
        }
    }

    /// Answers each turn with the next scripted outcome, streaming the text of each answer.
    fn scripted(script: Vec<Result<TurnOutcome, WireError>>) -> Self {
        let mut script = VecDeque::from(script);
        Self::new(move |_, sink| {
            let next = script
                .pop_front()
                .unwrap_or_else(|| Err(WireError::protocol("the script is exhausted")))?;
            for item in &next.items {
                if let Item::AssistantText { text } = item {
                    sink.emit(StreamEvent::TextDelta { text: text.clone() });
                }
            }
            Ok(next)
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

    fn turn(
        &mut self,
        request: &TurnRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        (self.turn)(request, sink)
    }
}

impl Drop for Model {
    fn drop(&mut self) {
        if let Some(action) = self.on_drop.take() {
            action();
        }
    }
}

/// A `read` tool at `Risk::Low` and a `write` tool at `Risk::Medium`, recording each call id.
struct Tools {
    specs: Vec<ToolSpec>,
    seen: Arc<Mutex<Vec<String>>>,
    /// How long the call with this id takes, so a second command could overtake it.
    slow: Option<(String, Duration)>,
}

impl Tools {
    fn new(seen: Arc<Mutex<Vec<String>>>) -> Self {
        let specs = [("read", Risk::Low), ("write", Risk::Medium)]
            .into_iter()
            .map(|(name, risk)| ToolSpec {
                name: ToolName::new(name).unwrap(),
                description: name.to_owned(),
                input_schema: json!({"type": "object"}),
                approval: Approval::NotRequired,
                envelope: Envelope {
                    risk,
                    ..Envelope::default()
                },
            })
            .collect();
        Self {
            specs,
            seen,
            slow: None,
        }
    }
}

impl ToolPort for Tools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        if let Some((id, delay)) = &self.slow
            && call.call_id.as_str() == id
        {
            thread::sleep(*delay);
        }
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

/// Defers every decision under one checkpoint id.
struct Deferring;

impl ApprovalPort for Deferring {
    fn decide(&mut self, _: &ToolCall, _: &ToolSpec) -> ApprovalDecision {
        ApprovalDecision::deferred("checkpoint-1")
    }
}

/// Waits up to [`WAIT`] for `ready`, polling; `false` when it never came.
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

#[tokio::test]
async fn awaited_run_leaves_the_runtime_thread_free() {
    // `#[tokio::test]` is a `current_thread` runtime: the spawned task runs only when the test's
    // own future yields. Run inline, the model's wait would hold the one thread and time out.
    let counter = Arc::new(AtomicU64::new(0));
    let watched = Arc::clone(&counter);
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Model::new(move |_, _| {
                if wait_until(|| watched.load(Ordering::SeqCst) == 1) {
                    Ok(answer("the runtime ran the spawned task"))
                } else {
                    Err(WireError::protocol("the spawned task never ran"))
                }
            }),
            tools(),
            DenyAll,
        ))
    });
    let bump = Arc::clone(&counter);
    tokio::spawn(async move {
        bump.fetch_add(1, Ordering::SeqCst);
    });
    let outcome = within(task.run("go", NullLoopSink))
        .await
        .unwrap()
        .result
        .unwrap();
    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.text, "the runtime ran the spawned task");
}

#[tokio::test]
async fn awaited_run_has_no_runtime_context() {
    let observed = Arc::new(Mutex::new(None));
    let record = Arc::clone(&observed);
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Model::new(move |_, _| {
                *record.lock().unwrap() = Some(tokio::runtime::Handle::try_current().is_err());
                Ok(answer("done"))
            }),
            tools(),
            DenyAll,
        ))
    });
    within(task.run("go", NullLoopSink))
        .await
        .unwrap()
        .result
        .unwrap();
    assert_eq!(
        *observed.lock().unwrap(),
        Some(true),
        "the model's turn ran inside a tokio runtime context"
    );
}

#[tokio::test]
async fn ports_are_built_on_the_task_thread() {
    let built: Arc<Mutex<Option<ThreadId>>> = Arc::new(Mutex::new(None));
    let turned: Arc<Mutex<Option<ThreadId>>> = Arc::new(Mutex::new(None));
    let (record_built, record_turned) = (Arc::clone(&built), Arc::clone(&turned));
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        *record_built.lock().unwrap() = Some(thread::current().id());
        Ok::<_, String>(LoopPorts::new(
            Model::new(move |_, _| {
                *record_turned.lock().unwrap() = Some(thread::current().id());
                Ok(answer("done"))
            }),
            tools(),
            DenyAll,
        ))
    });
    within(task.run("go", NullLoopSink))
        .await
        .unwrap()
        .result
        .unwrap();
    let built = built.lock().unwrap().expect("the builder ran");
    assert_ne!(built, thread::current().id());
    assert_eq!(Some(built), *turned.lock().unwrap());
}

/// Runs `script` through the synchronous `run_in` and through `LoopTask::run_in`, from the same
/// earlier conversation, and asserts the two hand back the same result, items and spend.
async fn run_in_agrees(
    script: &[Result<TurnOutcome, WireError>],
) -> harness_loop::RanIn<NullLoopSink> {
    let earlier = vec![
        Item::UserText {
            text: "earlier".to_owned(),
        },
        Item::AssistantText {
            text: "noted".to_owned(),
        },
    ];

    let mut model = Model::scripted(script.to_vec());
    let mut tools_port = tools();
    let mut items = earlier.clone();
    let mut spend = RunLedger::default();
    let expected = AgentLoop::new(&mut model, &mut tools_port, &mut ApproveAll, config()).run_in(
        &mut items,
        &mut spend,
        "go",
        &mut NullLoopSink,
    );

    let owned = script.to_vec();
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(Model::scripted(owned), tools(), ApproveAll))
    });
    let ran = within(task.run_in(earlier, "go", NullLoopSink))
        .await
        .unwrap();
    assert_eq!(ran.result, expected);
    assert_eq!(ran.items, items);
    assert_eq!(ran.spend, spend);
    ran
}

#[tokio::test]
async fn awaited_run_in_returns_items_and_spend() {
    let ran = run_in_agrees(&[Ok(asks("c1", "read")), Ok(answer("done"))]).await;
    assert_eq!(ran.result.unwrap().stop, LoopStop::Completed);
    assert_eq!(ran.spend.turns, 2);

    let ran = run_in_agrees(&[Ok(asks("c1", "read")), Err(WireError::protocol("broken"))]).await;
    assert!(ran.result.is_err(), "the wire error is the run's result");
    assert!(
        ran.items.len() > 2,
        "the conversation before the wire error came back: {:?}",
        ran.items
    );
    assert_eq!(ran.spend.usage.len(), 1, "the billed first turn came back");
}

#[tokio::test]
async fn dropping_the_awaited_run_cancels_the_ports_token() {
    let turns = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&turns);
    let (entered, in_turn) = mpsc::channel();
    let (observe, observed) = mpsc::channel();
    let task = LoopTask::spawn(config(), move |cancel: &LoopCancel| {
        let token = cancel.clone();
        Ok::<_, String>(LoopPorts::new(
            Model::new(move |_, _| {
                counted.fetch_add(1, Ordering::SeqCst);
                entered.send(()).unwrap();
                let saw = wait_until(|| token.is_cancelled());
                observe.send(saw).unwrap();
                Err(WireError::cancelled())
            }),
            tools(),
            DenyAll,
        ))
    });
    let running = task.run("go", NullLoopSink);
    in_turn
        .recv_timeout(WAIT)
        .expect("the model port entered its turn");
    drop(running);
    assert!(
        observed.recv_timeout(WAIT).unwrap(),
        "the model port never saw the cancel"
    );

    let outcome = within(task.run("again", NullLoopSink))
        .await
        .unwrap()
        .result
        .unwrap();
    assert!(
        matches!(outcome.stop, LoopStop::Cancelled { .. }),
        "{:?}",
        outcome.stop
    );
    assert_eq!(
        turns.load(Ordering::SeqCst),
        1,
        "a cancelled run took a turn"
    );
}

#[tokio::test]
async fn dropping_the_task_drops_the_ports_on_its_thread() {
    let built: Arc<Mutex<Option<ThreadId>>> = Arc::new(Mutex::new(None));
    let record = Arc::clone(&built);
    let (dropped, on_thread) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        *record.lock().unwrap() = Some(thread::current().id());
        let model = Model::scripted(vec![Ok(answer("done"))]).on_drop(move || {
            // Waits for the test to say the drop of the task has returned: a drop of the task
            // that waited for this one would hold both until the timeout.
            let unblocked = released.recv_timeout(WAIT).is_ok();
            dropped.send((thread::current().id(), unblocked)).unwrap();
        });
        Ok::<_, String>(LoopPorts::new(model, tools(), DenyAll))
    });
    within(task.run("go", NullLoopSink))
        .await
        .unwrap()
        .result
        .unwrap();
    drop(task);
    release.send(()).unwrap();
    let (thread_id, unblocked) = on_thread
        .recv_timeout(WAIT)
        .expect("the ports were dropped");
    assert!(unblocked, "dropping the task waited for the ports to drop");
    assert_eq!(Some(thread_id), *built.lock().unwrap());
    assert_ne!(thread_id, thread::current().id());
}

#[tokio::test]
async fn shutdown_waits_for_the_ports_to_drop() {
    let dropped = Arc::new(AtomicBool::new(false));
    let mark = Arc::clone(&dropped);
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        let model = Model::scripted(Vec::new()).on_drop(move || {
            thread::sleep(Duration::from_millis(100));
            mark.store(true, Ordering::SeqCst);
        });
        Ok::<_, String>(LoopPorts::new(model, tools(), DenyAll))
    });
    within(task.shutdown()).await.unwrap();
    assert!(dropped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn shutdown_reports_a_panicking_drop() {
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        let model =
            Model::scripted(Vec::new()).on_drop(|| panic!("the model port's drop exploded"));
        Ok::<_, String>(LoopPorts::new(model, tools(), DenyAll))
    });
    let error = within(task.shutdown()).await.unwrap_err();
    assert!(
        matches!(&error, LoopTaskError::Panicked(message) if message.contains("drop exploded")),
        "{error:?}"
    );
}

#[tokio::test]
async fn spawn_does_not_wait_for_the_builder() {
    let (release, released) = mpsc::channel::<()>();
    let waited = Arc::new(Mutex::new(None));
    let record = Arc::clone(&waited);
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        *record.lock().unwrap() = Some(released.recv_timeout(WAIT).is_ok());
        Ok::<_, String>(LoopPorts::new(
            Model::scripted(vec![Ok(answer("done"))]),
            tools(),
            DenyAll,
        ))
    });
    // Reached only once `spawn` has returned; a `spawn` that waited for the builder would have
    // let it time out first.
    release.send(()).unwrap();
    within(task.run("go", NullLoopSink))
        .await
        .unwrap()
        .result
        .unwrap();
    assert_eq!(
        *waited.lock().unwrap(),
        Some(true),
        "spawn returned only after the builder had finished"
    );
}

#[tokio::test]
async fn a_failing_builder_resolves_the_first_future() {
    let task = LoopTask::spawn(config(), |_: &LoopCancel| {
        Err::<LoopPorts, _>("no model is configured".to_owned())
    });
    let error = within(task.run("go", NullLoopSink)).await.unwrap_err();
    assert!(
        matches!(&error, LoopTaskError::Build(message) if message.contains("no model is configured")),
        "{error:?}"
    );
}

#[tokio::test]
async fn events_reach_the_sink_in_order() {
    let script = vec![Ok(asks("c1", "read")), Ok(answer("done"))];

    let mut model = Model::scripted(script.clone());
    let mut tools_port = tools();
    let mut expected = VecLoopSink::new();
    AgentLoop::new(&mut model, &mut tools_port, &mut ApproveAll, config())
        .run("go", &mut expected)
        .unwrap();

    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(Model::scripted(script), tools(), ApproveAll))
    });
    let sink = within(task.run("go", VecLoopSink::new()))
        .await
        .unwrap()
        .sink;
    assert!(!expected.events().is_empty());
    assert_eq!(sink.events(), expected.events());
}

#[tokio::test]
async fn commands_run_in_the_order_sent() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        let mut tools_port = Tools::new(record);
        // The first call is slow, so a second command that overtook it would be seen first.
        tools_port.slow = Some(("first".to_owned(), Duration::from_millis(100)));
        Ok::<_, String>(LoopPorts::new(
            Model::scripted(Vec::new()),
            tools_port,
            DenyAll,
        ))
    });
    let first = task.call(tool_call("first", "read"), NullLoopSink);
    let second = task.call(tool_call("second", "read"), NullLoopSink);
    let (second, _) = within(second).await.unwrap();
    let (first, _) = within(first).await.unwrap();
    assert!(!first.failed && !second.failed);
    assert_eq!(*seen.lock().unwrap(), ["first", "second"]);
}

#[tokio::test]
async fn awaited_approval_resumes() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Model::scripted(vec![Ok(asks("c1", "write")), Ok(answer("written"))]),
            Tools::new(record),
            Deferring,
        ))
    });
    let suspended = within(task.run("go", NullLoopSink))
        .await
        .unwrap()
        .result
        .unwrap();
    assert_eq!(
        suspended.stop,
        LoopStop::AwaitingApproval {
            checkpoint_id: "checkpoint-1".to_owned()
        }
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "the effect ran before approval"
    );
    let checkpoint = suspended.checkpoint.expect("a checkpoint to resume");

    let resumed =
        within(task.resume_approval(checkpoint, ApprovalDecision::Approved, NullLoopSink))
            .await
            .unwrap()
            .result
            .unwrap();
    assert_eq!(resumed.stop, LoopStop::Completed);
    assert_eq!(resumed.text, "written");
    assert_eq!(*seen.lock().unwrap(), ["c1"]);
}

#[tokio::test]
async fn awaited_resume_hands_back_its_sink_on_a_loop_error() {
    // The approved effect runs, then the next turn breaks on the wire: the resume fails with a
    // `LoopError`, after the events of the effect went into the sink.
    let script = || vec![Ok(asks("c1", "write")), Err(WireError::protocol("broken"))];

    let mut model = Model::scripted(script());
    let mut tools_port = tools();
    let suspended = AgentLoop::new(&mut model, &mut tools_port, &mut Deferring, config())
        .run("go", &mut NullLoopSink)
        .unwrap();
    let mut expected = VecLoopSink::new();
    let expected_result = AgentLoop::new(&mut model, &mut tools_port, &mut Deferring, config())
        .resume_approval(
            suspended.checkpoint.expect("a checkpoint to resume"),
            ApprovalDecision::Approved,
            &mut expected,
        );
    assert!(expected_result.is_err(), "the synchronous resume failed");
    assert!(!expected.events().is_empty());

    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Model::scripted(script()),
            tools(),
            Deferring,
        ))
    });
    let suspended = within(task.run("go", NullLoopSink))
        .await
        .unwrap()
        .result
        .unwrap();
    let ran = within(task.resume_approval(
        suspended.checkpoint.expect("a checkpoint to resume"),
        ApprovalDecision::Approved,
        VecLoopSink::new(),
    ))
    .await
    .expect("a loop error is the run's result, not the future's error");
    assert_eq!(ran.result, expected_result);
    assert_eq!(ran.sink.events(), expected.events());
}

#[tokio::test]
async fn a_run_dropped_before_it_started_takes_no_turn() {
    let turns = Arc::new(AtomicU64::new(0));
    let counted = Arc::clone(&turns);
    let (open, gated) = mpsc::channel::<()>();
    let (entered, in_first) = mpsc::channel();
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        let tools_port = tools();
        let model = Model::new(move |_, _| {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(answer("done"))
        });
        Ok::<_, String>(LoopPorts::new(model, tools_port, DenyAll))
    });
    // A command ahead of the run holds the thread until the test opens it.
    let first = task.call(tool_call("first", "read"), GateSink(Some((entered, gated))));
    let queued = task.run("go", NullLoopSink);
    in_first
        .recv_timeout(WAIT)
        .expect("the first command started");
    drop(queued);
    open.send(()).unwrap();
    within(first).await.unwrap();
    within(task.shutdown()).await.unwrap();
    assert_eq!(
        turns.load(Ordering::SeqCst),
        0,
        "the dropped run took a turn"
    );
}

#[tokio::test]
async fn a_future_dropped_while_queued_leaves_the_token_alone() {
    let (open, gated) = mpsc::channel::<()>();
    let (entered, in_first) = mpsc::channel();
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Model::scripted(vec![Ok(answer("still running"))]),
            tools(),
            DenyAll,
        ))
    });
    let first = task.call(tool_call("first", "read"), GateSink(Some((entered, gated))));
    let queued = task.call(tool_call("queued", "read"), NullLoopSink);
    in_first
        .recv_timeout(WAIT)
        .expect("the first command started");
    drop(queued);
    assert!(
        !task.cancel_handle().is_cancelled(),
        "a skipped command cancelled the task"
    );
    open.send(()).unwrap();
    within(first).await.unwrap();
    let outcome = within(task.run("go", NullLoopSink))
        .await
        .unwrap()
        .result
        .unwrap();
    assert_eq!(outcome.stop, LoopStop::Completed);
}

/// Holds its command on the task's thread at the first event, after saying so, until the test
/// sends.
struct GateSink(Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>);

impl LoopSink for GateSink {
    fn emit(&mut self, _: LoopEvent) {
        if let Some((entered, held)) = self.0.take() {
            entered.send(()).unwrap();
            held.recv_timeout(WAIT).unwrap();
        }
    }
}

#[tokio::test]
async fn awaited_call_meets_the_approver() {
    let call = tool_call("c1", "write");
    let mut model = Model::scripted(Vec::new());
    let mut tools_port = tools();
    let expected = AgentLoop::new(&mut model, &mut tools_port, &mut DenyAll, config())
        .call(&call, &mut NullLoopSink);
    // The denial is a failed outcome the model reads (invariant 9); it names no `Refusal`, which is
    // kept for the run's own rules.
    assert!(
        expected.failed && expected.output.to_string().contains("not approved"),
        "{expected:?}"
    );

    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Model::scripted(Vec::new()),
            Tools::new(record),
            DenyAll,
        ))
    });
    let (outcome, _) = within(task.call(call, NullLoopSink)).await.unwrap();
    assert_eq!(outcome, expected);
    assert!(
        seen.lock().unwrap().is_empty(),
        "a denied call reached the tool"
    );
}

#[tokio::test]
async fn a_panicking_port_resolves_the_future() {
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Model::new(|_, _| panic!("the model port exploded")),
            tools(),
            DenyAll,
        ))
    });
    let error = within(task.run("go", NullLoopSink)).await.unwrap_err();
    assert!(
        matches!(&error, LoopTaskError::Panicked(message) if message.contains("model port exploded")),
        "{error:?}"
    );
}

#[tokio::test]
async fn a_builder_panic_nobody_read_is_reported_at_shutdown() {
    let task = LoopTask::spawn(config(), |_: &LoopCancel| -> Result<LoopPorts, String> {
        panic!("the builder exploded")
    });
    let ended = within(task.shutdown()).await;
    assert!(
        matches!(&ended, Err(LoopTaskError::Panicked(message)) if message.contains("builder exploded")),
        "{ended:?}"
    );
}

#[tokio::test]
async fn a_panic_its_future_read_is_not_reported_again() {
    let task = LoopTask::spawn(config(), |_: &LoopCancel| -> Result<LoopPorts, String> {
        panic!("the builder exploded")
    });
    let first = within(task.run("go", NullLoopSink)).await.unwrap_err();
    assert!(matches!(&first, LoopTaskError::Panicked(_)), "{first:?}");
    let later = within(task.call(tool_call("c1", "read"), NullLoopSink)).await;
    assert_eq!(later.map(|(outcome, _)| outcome), Err(first));
    within(task.shutdown()).await.unwrap();
}

/// A sink whose drop panics on the task's thread.
struct ExplodesOnTaskThread;

impl LoopSink for ExplodesOnTaskThread {
    fn emit(&mut self, _: LoopEvent) {}
}

impl Drop for ExplodesOnTaskThread {
    fn drop(&mut self) {
        assert!(
            thread::current().name() != Some("harness-loop-task"),
            "a skipped command's sink exploded"
        );
    }
}

#[tokio::test]
async fn an_unheard_builder_panic_is_the_first_one_reported() {
    let (release, released) = mpsc::channel::<()>();
    let task = LoopTask::spawn(
        config(),
        move |_: &LoopCancel| -> Result<LoopPorts, String> {
            let _ = released.recv_timeout(WAIT);
            panic!("the builder exploded")
        },
    );
    // Dropped while the builder still holds the thread: skipped, and its sink's drop panics after
    // the builder's panic.
    drop(task.run("go", ExplodesOnTaskThread));
    release.send(()).unwrap();
    let ended = within(task.shutdown()).await;
    assert!(
        matches!(&ended, Err(LoopTaskError::Panicked(message)) if message.contains("builder exploded")),
        "{ended:?}"
    );
}

/// Wakes the thread that is waiting in [`block_on`].
struct Unpark(thread::Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// The smallest executor there is: poll, park until woken, poll again. No runtime of any kind.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut context = Context::from_waker(&waker);
    let deadline = Instant::now() + WAIT;
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        let now = Instant::now();
        assert!(
            now < deadline,
            "the loop task did not wake its future within 5 s"
        );
        thread::park_timeout(deadline - now);
    }
}

#[test]
fn the_future_needs_no_tokio() {
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Model::scripted(vec![Ok(answer("no runtime needed"))]),
            tools(),
            DenyAll,
        ))
    });
    let outcome = block_on(task.run("go", NullLoopSink))
        .unwrap()
        .result
        .unwrap();
    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.text, "no runtime needed");
    block_on(task.shutdown()).unwrap();
}
