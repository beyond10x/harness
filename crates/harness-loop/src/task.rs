//! [`LoopTask`]: the loop awaited from an async program.

use std::any::Any;
use std::fmt;
use std::future::Future;
use std::panic::{self, AssertUnwindSafe};
use std::pin::Pin;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread;

use harness_wire::{Item, ModelPort, ToolCall, ToolOutcome, ToolPort};

use crate::{
    AgentLoop, ApprovalCheckpoint, ApprovalDecision, ApprovalPort, HookPort, LoopCancel,
    LoopConfig, LoopError, LoopOutcome, LoopSink, RunLedger, TurnEnvironmentProvider,
};

/// The ports a [`LoopTask`] holds on its thread, built there by the closure given to
/// [`LoopTask::spawn`].
///
/// Owned rather than borrowed, because they outlive every single command; not required to be
/// [`Send`], because they never leave the thread that built them.
pub struct LoopPorts {
    model: Box<dyn ModelPort>,
    tools: Box<dyn ToolPort>,
    approvals: Box<dyn ApprovalPort>,
    hooks: Option<Box<dyn HookPort>>,
    environment: Option<Box<dyn TurnEnvironmentProvider>>,
}

impl LoopPorts {
    /// The three ports every run needs.
    pub fn new(
        model: impl ModelPort + 'static,
        tools: impl ToolPort + 'static,
        approvals: impl ApprovalPort + 'static,
    ) -> Self {
        Self::boxed(Box::new(model), Box::new(tools), Box::new(approvals))
    }

    /// [`new`](Self::new), for ports the caller already holds boxed.
    pub fn boxed(
        model: Box<dyn ModelPort>,
        tools: Box<dyn ToolPort>,
        approvals: Box<dyn ApprovalPort>,
    ) -> Self {
        Self {
            model,
            tools,
            approvals,
            hooks: None,
            environment: None,
        }
    }

    /// Attaches the operator's hooks to every loop the task builds ([`AgentLoop::with_hooks`]).
    #[must_use]
    pub fn with_hooks(mut self, hooks: impl HookPort + 'static) -> Self {
        self.hooks = Some(Box::new(hooks));
        self
    }

    /// Attaches a turn environment to every loop the task builds
    /// ([`AgentLoop::with_environment`]).
    #[must_use]
    pub fn with_environment(mut self, environment: impl TurnEnvironmentProvider + 'static) -> Self {
        self.environment = Some(Box::new(environment));
        self
    }

    /// The loop one command runs, over these ports and the task's token.
    fn agent(&mut self, config: &LoopConfig, cancel: &LoopCancel) -> AgentLoop<'_> {
        let mut agent = AgentLoop::new(
            self.model.as_mut(),
            self.tools.as_mut(),
            self.approvals.as_mut(),
            config.clone(),
        )
        .with_cancel(cancel.clone());
        if let Some(hooks) = self.hooks.as_deref_mut() {
            agent = agent.with_hooks(hooks);
        }
        if let Some(environment) = self.environment.as_deref_mut() {
            agent = agent.with_environment(environment);
        }
        agent
    }
}

impl fmt::Debug for LoopPorts {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoopPorts")
            .field("wire", self.model.wire())
            .field("tools", &self.tools.specs().len())
            .field("hooks", &self.hooks.is_some())
            .field("environment", &self.environment.is_some())
            .finish_non_exhaustive()
    }
}

/// Why a [`LoopTask`] future resolved without the command's own answer.
///
/// The loop's own [`LoopError`] is never one of these: it is part of the command's answer
/// ([`Ran::result`], [`RanIn::result`]), beside the sink it emitted into, as a synchronous caller
/// holds both.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoopTaskError {
    /// The builder given to [`LoopTask::spawn`] returned an error; its `Display`, verbatim.
    ///
    /// Every command on the task resolves to it, the first one included.
    #[error("the loop task could not build its ports: {0}")]
    Build(String),
    /// Something on the task's thread panicked: the builder, a port during a command, a port's
    /// drop, or the drop of an answer nobody was waiting for. The panic's message, when it had one.
    #[error("the loop task's thread panicked: {0}")]
    Panicked(String),
    /// The task's thread is gone without answering; it could not be started, or it ended before
    /// this command reached it.
    #[error("the loop task's thread is gone")]
    Gone,
}

