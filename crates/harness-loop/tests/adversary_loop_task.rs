//! Adversarial cases against `LoopTask` (story `loop-task-awaits-a-run`).
//!
//! Each test bounds every wait at [`WAIT`].

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, ThreadId};
use std::time::{Duration, Instant};

use b10x_harness_loop as harness_loop;
use harness_loop::{
    AgentLoop, ApproveAll, EnvironmentError, HookDecision, HookPort, LoopCancel, LoopConfig,
    LoopEvent, LoopPorts, LoopSink, LoopTask, LoopTaskError, NullLoopSink, TurnEnvironment,
    TurnEnvironmentProvider, TurnEnvironmentRequest,
};
use harness_wire::{
    Approval, CallId, Envelope, Item, ModelPort, Risk, StopReason, StreamSink, ToolCall, ToolName,
    ToolOutcome, ToolPort, ToolSpec, TurnOutcome, TurnRequest, Usage, WireError, WireId,
};
use serde_json::json;

const WAIT: Duration = Duration::from_secs(5);

async fn within<F: Future>(future: F) -> F::Output {
    tokio::time::timeout(WAIT, future)
        .await
        .expect("the loop task answered within 5 s")
}

fn config() -> LoopConfig {
    LoopConfig::new("fixture", "Answer briefly.")
}

fn usage() -> Usage {
    Usage {
        model: "fixture".to_owned(),
        input_tokens: 7,
        output_tokens: 3,
        cached_input_tokens: 0,
        cache_creation_input_tokens: None,
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
        usage: Some(usage()),
    }
}

fn answer(text: &str) -> TurnOutcome {
    TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::AssistantText {
            text: text.to_owned(),
        }],
        usage: Some(usage()),
    }
}

struct Scripted {
    wire: WireId,
    script: VecDeque<Result<TurnOutcome, WireError>>,
}

impl Scripted {
    fn new(script: Vec<Result<TurnOutcome, WireError>>) -> Self {
        Self {
            wire: WireId::new("fixture").unwrap(),
            script: script.into(),
        }
    }
}

impl ModelPort for Scripted {
    fn wire(&self) -> &WireId {
        &self.wire
    }

    fn turn(&mut self, _: &TurnRequest, _: &mut dyn StreamSink) -> Result<TurnOutcome, WireError> {
        self.script
            .pop_front()
            .unwrap_or_else(|| Err(WireError::protocol("the script is exhausted")))
    }
}

/// `read` (low risk) and `write` (medium risk). Records each call id; the call named `gate`
/// signals `entered` and then waits for `release` before it returns.
struct Tools {
    specs: Vec<ToolSpec>,
    seen: Arc<Mutex<Vec<String>>>,
    gate: Option<(String, mpsc::Sender<()>, mpsc::Receiver<()>)>,
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
            gate: None,
        }
    }
}

impl ToolPort for Tools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        if let Some((id, entered, release)) = &self.gate
            && call.call_id.as_str() == id
        {
            entered.send(()).unwrap();
            release.recv_timeout(WAIT).unwrap();
        }
        self.seen
            .lock()
            .unwrap()
            .push(call.call_id.as_str().to_owned());
        ToolOutcome::ok(json!("effect"))
    }
}

/// A future abandoned before its command reached the task's thread is cancelled: the caller has
/// walked away (a `timeout`, a `select!` arm that lost) and will never read the outcome. A
/// `call` ignores the task's token, so its effect still runs, unseen and unreported: a caller
/// that times out and retries performs the write twice.
#[tokio::test]
async fn a_call_dropped_before_it_started_performs_no_effect() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    let (entered, in_first) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        let mut tools = Tools::new(record);
        tools.gate = Some(("first".to_owned(), entered, released));
        Ok::<_, String>(LoopPorts::new(Scripted::new(Vec::new()), tools, ApproveAll))
    });
    let first = task.call(tool_call("first", "read"), NullLoopSink);
    let abandoned = task.call(tool_call("abandoned", "write"), NullLoopSink);
    in_first
        .recv_timeout(WAIT)
        .expect("the first call reached the tool");
    // The second call is still queued behind the first: it has not started.
    drop(abandoned);
    release.send(()).unwrap();
    within(first).await.unwrap();
    within(task.shutdown()).await.unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        ["first"],
        "a call whose future was dropped before it started still ran its effect"
    );
}