/// What [`LoopTask::run`] and [`LoopTask::resume_approval`] hand back: the run's own result and
/// the sink, on every exit path, as a synchronous caller holds both.
#[derive(Debug)]
pub struct Ran<S> {
    /// The run's own result. A wire failure is here, beside the sink holding what it emitted.
    pub result: Result<LoopOutcome, LoopError>,
    /// The sink, holding whatever it kept.
    pub sink: S,
}

/// What [`LoopTask::run_in`] hands back: everything the synchronous
/// [`AgentLoop::run_in`] writes back, on every exit path.
#[derive(Debug)]
pub struct RanIn<S> {
    /// The run's own result. A wire failure is here, beside the items and spend it left.
    pub result: Result<LoopOutcome, LoopError>,
    /// The conversation as the run left it.
    pub items: Vec<Item>,
    /// What this run spent.
    pub spend: RunLedger,
    /// The sink, holding whatever it kept.
    pub sink: S,
}

/// An owned handle on a loop whose ports live on a thread of their own.
///
/// # Why a thread of its own
///
/// [`AgentLoop`] borrows its ports and blocks the thread that calls it, and some ports are not
/// safe on an async runtime's thread at all: one that owns a private tokio runtime calls
/// `Runtime::block_on`, which panics inside another runtime's context, and dropping such a runtime
/// there panics too. So a `LoopTask` starts one `std::thread` with no runtime context, builds the
/// ports on it from a closure the caller hands over, and keeps them there until they drop. The
/// caller's runtime only ever holds futures; the ports never cross threads, so they need not be
/// [`Send`].
///
/// # Runtime-neutral futures
///
/// [`run`](LoopTask::run), [`run_in`](LoopTask::run_in),
/// [`resume_approval`](LoopTask::resume_approval) and [`call`](LoopTask::call) send one command
/// to the thread **when they are called**, not when their future is first polled, and the thread
/// runs the commands one at a time in the order they were sent. Each future is completed by the
/// thread through the [`Waker`] it was last polled with, so it can be awaited on tokio, on any
/// other executor, or on a thread that parks. This crate depends on no runtime.
///
/// Each command builds a fresh [`AgentLoop`] over the held ports, exactly as a synchronous caller
/// builds one per run, with the task's cancel token and with the hooks and environment when the
/// [`LoopPorts`] carry them.
///
/// # Cancellation
///
/// A task has **one** [`LoopCancel`]. It is created by the task, handed to the builder (so a port
/// that checks its own token, such as a model client, holds the same one), installed on every
/// loop the task builds, and returned by [`LoopTask::cancel_handle`].
///
/// Dropping a future before it resolved depends on whether its command has started:
///
/// - **Still queued:** the command is skipped. It never reaches the ports, so a `call` abandoned
///   by a timeout performs no effect a retry would repeat, and the token is left alone, because
///   the command running ahead of it is somebody else's.
/// - **Running:** the token is cancelled, and the run ends as a cancelled synchronous run does,
///   with [`LoopStop::Cancelled`](crate::LoopStop::Cancelled). The token has no reset, so every
///   later run on the same task ends the same way; a caller that wants to run again starts
///   another task.
///
/// # Ending a task
///
/// Dropping the `LoopTask` closes its command channel and returns at once. The thread finishes
/// the commands already sent, drops the ports and exits by itself. [`LoopTask::shutdown`] is the
/// awaitable form: it resolves once the ports have dropped.
///
/// # Panics are answers
///
/// The thread catches a panic in the builder, in a command and in the drop of the ports, and
/// resolves the waiting future to [`LoopTaskError::Panicked`] with the panic's message. After a
/// panic in a command the ports are dropped and every later command resolves to that same error:
/// a port that panicked half-way through a turn is not one a next run should trust.
///
/// Every panic reaches somebody once. One that no future read is kept and the thread goes on:
/// a command's or the builder's panic whose answer nobody read, a port's drop after a command
/// panicked, or the drop of an answer or a skipped command's sink whose future is gone.
/// [`shutdown`](LoopTask::shutdown) resolves to the first of them. A later command answering a
/// panic somebody already read is not a new one.
///
/// # Example
///
/// ```
/// use b10x_harness_loop::{DenyAll, LoopCancel, LoopConfig, LoopPorts, LoopStop, LoopTask, VecLoopSink};
/// # use harness_wire::{
/// #     Item, ModelPort, StopReason, StreamSink, ToolPort, ToolSpec, TurnOutcome, TurnRequest,
/// #     WireError, WireId,
/// # };
/// # struct Model(WireId);
/// # impl ModelPort for Model {
/// #     fn wire(&self) -> &WireId { &self.0 }
/// #     fn turn(&mut self, _: &TurnRequest, _: &mut dyn StreamSink) -> Result<TurnOutcome, WireError> {
/// #         Ok(TurnOutcome {
/// #             stop_reason: StopReason::EndTurn,
/// #             items: vec![Item::AssistantText { text: "hello".to_owned() }],
/// #             usage: None,
/// #         })
/// #     }
/// # }
/// # struct NoTools;
/// # impl ToolPort for NoTools {
/// #     fn specs(&self) -> &[ToolSpec] { &[] }
/// #     fn call(&mut self, _: &harness_wire::ToolCall) -> harness_wire::ToolOutcome { unreachable!() }
/// # }
/// # tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
/// let task = LoopTask::spawn(LoopConfig::new("model", "Answer briefly."), |_cancel: &LoopCancel| {
///     // Runs on the task's own thread: the place to open clients and private runtimes.
///     Ok::<_, String>(LoopPorts::new(Model(WireId::new("fixture").unwrap()), NoTools, DenyAll))
/// });
/// let ran = task.run("Say hello.", VecLoopSink::new()).await?;
/// let outcome = ran.result?;
/// assert_eq!(outcome.stop, LoopStop::Completed);
/// assert_eq!(outcome.text, "hello");
/// assert!(!ran.sink.events().is_empty());
/// task.shutdown().await?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # }).unwrap();
/// ```
pub struct LoopTask {
    outbox: mpsc::Sender<Message>,
    cancel: LoopCancel,
}