/// Records how many events it held, and on which thread, when it is dropped.
struct ProbeSink {
    events: Vec<LoopEvent>,
    dropped: mpsc::Sender<(usize, ThreadId)>,
}

impl LoopSink for ProbeSink {
    fn emit(&mut self, event: LoopEvent) {
        self.events.push(event);
    }
}

impl Drop for ProbeSink {
    fn drop(&mut self) {
        let _ = self
            .dropped
            .send((self.events.len(), thread::current().id()));
    }
}

/// D3: each command "takes a sink by value and returns it with the result, so a `VecLoopSink`
/// comes back holding the run's events". A synchronous caller of `run` still holds its sink, and
/// the events the run emitted before a wire failure, when `run` returns `Err`. Through
/// `LoopTask::run` the sink, with those events, is dropped on the task's thread.
#[tokio::test]
async fn a_run_that_fails_on_the_wire_hands_back_its_sink() {
    let script = || vec![Ok(asks("c1", "read")), Err(WireError::protocol("broken"))];

    let (sync_dropped, _sync_seen) = mpsc::channel();
    let mut sync_sink = ProbeSink {
        events: Vec::new(),
        dropped: sync_dropped,
    };
    let mut model = Scripted::new(script());
    let mut tools = Tools::new(Arc::new(Mutex::new(Vec::new())));
    let sync_result =
        AgentLoop::new(&mut model, &mut tools, &mut ApproveAll, config()).run("go", &mut sync_sink);
    assert!(
        sync_result.is_err(),
        "the synchronous run failed on the wire"
    );
    let held = sync_sink.events.len();
    assert!(held > 0, "the synchronous caller still reads the events");

    let (dropped, on_drop) = mpsc::channel();
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Scripted::new(script()),
            Tools::new(Arc::new(Mutex::new(Vec::new()))),
            ApproveAll,
        ))
    });
    let result = within(task.run(
        "go",
        ProbeSink {
            events: Vec::new(),
            dropped,
        },
    ))
    .await;
    assert!(
        matches!(&result, Ok(ran) if ran.result.is_err()),
        "the run failed on the wire"
    );
    within(task.shutdown()).await.unwrap();
    let lost = on_drop.try_recv().ok();
    assert!(
        lost.is_none(),
        "the sink was dropped on the task's thread holding {:?} events (the synchronous caller \
         kept {held}); the caller of LoopTask::run cannot read them",
        lost.map(|(events, _)| events)
    );
}

/// A sink that panics when dropped on the task's thread.
struct PanicsOnTaskThread;

impl LoopSink for PanicsOnTaskThread {
    fn emit(&mut self, _: LoopEvent) {}
}

impl Drop for PanicsOnTaskThread {
    fn drop(&mut self) {
        assert!(
            thread::current().name() != Some("harness-loop-task"),
            "the sink's drop exploded"
        );
    }
}

/// The brief: panics on the task thread are caught "so the thread reports instead of dying
/// silently". An answer whose future was already dropped is dropped on the task's thread, outside
/// every `catch_unwind`: a panic there ends the thread, and `shutdown` can only say `Gone`.
#[tokio::test]
async fn a_panic_dropping_an_unread_answer_does_not_kill_the_task() {
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Scripted::new(vec![Ok(answer("done"))]),
            Tools::new(Arc::new(Mutex::new(Vec::new()))),
            ApproveAll,
        ))
    });
    drop(task.run("go", PanicsOnTaskThread));
    let ended = within(task.shutdown()).await;
    assert!(
        !matches!(ended, Err(LoopTaskError::Gone)),
        "the task's thread died silently: shutdown resolved to {ended:?}"
    );
}

/// Blocks every call before it runs.
struct BlockAll;

impl HookPort for BlockAll {
    fn before_call(&mut self, _: &ToolCall, _: &ToolSpec) -> HookDecision {
        HookDecision::block("the operator's hook blocks every call")
    }
}

/// `LoopPorts::with_hooks` reaches the loop: no implementor test attaches hooks, so a
/// `LoopPorts::agent` that dropped them would stay green.
#[tokio::test]
async fn hooks_on_the_ports_are_consulted() {
    let call = tool_call("c1", "read");
    let mut model = Scripted::new(Vec::new());
    let mut tools = Tools::new(Arc::new(Mutex::new(Vec::new())));
    let mut hooks = BlockAll;
    let expected = AgentLoop::new(&mut model, &mut tools, &mut ApproveAll, config())
        .with_hooks(&mut hooks)
        .call(&call, &mut NullLoopSink);
    assert!(expected.failed, "the synchronous hook blocked the call");

    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(
            LoopPorts::new(Scripted::new(Vec::new()), Tools::new(record), ApproveAll)
                .with_hooks(BlockAll),
        )
    });
    let (outcome, _) = within(task.call(call, NullLoopSink)).await.unwrap();
    assert_eq!(outcome, expected);
    assert!(
        seen.lock().unwrap().is_empty(),
        "a blocked call reached the tool"
    );
}