impl fmt::Debug for LoopTask {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoopTask")
            .field("cancelled", &self.cancel.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl LoopTask {
    /// Starts the task's thread and returns at once; `build` runs on that thread.
    ///
    /// `build` receives the task's cancel token. A builder that fails does not fail `spawn`: its
    /// error resolves the first command's future, and every later one, as
    /// [`LoopTaskError::Build`] carrying the error's `Display`.
    pub fn spawn<F, E>(config: LoopConfig, build: F) -> Self
    where
        F: FnOnce(&LoopCancel) -> Result<LoopPorts, E> + Send + 'static,
        E: fmt::Display,
    {
        let (outbox, inbox) = mpsc::channel();
        let cancel = LoopCancel::new();
        let token = cancel.clone();
        // A thread that cannot be started drops `inbox` with the closure: every command then
        // finds the channel closed and resolves to `Gone`.
        let _started = thread::Builder::new()
            .name("harness-loop-task".to_owned())
            .spawn(move || serve(config, token, build, &inbox));
        Self { outbox, cancel }
    }

    /// The task's one cancel token: the one the builder received and every run observes.
    pub fn cancel_handle(&self) -> LoopCancel {
        self.cancel.clone()
    }

    /// [`AgentLoop::run`] on the task's thread; resolves to the run's result and the sink.
    pub fn run<S>(&self, input: impl Into<String>, sink: S) -> LoopTaskFuture<Ran<S>>
    where
        S: LoopSink + Send + 'static,
    {
        let input = input.into();
        self.send(move |agent| {
            let mut sink = sink;
            let result = agent.run(input, &mut sink);
            Ran { result, sink }
        })
    }

    /// [`AgentLoop::run_in`] on the task's thread, continuing the conversation `items`.
    ///
    /// The run's own [`LoopError`] is in [`RanIn::result`] rather than in the future's error, so
    /// that the conversation and the spend come back whichever way the run ended, as the
    /// synchronous `run_in` writes them back.
    pub fn run_in<S>(
        &self,
        items: Vec<Item>,
        input: impl Into<String>,
        sink: S,
    ) -> LoopTaskFuture<RanIn<S>>
    where
        S: LoopSink + Send + 'static,
    {
        let input = input.into();
        self.send(move |agent| {
            let (mut items, mut sink) = (items, sink);
            let mut spend = RunLedger::default();
            let result = agent.run_in(&mut items, &mut spend, input, &mut sink);
            RanIn {
                result,
                items,
                spend,
                sink,
            }
        })
    }

    /// [`AgentLoop::resume_approval`] on the task's thread, over the ports the task holds;
    /// resolves to the run's result and the sink.
    pub fn resume_approval<S>(
        &self,
        checkpoint: ApprovalCheckpoint,
        decision: ApprovalDecision,
        sink: S,
    ) -> LoopTaskFuture<Ran<S>>
    where
        S: LoopSink + Send + 'static,
    {
        self.send(move |agent| {
            let mut sink = sink;
            let result = agent.resume_approval(checkpoint, decision, &mut sink);
            Ran { result, sink }
        })
    }

    /// [`AgentLoop::call`] on the task's thread: one call through the gate a model's call meets.
    pub fn call<S>(&self, call: ToolCall, sink: S) -> LoopTaskFuture<(ToolOutcome, S)>
    where
        S: LoopSink + Send + 'static,
    {
        self.send(move |agent| {
            let mut sink = sink;
            let outcome = agent.call(&call, &mut sink);
            (outcome, sink)
        })
    }

    /// Ends the task after the commands already sent; resolves once the ports have dropped.
    ///
    /// Resolves to [`LoopTaskError::Panicked`] when a port's drop panicked, or when an earlier
    /// panic had nobody to report to (see [Panics are answers](LoopTask#panics-are-answers)).
    /// Dropping this future does not cancel anything: the task is ending either way.
    pub fn shutdown(self) -> LoopTaskFuture<()> {
        let (reply, future) = pair(None);
        // A closed channel drops the reply with the message, which resolves the future to `Gone`.
        let _sent = self.outbox.send(Message::Shutdown(reply));
        future
    }

    /// Sends one command and returns the future its answer completes.
    fn send<T, W>(&self, work: W) -> LoopTaskFuture<T>
    where
        T: Send + 'static,
        W: FnOnce(&mut AgentLoop<'_>) -> T + Send + 'static,
    {
        let (reply, future) = pair(Some(self.cancel.clone()));
        let job: Job = Box::new(move |station: &mut Station| {
            // A command whose future is gone is skipped: `work`, and the sink in it, drop here,
            // inside the thread's guard.
            if !reply.start() {
                return;
            }
            let (answer, dropped) = station.execute(work);
            let panicked = match &answer {
                Err(error @ LoopTaskError::Panicked(_)) => Some(error.clone()),
                _ => None,
            };
            let unread = reply.send(answer);
            if let Some(error) = panicked {
                station.answered_panic(error, unread.is_none());
            }
            // After the command's own panic: it happened first.
            if let Some(error) = dropped {
                station.keep(error);
            }
            // Last, because an unread sink's drop may panic; `serve` catches and keeps that.
            drop(unread);
        });
        // A closed channel drops the job, and its reply with it, which resolves the future to
        // `Gone`.
        let _sent = self.outbox.send(Message::Job(job));
        future
    }
}

/// The answer to one [`LoopTask`] command, completed by the task's thread.
///
/// Dropping it before it resolved skips its command when the command has not started, and
/// cancels the task's token when it has (see [`LoopTask`'s documentation](LoopTask#cancellation)).
#[must_use = "a dropped LoopTask future skips its command or cancels the task's token"]
pub struct LoopTaskFuture<T> {
    slot: Arc<Mutex<Slot<T>>>,
    /// The token a drop during the command cancels; [`None`] for [`LoopTask::shutdown`].
    cancel: Option<LoopCancel>,
    done: bool,
}

impl<T> fmt::Debug for LoopTaskFuture<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoopTaskFuture")
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}

impl<T> Future for LoopTaskFuture<T> {
    type Output = Result<T, LoopTaskError>;

    /// Polled again after it resolved, it resolves to [`LoopTaskError::Gone`].
    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(Err(LoopTaskError::Gone));
        }
        let mut slot = lock(&this.slot);
        if let Some(answer) = slot.answer.take() {
            this.done = true;
            return Poll::Ready(answer);
        }
        match &mut slot.waker {
            Some(waker) if waker.will_wake(context.waker()) => {}
            waker => *waker = Some(context.waker().clone()),
        }
        Poll::Pending
    }
}

impl<T> Drop for LoopTaskFuture<T> {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        // Decided under the lock the thread takes to start the command, so a command is either
        // skipped or running, never both.
        let started = {
            let mut slot = lock(&self.slot);
            if slot.answer.is_some() {
                // Answered and never read: nothing is left to stop.
                return;
            }
            slot.abandoned = true;
            slot.started
        };
        if started && let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
    }
}

struct Slot<T> {
    answer: Option<Result<T, LoopTaskError>>,
    waker: Option<Waker>,
    /// Set by the thread when the command begins.
    started: bool,
    /// Set by the future when it is dropped unanswered; nobody will read an answer.
    abandoned: bool,
}

/// The thread's half of one future. Dropped without an answer, it answers [`LoopTaskError::Gone`],
/// so a command that never ran can never leave its future pending.
struct Reply<T> {
    slot: Option<Arc<Mutex<Slot<T>>>>,
}

impl<T> Reply<T> {
    /// Marks the command started; `false`, and nothing marked, when its future is already gone.
    fn start(&self) -> bool {
        let Some(slot) = &self.slot else {
            return false;
        };
        let mut slot = lock(slot);
        if slot.abandoned {
            return false;
        }
        slot.started = true;
        true
    }

    /// Hands `answer` to the future; returns it instead when the future is gone, for the caller to
    /// read what nobody else will and then drop it.
    fn send(mut self, answer: Result<T, LoopTaskError>) -> Option<Result<T, LoopTaskError>> {
        let slot = self.slot.take()?;
        fill(&slot, answer)
    }
}