/// Refuses every refresh.
struct Unavailable;

impl TurnEnvironmentProvider for Unavailable {
    fn refresh(&mut self, _: TurnEnvironmentRequest) -> Result<TurnEnvironment, EnvironmentError> {
        Err(EnvironmentError::Unavailable("no actor view".to_owned()))
    }
}

/// `LoopPorts::with_environment` reaches the loop: no implementor test attaches one.
#[tokio::test]
async fn an_environment_on_the_ports_is_consulted() {
    let mut model = Scripted::new(vec![Ok(answer("done"))]);
    let mut tools = Tools::new(Arc::new(Mutex::new(Vec::new())));
    let mut environment = Unavailable;
    let expected = AgentLoop::new(&mut model, &mut tools, &mut ApproveAll, config())
        .with_environment(&mut environment)
        .run("go", &mut NullLoopSink);
    assert!(expected.is_err(), "the synchronous run failed closed");

    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(
            LoopPorts::new(
                Scripted::new(vec![Ok(answer("done"))]),
                Tools::new(Arc::new(Mutex::new(Vec::new()))),
                ApproveAll,
            )
            .with_environment(Unavailable),
        )
    });
    let result = within(task.run("go", NullLoopSink)).await;
    assert_eq!(result.map(|ran| ran.result), Ok(expected));
}

struct CountWakes(AtomicU64);

impl Wake for CountWakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// The suite's executors cannot see a lost wake-up: `block_on` re-polls when `park_timeout` runs
/// out, and `tokio::time::timeout` polls the inner future first when its timer fires, so a
/// `LoopTaskFuture` that never woke anyone still resolves, five seconds late, and every test
/// stays green. This executor polls only after a wake, and only the waker of the last poll may
/// be the one woken.
#[test]
fn the_answer_wakes_the_waker_of_the_last_poll() {
    let task = LoopTask::spawn(config(), move |_: &LoopCancel| {
        Ok::<_, String>(LoopPorts::new(
            Scripted::new(vec![Ok(answer("done"))]),
            Tools::new(Arc::new(Mutex::new(Vec::new()))),
            ApproveAll,
        ))
    });
    let (open, gated) = mpsc::channel::<()>();
    // A command ahead of the run keeps the thread busy until both polls have happened.
    let gate = task.call(tool_call("gate", "read"), GateSink(Some(gated)));
    let mut future = task.run("go", NullLoopSink);

    let first = Arc::new(CountWakes(AtomicU64::new(0)));
    let last = Arc::new(CountWakes(AtomicU64::new(0)));
    let first_waker = Waker::from(Arc::clone(&first));
    let last_waker = Waker::from(Arc::clone(&last));
    assert!(
        Pin::new(&mut future)
            .poll(&mut Context::from_waker(&first_waker))
            .is_pending()
    );
    assert!(
        Pin::new(&mut future)
            .poll(&mut Context::from_waker(&last_waker))
            .is_pending()
    );
    open.send(()).unwrap();

    let deadline = Instant::now() + WAIT;
    while last.0.load(Ordering::SeqCst) == 0 {
        assert!(
            Instant::now() < deadline,
            "the answer arrived without waking the last poll's waker"
        );
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        first.0.load(Ordering::SeqCst),
        0,
        "a replaced waker was woken"
    );
    let Poll::Ready(answer) = Pin::new(&mut future).poll(&mut Context::from_waker(&last_waker))
    else {
        panic!("woken, but not ready");
    };
    assert_eq!(answer.unwrap().result.unwrap().text, "done");
    drop(gate);
}

/// Holds its command on the task's thread until the test sends.
struct GateSink(Option<mpsc::Receiver<()>>);

impl LoopSink for GateSink {
    fn emit(&mut self, _: LoopEvent) {
        if let Some(held) = self.0.take() {
            held.recv_timeout(WAIT).unwrap();
        }
    }
}