impl<T> Drop for Reply<T> {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            // `Gone` carries nothing to report and nothing whose drop could panic.
            let _unread = fill(&slot, Err(LoopTaskError::Gone));
        }
    }
}

fn pair<T>(cancel: Option<LoopCancel>) -> (Reply<T>, LoopTaskFuture<T>) {
    let slot = Arc::new(Mutex::new(Slot {
        answer: None,
        waker: None,
        started: false,
        abandoned: false,
    }));
    (
        Reply {
            slot: Some(Arc::clone(&slot)),
        },
        LoopTaskFuture {
            slot,
            cancel,
            done: false,
        },
    )
}

/// Hands `answer` to the future, or returns it when the future is gone.
///
/// The unread answer goes back to the task's thread, which drops it inside the guard `serve` puts
/// around each command: it holds the caller's sink, and a sink whose drop panics must not end the
/// thread. It is returned rather than dropped here so that a panic it carries is not lost.
fn fill<T>(
    slot: &Mutex<Slot<T>>,
    answer: Result<T, LoopTaskError>,
) -> Option<Result<T, LoopTaskError>> {
    let waker = {
        let mut slot = lock(slot);
        if slot.abandoned {
            return Some(answer);
        }
        slot.answer = Some(answer);
        slot.waker.take()
    };
    // Woken outside the lock, so an executor that polls inside `wake` does not deadlock on it.
    if let Some(waker) = waker {
        waker.wake();
    }
    None
}

/// A slot is only ever written whole, so one whose lock was poisoned still holds a whole value.
fn lock<T>(slot: &Mutex<T>) -> MutexGuard<'_, T> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

type Job = Box<dyn FnOnce(&mut Station) + Send>;

enum Message {
    Job(Job),
    Shutdown(Reply<()>),
}

/// What the task's thread holds between commands.
struct Station {
    config: LoopConfig,
    cancel: LoopCancel,
    held: Held,
    /// The first panic no future carried: a port's drop after a command panicked, a command's or
    /// the builder's panic whose answer nobody read, or the drop of an answer or a skipped command
    /// nobody was waiting for. Kept for `shutdown`.
    unreported: Option<LoopTaskError>,
    /// Whether the panic in [`Held::Failed`] has reached somebody: a future that read it, or
    /// `unreported`. Later commands answer clones of it, which are not new panics.
    heard: bool,
    /// Whether that panic is the builder's, which came before anything else on this thread.
    builder_panicked: bool,
}

enum Held {
    Ready(LoopPorts),
    /// Every command answers this: the builder's failure, or the panic that retired the ports.
    Failed(LoopTaskError),
}

impl Station {
    /// Runs one command; on a panic, also the panic of dropping the ports it retired, which no
    /// answer carries.
    fn execute<T>(
        &mut self,
        work: impl FnOnce(&mut AgentLoop<'_>) -> T,
    ) -> (Result<T, LoopTaskError>, Option<LoopTaskError>) {
        let ports = match &mut self.held {
            Held::Ready(ports) => ports,
            Held::Failed(error) => return (Err(error.clone()), None),
        };
        let (config, cancel) = (&self.config, &self.cancel);
        let ran = panic::catch_unwind(AssertUnwindSafe(|| {
            let mut agent = ports.agent(config, cancel);
            work(&mut agent)
        }));
        match ran {
            Ok(answer) => (Ok(answer), None),
            Err(payload) => {
                let error = LoopTaskError::Panicked(panic_message(payload.as_ref()));
                // Dropped before the answer goes out, so a caller that reads the panic finds the
                // ports already gone.
                let dropped = self.release(Held::Failed(error.clone())).err();
                self.heard = false;
                (Err(error), dropped)
            }
        }
    }

    /// A command answered the panic in [`Held::Failed`]; `read` says whether its future took it.
    /// The first answer nobody read keeps it for `shutdown`, unless somebody already heard it.
    fn answered_panic(&mut self, error: LoopTaskError, read: bool) {
        if !read && !self.heard {
            self.keep_unheard(error);
        }
        self.heard = true;
    }

    /// Keeps the panic in [`Held::Failed`] that nobody heard. The builder's is the first panic
    /// there was, so it displaces whatever was kept after it.
    fn keep_unheard(&mut self, error: LoopTaskError) {
        if self.builder_panicked {
            self.unreported = Some(error);
        } else {
            self.keep(error);
        }
        self.heard = true;
    }

    /// Keeps the first panic nobody else will hear about.
    fn keep(&mut self, error: LoopTaskError) {
        self.unreported.get_or_insert(error);
    }

    /// Drops the ports, if any are held, leaving `next` in their place.
    fn release(&mut self, next: Held) -> Result<(), LoopTaskError> {
        match std::mem::replace(&mut self.held, next) {
            Held::Ready(ports) => panic::catch_unwind(AssertUnwindSafe(move || drop(ports)))
                .map_err(|payload| LoopTaskError::Panicked(panic_message(payload.as_ref()))),
            Held::Failed(_) => Ok(()),
        }
    }

    /// The answer `shutdown` resolves to: the drop of the ports, or an earlier unreported panic.
    fn finish(&mut self) -> Result<(), LoopTaskError> {
        // A panic still unheard here is the builder's with no command left to answer it: every
        // command that answers one marks it heard.
        if !self.heard
            && let Held::Failed(error @ LoopTaskError::Panicked(_)) = &self.held
        {
            let error = error.clone();
            self.keep_unheard(error);
        }
        let released = self.release(Held::Failed(LoopTaskError::Gone));
        match self.unreported.take() {
            Some(error) => Err(error),
            None => released,
        }
    }
}

/// The task's thread: build the ports, answer commands in order, drop the ports, exit.
fn serve<F, E>(config: LoopConfig, cancel: LoopCancel, build: F, inbox: &mpsc::Receiver<Message>)
where
    F: FnOnce(&LoopCancel) -> Result<LoopPorts, E>,
    E: fmt::Display,
{
    let constructed = panic::catch_unwind(AssertUnwindSafe(|| {
        build(&cancel).map_err(|error| error.to_string())
    }));
    let builder_panicked = constructed.is_err();
    let held = match constructed {
        Ok(Ok(ports)) => Held::Ready(ports),
        Ok(Err(message)) => Held::Failed(LoopTaskError::Build(message)),
        Err(payload) => Held::Failed(LoopTaskError::Panicked(panic_message(payload.as_ref()))),
    };
    let mut station = Station {
        config,
        cancel,
        held,
        unreported: None,
        builder_panicked,
        heard: !builder_panicked,
    };
    // Ends when the `LoopTask` is dropped, after every command it had already sent.
    while let Ok(message) = inbox.recv() {
        match message {
            Message::Job(job) => {
                // A panic in a command is answered inside `execute`. What reaches here is a panic
                // in a drop nobody waits on: an unread answer, or a skipped command's sink.
                if let Err(payload) = panic::catch_unwind(AssertUnwindSafe(|| job(&mut station))) {
                    station.keep(LoopTaskError::Panicked(panic_message(payload.as_ref())));
                }
            }
            Message::Shutdown(reply) => {
                // Read by nobody when the shutdown future was dropped; nothing is left to tell.
                let _unread = reply.send(station.finish());
                return;
            }
        }
    }
    // Nobody is left to tell about a panicking drop; it was caught so the thread ends cleanly.
    let _ignored = station.finish();
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "a panic without a message".to_owned()
    }
}
