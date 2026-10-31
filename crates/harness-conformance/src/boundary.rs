//! Calls existing production APIs; only upstream ports are scripted.
//!
//! `ValidateBudget`, `Run` and `Call` each call one library function. The `Loop` commands
//! (story:harness-loop-modeled-in-ess) drive one real `AgentLoop` run per `run_id` on a thread of
//! its own. Its model, approval and tool ports block until the scenario answers them, one request
//! at a time. A command answers the port the loop is blocked on, and the state it reports is where
//! the loop blocked next, or what it returned: observed, never computed from the specification.
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use ess_conformance::target::{DeclaredErrorValue, ObservedEvent, SemanticCommandResult};
use ess_primitives::consistency::ConsistencyToken;
use ess_primitives::node::Node;
use harness_loop::{
    AgentLoop, ApprovalCheckpoint, ApprovalDecision, ApprovalPort, ApproveAll, Budget, DenyAll,
    LoopConfig, LoopError, LoopEvent, LoopOutcome, LoopSink, LoopStop, RunLedger, VecLoopSink,
};
use harness_wire::{
    Approval, CallId, Envelope, Item, ModelPort, Risk, StopReason, StreamSink, ToolCall, ToolName,
    ToolOutcome, ToolPort, ToolSpec, TurnOutcome, TurnRequest, Usage, WireError, WireId,
};
use serde_json::{Value, json};

use crate::Result;

/// How often a port was reached, readable after the port has moved to another thread.
type Count = Arc<AtomicU64>;

struct Model {
    wire: WireId,
    script: VecDeque<TurnOutcome>,
    calls: Count,
}
impl ModelPort for Model {
    fn wire(&self) -> &WireId {
        &self.wire
    }
    fn turn(
        &mut self,
        request: &TurnRequest,
        _: &mut dyn StreamSink,
    ) -> std::result::Result<TurnOutcome, WireError> {
        request.validate()?;
        request.check_opaque_items(&self.wire)?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.script
            .pop_front()
            .ok_or_else(|| WireError::protocol("fixture model exhausted"))
    }
}

/// `read` runs unattended (`Risk::Low`); `write` is over the default unattended ceiling.
fn specs() -> Result<Vec<ToolSpec>> {
    let mut specs = Vec::new();
    for (name, risk) in [("read", Risk::Low), ("write", Risk::Medium)] {
        specs.push(ToolSpec {
            name: ToolName::new(name)?,
            description: name.into(),
            input_schema: json!({"type":"object"}),
            approval: Approval::NotRequired,
            envelope: Envelope {
                risk,
                ..Envelope::default()
            },
        });
    }
    Ok(specs)
}

struct Tools {
    specs: Vec<ToolSpec>,
    calls: Count,
}
impl Tools {
    fn new(calls: Count) -> Result<Self> {
        Ok(Self {
            specs: specs()?,
            calls,
        })
    }
}
impl ToolPort for Tools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }
    fn call(&mut self, _: &ToolCall) -> ToolOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        ToolOutcome::ok(json!("effect"))
    }
}

pub fn execute(command: &str, input: &Value) -> Result<Value> {
    match command {
        "harness.loop.ValidateBudget" => {
            let budget: Budget = serde_json::from_value(input["budget"].clone())?;
            let diagnostic = budget
                .validate(boolean(input, "priced")?)
                .err()
                .map(|e| e.to_string());
            Ok(json!({"accepted":diagnostic.is_none(),"diagnostic":diagnostic}))
        }
        "harness.loop.Run" => run_loop(input),
        "harness.loop.Call" => call(input),
        _ => Err(format!("no production adapter for {command}").into()),
    }
}
fn boolean(value: &Value, field: &str) -> Result<bool> {
    value[field]
        .as_bool()
        .ok_or_else(|| format!("{field} must be Boolean").into())
}
fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("{field} must be String").into())
}
fn model(script: VecDeque<TurnOutcome>, calls: Count) -> Result<Model> {
    Ok(Model {
        wire: WireId::new("fixture")?,
        script,
        calls,
    })
}
fn approver(approve: bool) -> Box<dyn ApprovalPort> {
    if approve {
        Box::new(ApproveAll)
    } else {
        Box::new(DenyAll)
    }
}

/// One `harness.loop.Run` input, read once, so every way of driving it starts from the same ports.
struct RunFixture {
    script: VecDeque<TurnOutcome>,
    approve: bool,
    cancelled: bool,
    config: LoopConfig,
    model_calls: Count,
    tool_calls: Count,
}
impl RunFixture {
    fn read(input: &Value) -> Result<Self> {
        let mut config = LoopConfig::new("fixture", "Deterministic native conformance");
        config.budget = serde_json::from_value(input["budget"].clone())?;
        Ok(Self {
            script: serde_json::from_value(input["script"].clone())?,
            approve: boolean(input, "approve")?,
            cancelled: boolean(input, "cancelled")?,
            config,
            model_calls: Count::default(),
            tool_calls: Count::default(),
        })
    }

    /// The response for what the run returned; `items` and `ledger` are what it wrote back.
    fn response(
        &self,
        outcome: std::result::Result<LoopOutcome, LoopError>,
        items: &[Item],
        ledger: &RunLedger,
    ) -> Result<Value> {
        let model_calls = self.model_calls.load(Ordering::SeqCst);
        let tool_calls = self.tool_calls.load(Ordering::SeqCst);
        Ok(match outcome {
            Ok(outcome) => {
                let tokens = outcome.total_tokens();
                let failures = failed_results(&outcome.items);
                json!({"stop":tagged(&outcome.stop)?,"text":outcome.text,"turns":outcome.turns,"model_calls":model_calls,"tool_calls":tool_calls,"failed_results":failures,"input_tokens":tokens.map(|v|v.0),"output_tokens":tokens.map(|v|v.1),"diagnostic":null})
            }
            Err(error) => {
                let tokens = ledger.total_tokens();
                let failures = failed_results(items);
                json!({"stop":null,"text":"","turns":ledger.turns,"model_calls":model_calls,"tool_calls":tool_calls,"failed_results":failures,"input_tokens":tokens.map(|v|v.0),"output_tokens":tokens.map(|v|v.1),"diagnostic":error.to_string()})
            }
        })
    }
}
fn failed_results(items: &[Item]) -> usize {
    items
        .iter()
        .filter(|item| matches!(item, Item::ToolResult { failed: true, .. }))
        .count()
}
fn run_loop(input: &Value) -> Result<Value> {
    let fixture = RunFixture::read(input)?;
    let mut model = model(fixture.script.clone(), Arc::clone(&fixture.model_calls))?;
    let mut tools = Tools::new(Arc::clone(&fixture.tool_calls))?;
    let mut approver = approver(fixture.approve);
    let mut agent = AgentLoop::new(
        &mut model,
        &mut tools,
        approver.as_mut(),
        fixture.config.clone(),
    );
    if fixture.cancelled {
        agent.cancel_handle().cancel();
    }
    let mut items = Vec::new();
    let mut ledger = RunLedger::default();
    let outcome = agent.run_in(
        &mut items,
        &mut ledger,
        "synthetic input",
        &mut VecLoopSink::new(),
    );
    fixture.response(outcome, &items, &ledger)
}
/// The production serialization of `stop`, in the wire form ESS gives a tagged union.
///
/// serde writes a variant's fields beside `kind` (`#[serde(tag = "kind")]`); ESS 0.77.0 writes them
/// under `value`, and a unit variant as the tag alone (`spec/COVERAGE.md`). Members are moved, none
/// is computed.
fn tagged(stop: &LoopStop) -> Result<Value> {
    let Value::Object(mut fields) = serde_json::to_value(stop)? else {
        return Err("a loop stop serializes as an object".into());
    };
    let kind = fields
        .remove("kind")
        .ok_or("a loop stop serializes its kind")?;
    Ok(if fields.is_empty() {
        json!({"kind": kind})
    } else {
        json!({"kind": kind, "value": fields})
    })
}
/// One `harness.loop.Call` input.
struct CallFixture {
    approve: bool,
    call: ToolCall,
    tool_calls: Count,
}
impl CallFixture {
    fn read(input: &Value) -> Result<Self> {
        Ok(Self {
            approve: boolean(input, "approve")?,
            call: ToolCall {
                call_id: CallId::new("call")?,
                name: ToolName::new(input["name"].as_str().ok_or("name must be String")?)?,
                arguments: json!({}),
            },
            tool_calls: Count::default(),
        })
    }

    fn config() -> LoopConfig {
        LoopConfig::new("fixture", "Deterministic native conformance")
    }

    fn response(&self, outcome: &ToolOutcome) -> Value {
        json!({"failed":outcome.failed,"tool_calls":self.tool_calls.load(Ordering::SeqCst),"refusal":outcome.refusal})
    }
}
fn call(input: &Value) -> Result<Value> {
    let fixture = CallFixture::read(input)?;
    let mut model = model(VecDeque::new(), Count::default())?;
    let mut tools = Tools::new(Arc::clone(&fixture.tool_calls))?;
    let mut approver = approver(fixture.approve);
    let outcome = AgentLoop::new(
        &mut model,
        &mut tools,
        approver.as_mut(),
        CallFixture::config(),
    )
    .call(&fixture.call, &mut VecLoopSink::new());
    Ok(fixture.response(&outcome))
}

// -------------------------------------------------------------------------------------------------
// The Loop entity: one real run per run_id, answered port by port.
// -------------------------------------------------------------------------------------------------

/// The context window the stepwise runs declare, in tokens. Small, so that one long answer
/// (`LONG_TEXT_BYTES`) crosses the compaction trigger and a short one never does.
const CONTEXT_WINDOW: u64 = 4096;
/// Long assistant text: over 80 % of `CONTEXT_WINDOW` at four bytes a token, and over
/// `SUMMARY_MIN_FOLD_BYTES`, so the next turn is a summary turn.
const LONG_TEXT_BYTES: usize = 16 * 1024;
/// Text for the trigger's edge: over `SUMMARY_MIN_FOLD_BYTES`, and about 58 % of `CONTEXT_WINDOW`
/// at four bytes a token, so the provider's reported count decides whether the trigger fires.
const EDGE_TEXT_BYTES: usize = 9 * 1024;
/// 80 % of `CONTEXT_WINDOW`, written out rather than computed from the loop's constant, so a
/// changed trigger in the loop is a failed scenario and not a moved fixture.
const TRIGGER_TOKENS: u64 = 3276;
/// The input tokens an ordinary turn reports.
const REPORTED_TOKENS: u64 = 100;
/// How long the adapter waits for the loop to block again or return before calling it stuck.
const PATIENCE: Duration = Duration::from_secs(30);

/// The usage an answered turn reports, so the ledger has something exact to carry.
fn usage_of(input_tokens: u64) -> Usage {
    Usage {
        model: "fixture".to_owned(),
        input_tokens,
        output_tokens: 10,
        cached_input_tokens: 0,
        cache_creation_input_tokens: None,
    }
}

/// The assistant text and reported input tokens of a `harness.loop.TextSize`.
fn sized(size: &str) -> Result<(String, u64)> {
    Ok(match size {
        "short" => ("Calling the tool.".to_owned(), REPORTED_TOKENS),
        "long" => (text_of(LONG_TEXT_BYTES), REPORTED_TOKENS),
        "under-trigger" => (text_of(EDGE_TEXT_BYTES), TRIGGER_TOKENS - 1),
        "at-trigger" => (text_of(EDGE_TEXT_BYTES), TRIGGER_TOKENS),
        other => return Err(format!("no text size {other}").into()),
    })
}

fn config() -> LoopConfig {
    let mut config = LoopConfig::new("fixture", "Deterministic native conformance")
        .with_retry_backoff(Duration::from_millis(1));
    config.context_window = Some(CONTEXT_WINDOW);
    config
}

/// The port a running loop is blocked on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Port {
    Model,
    Summary,
    Approval,
    Tool,
}

/// What a port was answered with.
enum Reply {
    Model(std::result::Result<TurnOutcome, WireError>),
    Approval(ApprovalDecision),
    Tool(ToolOutcome),
}

/// What the run's thread reports: it blocked on a port, or the library call returned.
enum Report {
    Blocked(Port),
    Ended(Ended),
}

/// A returned run, serialized on its own thread.
struct Ended {
    /// The stop it returned, or the `Failed` stop it finished with on an error (`lib.rs:845`).
    stop: Option<Value>,
    /// The ledger `run_in` wrote; `None` for `resume_approval`, which writes none.
    ledger: Option<Value>,
    /// The checkpoint it returned, as the bytes an embedder would persist.
    checkpoint: Option<String>,
    /// The error it returned.
    error: Option<String>,
}

/// What the run's events said, read by the adapter after each report.
#[derive(Default)]
struct Observed {
    turn: Option<u64>,
    retries: u64,
    finished: Option<LoopStop>,
    folded: usize,
    summary_failed: bool,
}

struct Observer(Arc<Mutex<Observed>>);
impl LoopSink for Observer {
    fn emit(&mut self, event: LoopEvent) {
        let mut seen = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match event {
            LoopEvent::TurnStarted { turn } => {
                seen.turn = Some(turn);
                seen.retries = 0;
            }
            LoopEvent::TurnRetried { attempt, .. } => seen.retries = u64::from(attempt),
            LoopEvent::Compacted {
                summarised_items, ..
            } => seen.folded = summarised_items,
            LoopEvent::Warning { code, .. } if code == "summary-failed" => {
                seen.summary_failed = true;
            }
            LoopEvent::Finished { stop, .. } => seen.finished = Some(stop),
            _ => {}
        }
    }
}

/// The run thread's end of the conversation with the adapter.
struct Line {
    reports: Sender<Report>,
    replies: Receiver<Reply>,
}
impl Line {
    /// Says where the loop is blocked and waits for the answer; `None` once the adapter is gone.
    fn ask(&self, port: Port) -> Option<Reply> {
        self.reports.send(Report::Blocked(port)).ok()?;
        self.replies.recv().ok()
    }
}

struct AskedModel {
    wire: WireId,
    line: Rc<Line>,
}
impl ModelPort for AskedModel {
    fn wire(&self) -> &WireId {
        &self.wire
    }
    fn turn(
        &mut self,
        request: &TurnRequest,
        _: &mut dyn StreamSink,
    ) -> std::result::Result<TurnOutcome, WireError> {
        request.validate()?;
        request.check_opaque_items(&self.wire)?;
        // A summary turn is the one request that publishes no tools (lib.rs:3224).
        let port = if request.tools.is_empty() {
            Port::Summary
        } else {
            Port::Model
        };
        match self.line.ask(port) {
            Some(Reply::Model(answer)) => answer,
            Some(_) => Err(WireError::protocol(
                "the model port was answered for another port",
            )),
            None => Err(WireError::cancelled()),
        }
    }
}

struct AskedTools {
    specs: Vec<ToolSpec>,
    line: Rc<Line>,
}
impl ToolPort for AskedTools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }
    fn call(&mut self, _: &ToolCall) -> ToolOutcome {
        match self.line.ask(Port::Tool) {
            Some(Reply::Tool(outcome)) => outcome,
            _ => ToolOutcome::failed("the conformance scenario ended"),
        }
    }
}

struct AskedApprover {
    line: Rc<Line>,
}
impl ApprovalPort for AskedApprover {
    fn decide(&mut self, _: &ToolCall, _: &ToolSpec) -> ApprovalDecision {
        match self.line.ask(Port::Approval) {
            Some(Reply::Approval(decision)) => decision,
            _ => ApprovalDecision::denied("the conformance scenario ended"),
        }
    }
}

/// How a run starts: from nothing, or from a persisted checkpoint and a decision.
enum Start {
    Fresh,
    Resume(String, ApprovalDecision),
}

/// The adapter's end of one running loop.
struct Live {
    replies: Sender<Reply>,
    reports: Receiver<Report>,
    observed: Arc<Mutex<Observed>>,
    thread: JoinHandle<()>,
}

/// Where a run is, as last observed.
enum At {
    Blocked(Port),
    Suspended(String),
    Stopped,
    Failed,
}
impl At {
    fn state(&self) -> &'static str {
        match self {
            Self::Blocked(Port::Model) => "AwaitingModel",
            Self::Blocked(Port::Summary) => "AwaitingSummary",
            Self::Blocked(Port::Approval) => "AwaitingApproval",
            Self::Blocked(Port::Tool) => "AwaitingTool",
            Self::Suspended(_) => "Suspended",
            Self::Stopped => "Stopped",
            Self::Failed => "Failed",
        }
    }
}

/// What one command saw the loop do.
enum Step {
    Blocked(Port),
    Ended(Ended),
}

struct Session {
    at: At,
    turn: u64,
    retries: u64,
    live: Option<Live>,
}
impl Session {
    fn start(start: Start) -> Result<(Self, Step)> {
        let mut session = Self {
            at: At::Stopped,
            turn: 0,
            retries: 0,
            live: None,
        };
        if let Start::Resume(checkpoint, _) = &start {
            let checkpoint: Value = serde_json::from_str(checkpoint)?;
            session.turn = checkpoint["state"]["turns"].as_u64().unwrap_or(0);
        }
        session.live = Some(spawn(start));
        let step = session.wait()?;
        Ok((session, step))
    }

    fn answer(&mut self, reply: Reply) -> Result<Step> {
        let live = self.live.as_ref().ok_or("no run is live")?;
        {
            let mut seen = live.observed.lock().unwrap_or_else(PoisonError::into_inner);
            seen.folded = 0;
            seen.summary_failed = false;
        }
        live.replies
            .send(reply)
            .map_err(|_| "the run thread is gone")?;
        self.wait()
    }

    fn wait(&mut self) -> Result<Step> {
        let live = self.live.as_ref().ok_or("no run is live")?;
        let report = match live.reports.recv_timeout(PATIENCE) {
            Ok(report) => report,
            Err(RecvTimeoutError::Timeout) => {
                return Err("the loop neither blocked on a port nor returned".into());
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err("the run thread ended without a report".into());
            }
        };
        let attempts = {
            let seen = live.observed.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(turn) = seen.turn {
                self.turn = turn;
            }
            seen.retries
        };
        Ok(match report {
            Report::Blocked(port) => {
                self.at = At::Blocked(port);
                // The attempts belong to the request the model has not answered yet; once the
                // loop is blocked anywhere else, that request was answered and they are over.
                self.retries = if port == Port::Model { attempts } else { 0 };
                Step::Blocked(port)
            }
            Report::Ended(ended) => {
                if let Some(live) = self.live.take() {
                    drop(live.replies);
                    live.thread.join().map_err(|_| "the run thread panicked")?;
                }
                // A run that returned keeps the attempts it last had: the failing turn's.
                self.at = match (&ended.checkpoint, &ended.error) {
                    (Some(checkpoint), None) => At::Suspended(checkpoint.clone()),
                    (None, None) => At::Stopped,
                    (_, Some(_)) => At::Failed,
                };
                Step::Ended(ended)
            }
        })
    }

    fn folded(&self) -> usize {
        self.live.as_ref().map_or(0, |live| {
            live.observed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .folded
        })
    }

    fn summary_failed(&self) -> bool {
        self.live.as_ref().is_some_and(|live| {
            live.observed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .summary_failed
        })
    }

    fn row(&self, run_id: &str) -> Value {
        json!({"run_id": run_id, "state": self.at.state(), "turns": self.turn, "retries": self.retries})
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if let Some(live) = self.live.take() {
            // Closing the reply channel makes every port answer as if cancelled, so the run ends.
            drop(live.replies);
            while live.reports.recv_timeout(PATIENCE).is_ok() {}
            let _ = live.thread.join();
        }
    }
}

fn spawn(start: Start) -> Live {
    let (reply_tx, reply_rx) = channel();
    let (report_tx, report_rx) = channel();
    let observed = Arc::new(Mutex::new(Observed::default()));
    let seen = Arc::clone(&observed);
    let thread = std::thread::spawn(move || {
        let ended = run(start, &report_tx, reply_rx, &seen).unwrap_or_else(|error| Ended {
            stop: None,
            ledger: None,
            checkpoint: None,
            error: Some(format!("adapter: {error}")),
        });
        let _ = report_tx.send(Report::Ended(ended));
    });
    Live {
        replies: reply_tx,
        reports: report_rx,
        observed,
        thread,
    }
}

fn run(
    start: Start,
    reports: &Sender<Report>,
    replies: Receiver<Reply>,
    observed: &Arc<Mutex<Observed>>,
) -> Result<Ended> {
    let line = Rc::new(Line {
        reports: reports.clone(),
        replies,
    });
    let mut model = AskedModel {
        wire: WireId::new("fixture")?,
        line: Rc::clone(&line),
    };
    let mut tools = AskedTools {
        specs: specs()?,
        line: Rc::clone(&line),
    };
    let mut approver = AskedApprover { line };
    let mut agent = AgentLoop::new(&mut model, &mut tools, &mut approver, config());
    let mut sink = Observer(Arc::clone(observed));
    let finished = || {
        observed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .finished
            .clone()
    };
    match start {
        Start::Fresh => {
            let mut items = Vec::new();
            let mut ledger = RunLedger::default();
            let result = agent.run_in(&mut items, &mut ledger, "synthetic input", &mut sink);
            ended(result, Some(&ledger), finished().as_ref())
        }
        Start::Resume(checkpoint, decision) => {
            let checkpoint: ApprovalCheckpoint = serde_json::from_str(&checkpoint)?;
            let result = agent.resume_approval(checkpoint, decision, &mut sink);
            ended(result, None, finished().as_ref())
        }
    }
}

fn ended(
    result: std::result::Result<LoopOutcome, LoopError>,
    ledger: Option<&RunLedger>,
    finished: Option<&LoopStop>,
) -> Result<Ended> {
    let ledger = ledger.map(serde_json::to_value).transpose()?;
    Ok(match result {
        Ok(outcome) => Ended {
            stop: Some(tagged(&outcome.stop)?),
            ledger,
            checkpoint: outcome
                .checkpoint
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            error: None,
        },
        Err(error) => Ended {
            stop: finished.map(tagged).transpose()?,
            ledger,
            checkpoint: None,
            error: Some(error.to_string()),
        },
    })
}

/// A command's answer, before it is spelled for the conformance runner.
pub struct Answer {
    pub outcome: String,
    pub response: Option<Value>,
    pub error: Option<(&'static str, Value)>,
    pub events: Vec<(&'static str, Value)>,
    /// The write this answer was, for a `read_your_writes` read of the `Loops` view after it.
    pub written: Option<u64>,
}
impl Answer {
    fn took(outcome: &str, response: Value) -> Self {
        Self {
            outcome: outcome.to_owned(),
            response: Some(response),
            error: None,
            events: Vec::new(),
            written: None,
        }
    }

    fn refused(outcome: &str, error: &'static str, fields: Value) -> Self {
        Self {
            outcome: outcome.to_owned(),
            response: None,
            error: Some((error, fields)),
            events: Vec::new(),
            written: None,
        }
    }

    pub fn into_result(self, command: &str) -> Result<SemanticCommandResult> {
        let mut result = SemanticCommandResult::took(ess_conformance::scenario::OutcomeRef::new(
            command.parse()?,
            self.outcome.parse()?,
        ));
        if let Some(response) = self.response {
            result.response = Some(serde_json::from_value(response)?);
        }
        if let Some((error, fields)) = self.error {
            let mut declared = DeclaredErrorValue::new(error.parse()?);
            declared.fields = serde_json::from_value(fields)?;
            result = result.with_error(declared);
        }
        if let Some(written) = self.written {
            result = result.with_consistency(ConsistencyToken::new(written.to_string())?);
        }
        for (event, payload) in self.events {
            let mut observed = ObservedEvent::new(event.parse()?);
            observed.payload = serde_json::from_value::<BTreeMap<String, Node>>(payload)?;
            result = result.emitting(observed);
        }
        Ok(result)
    }
}

/// Every run a conformance scenario started, by scenario correlation and then `run_id`.
#[derive(Default)]
pub struct Loops {
    scenarios: Mutex<BTreeMap<String, BTreeMap<String, Session>>>,
    writes: std::sync::atomic::AtomicU64,
}

/// The states each answer is for: the port it answers, or the suspension it resumes.
fn awaited(command: &str) -> Option<&'static [&'static str]> {
    Some(match command {
        "harness.loop.AnswerTurn" | "harness.loop.CallTool" | "harness.loop.FailTurn" => {
            &["AwaitingModel"]
        }
        "harness.loop.CancelTurn" => &["AwaitingModel", "AwaitingSummary"],
        "harness.loop.AnswerSummary" | "harness.loop.FailSummary" => &["AwaitingSummary"],
        "harness.loop.Approve" | "harness.loop.Deny" | "harness.loop.Defer" => {
            &["AwaitingApproval"]
        }
        "harness.loop.ResumeApproved" | "harness.loop.ResumeDenied" => &["Suspended"],
        "harness.loop.CompleteTool" => &["AwaitingTool"],
        _ => return None,
    })
}

fn turn(text: String, calls: Vec<ToolCall>) -> TurnOutcome {
    reported_turn(text, calls, REPORTED_TOKENS)
}

fn reported_turn(text: String, calls: Vec<ToolCall>, input_tokens: u64) -> TurnOutcome {
    let stop_reason = if calls.is_empty() {
        StopReason::EndTurn
    } else {
        StopReason::ToolCalls
    };
    let mut items = vec![Item::AssistantText { text }];
    items.extend(calls.into_iter().map(Item::ToolCall));
    TurnOutcome {
        stop_reason,
        items,
        usage: Some(usage_of(input_tokens)),
    }
}

fn text_of(bytes: usize) -> String {
    "The earlier conversation, at length. "
        .repeat(bytes / 37 + 1)
        .chars()
        .take(bytes)
        .collect()
}

/// The response of a command that saw the run return.
fn ended_response(ended: &Ended) -> Value {
    json!({"stop": ended.stop, "ledger": ended.ledger})
}

fn unexpected(command: &str, step: &Step) -> Box<dyn std::error::Error> {
    let seen = match step {
        Step::Blocked(port) => format!("the loop blocked on {port:?}"),
        Step::Ended(ended) => format!(
            "the run returned stop {:?}, error {:?}, checkpoint {}",
            ended.stop,
            ended.error,
            ended.checkpoint.is_some()
        ),
    };
    format!("{command}: {seen}").into()
}

impl Loops {
    pub fn begin(&self, correlation: &str) {
        self.lock().insert(correlation.to_owned(), BTreeMap::new());
    }

    pub fn end(&self, correlation: &str) {
        let ended = self.lock().remove(correlation);
        drop(ended);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, BTreeMap<String, Session>>> {
        self.scenarios
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The `Loops` view: one row per run this scenario started.
    pub fn rows(&self, correlation: &str) -> Vec<Value> {
        self.lock()
            .get(correlation)
            .map(|runs| {
                runs.iter()
                    .map(|(run_id, session)| session.row(run_id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Answers `command`; every answer is a write the `Loops` view is read no older than, refusals
    /// included, because the view is read from the same observations that decided it.
    pub fn execute(&self, correlation: &str, command: &str, input: &Value) -> Result<Answer> {
        let mut answer = self.decide(correlation, command, input)?;
        answer.written = Some(
            self.writes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1,
        );
        Ok(answer)
    }

    fn decide(&self, correlation: &str, command: &str, input: &Value) -> Result<Answer> {
        if command == "harness.loop.StartLoop" {
            return self.start(correlation, input);
        }
        let Some(awaited) = awaited(command) else {
            return Ok(Answer::took("returned", execute(command, input)?));
        };
        let run_id = string(input, "run_id")?.to_owned();
        let mut scenarios = self.lock();
        let runs = scenarios.entry(correlation.to_owned()).or_default();
        let Some(session) = runs.get_mut(&run_id) else {
            return Ok(Answer::refused(
                "unknown",
                "harness.loop.UnknownRun",
                json!({"run_id": run_id}),
            ));
        };
        let state = session.at.state();
        if !awaited.contains(&state) {
            return Ok(Answer::refused(
                "not-awaiting",
                "harness.loop.NotAwaiting",
                json!({"state": state}),
            ));
        }
        answer(session, command, input)
    }

    fn start(&self, correlation: &str, input: &Value) -> Result<Answer> {
        let run_id = string(input, "run_id")?.to_owned();
        if self
            .lock()
            .get(correlation)
            .is_some_and(|runs| runs.contains_key(&run_id))
        {
            return Err(format!("a loop already runs under {run_id}").into());
        }
        let (session, step) = Session::start(Start::Fresh)?;
        let turn = session.turn;
        self.lock()
            .entry(correlation.to_owned())
            .or_default()
            .insert(run_id.clone(), session);
        match step {
            Step::Blocked(Port::Model) => {
                let mut answer = Answer::took("requested", json!({"turn": turn}));
                answer
                    .events
                    .push(("harness.loop.LoopStarted", json!({"run_id": run_id})));
                Ok(answer)
            }
            step => Err(unexpected("harness.loop.StartLoop", &step)),
        }
    }
}

/// Answers the port `session` is blocked on, or resumes it, and names the branch it observed.
fn answer(session: &mut Session, command: &str, input: &Value) -> Result<Answer> {
    let retries_before = session.retries;
    // The text sizes the specification says pass the trigger. `CallTool` names its branch from
    // this, as the specification's guard does; whether the loop agrees is what the next answer
    // observes (a summary turn, or none).
    let long = matches!(input["text"].as_str(), Some("long" | "at-trigger"));
    let step = reply(session, command, input)?;
    named(session, command, &step, long, retries_before)
}

/// Sends `command`'s answer to the port `session` is blocked on, or resumes the suspended run, and
/// returns the loop's next step.
fn reply(session: &mut Session, command: &str, input: &Value) -> Result<Step> {
    Ok(match command {
        "harness.loop.AnswerTurn" => session.answer(Reply::Model(Ok(turn(
            string(input, "text")?.to_owned(),
            Vec::new(),
        ))))?,
        "harness.loop.CallTool" => {
            let call_id = string(input, "call_id")?;
            let mut calls = vec![ToolCall {
                call_id: CallId::new(call_id)?,
                name: ToolName::new(string(input, "tool")?)?,
                arguments: json!({}),
            }];
            if let Some(then) = input.get("then").and_then(Value::as_str) {
                calls.push(ToolCall {
                    call_id: CallId::new(format!("{call_id}-then"))?,
                    name: ToolName::new(then)?,
                    arguments: json!({}),
                });
            }
            let (text, reported) = sized(string(input, "text")?)?;
            session.answer(Reply::Model(Ok(reported_turn(text, calls, reported))))?
        }
        "harness.loop.FailTurn" => {
            session.answer(Reply::Model(Err(if boolean(input, "retriable")? {
                WireError::transport("the connection dropped")
            } else {
                WireError::protocol("the provider refused the request")
            })))?
        }
        "harness.loop.CancelTurn" => session.answer(Reply::Model(Err(WireError::cancelled())))?,
        "harness.loop.AnswerSummary" => session.answer(Reply::Model(Ok(turn(
            "Summary of the earlier conversation.".to_owned(),
            Vec::new(),
        ))))?,
        "harness.loop.FailSummary" => session.answer(Reply::Model(Err(WireError::protocol(
            "the provider refused the summary",
        ))))?,
        "harness.loop.Approve" => session.answer(Reply::Approval(ApprovalDecision::Approved))?,
        "harness.loop.Deny" => session.answer(Reply::Approval(ApprovalDecision::denied(
            string(input, "reason")?,
        )))?,
        "harness.loop.Defer" => session.answer(Reply::Approval(ApprovalDecision::Deferred {
            checkpoint_id: string(input, "checkpoint_id")?.to_owned(),
        }))?,
        "harness.loop.CompleteTool" => {
            session.answer(Reply::Tool(ToolOutcome::ok(json!("effect"))))?
        }
        "harness.loop.ResumeApproved" | "harness.loop.ResumeDenied" => {
            let At::Suspended(checkpoint) = &session.at else {
                return Err("only a suspended run resumes".into());
            };
            let decision = if command == "harness.loop.ResumeApproved" {
                ApprovalDecision::Approved
            } else {
                ApprovalDecision::denied(string(input, "reason")?)
            };
            let (resumed, step) = Session::start(Start::Resume(checkpoint.clone(), decision))?;
            *session = resumed;
            step
        }
        _ => return Err(format!("no production adapter for {command}").into()),
    })
}

/// Names the outcome of `command` from the step the loop took after it: the port it asks next, or
/// what the run returned.
fn named(
    session: &Session,
    command: &str,
    step: &Step,
    long: bool,
    retries_before: u64,
) -> Result<Answer> {
    let at_turn = json!({"turn": session.turn});
    // Where the loop went after a call was answered: the turn's next call (asked or run), a
    // summary turn, or the next conversation turn.
    let after = |base: &str, step: &Step| match step {
        Step::Blocked(Port::Approval) => Some(format!("{base}-then-ask")),
        Step::Blocked(Port::Tool) => Some(format!("{base}-then-call")),
        Step::Blocked(Port::Summary) => Some(format!("{base}-then-summarise")),
        Step::Blocked(Port::Model) => Some(base.to_owned()),
        Step::Ended(_) => None,
    };
    let outcome = match (command, step) {
        ("harness.loop.AnswerTurn", Step::Ended(ended))
            if ended.error.is_none() && ended.stop == Some(json!({"kind": "completed"})) =>
        {
            return Ok(Answer::took("completed", ended_response(ended)));
        }
        ("harness.loop.CancelTurn", Step::Ended(ended))
            if ended.error.is_none()
                && ended.stop.as_ref().and_then(|stop| stop.get("kind"))
                    == Some(&json!("cancelled")) =>
        {
            return Ok(Answer::took("cancelled", ended_response(ended)));
        }
        ("harness.loop.Defer", Step::Ended(ended)) if ended.error.is_none() => {
            if let Some(checkpoint) = &ended.checkpoint {
                let checkpoint: Value = serde_json::from_str(checkpoint)?;
                return Ok(Answer::took(
                    "deferred",
                    json!({"stop": ended.stop, "ledger": ended.ledger, "checkpoint": checkpoint}),
                ));
            }
            None
        }
        ("harness.loop.FailTurn", Step::Blocked(Port::Model)) if session.retries > 0 => {
            Some("retried".to_owned())
        }
        ("harness.loop.FailTurn", Step::Ended(ended)) if ended.error.is_some() => {
            let branch = if retries_before >= 3 {
                "exhausted"
            } else {
                "failed"
            };
            return Ok(Answer::took(
                branch,
                json!({"turn": session.turn, "stop": ended.stop, "ledger": ended.ledger}),
            ));
        }
        ("harness.loop.CallTool", Step::Blocked(Port::Approval)) => {
            Some(if long { "asked-long" } else { "asked" }.to_owned())
        }
        ("harness.loop.CallTool", Step::Blocked(Port::Tool)) => {
            Some(if long { "called-long" } else { "called" }.to_owned())
        }
        ("harness.loop.Approve", Step::Blocked(Port::Tool)) => Some("approved".to_owned()),
        ("harness.loop.ResumeApproved", Step::Blocked(Port::Tool)) => Some("resumed".to_owned()),
        ("harness.loop.Deny", step) => after("denied", step),
        ("harness.loop.ResumeDenied", step) => after("resumed-denied", step),
        ("harness.loop.CompleteTool", step) => after("returned", step),
        ("harness.loop.AnswerSummary", Step::Blocked(Port::Model)) if session.folded() > 0 => {
            Some("folded".to_owned())
        }
        ("harness.loop.FailSummary", Step::Blocked(Port::Model)) if session.summary_failed() => {
            Some("kept".to_owned())
        }
        _ => None,
    };
    match outcome {
        Some(outcome) => Ok(Answer::took(&outcome, at_turn)),
        None => Err(unexpected(command, step)),
    }
}

#[cfg(test)]
mod tests {
    //! `LoopTask` and `AgentLoop` agree on every scenario the boundary drives through the loop
    //! (story `loop-task-awaits-a-run`).

    use std::future::Future;
    use std::path::Path;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    use std::thread;
    use std::time::{Duration, Instant};

    use harness_loop::{LoopCancel, LoopPorts, LoopTask, VecLoopSink};
    use serde_json::Value;

    use super::{CallFixture, RunFixture, Tools, approver, execute, model};
    use crate::Result;

    /// Wakes the thread waiting in [`block_on`].
    struct Unpark(thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    /// Polls `future` on this thread, parking between polls, for at most five seconds.
    fn block_on<F: Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        let waker = Waker::from(Arc::new(Unpark(thread::current())));
        let mut context = Context::from_waker(&waker);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
            let now = Instant::now();
            assert!(now < deadline, "the loop task did not answer within 5 s");
            thread::park_timeout(deadline - now);
        }
    }

    /// `harness.loop.Run` through `LoopTask::run_in`; a cancelled scenario cancels through the
    /// task's own handle before the run is sent.
    fn run_through_task(input: &Value) -> Result<Value> {
        let fixture = RunFixture::read(input)?;
        let (script, approve) = (fixture.script.clone(), fixture.approve);
        let (model_calls, tool_calls) = (
            Arc::clone(&fixture.model_calls),
            Arc::clone(&fixture.tool_calls),
        );
        let task = LoopTask::spawn(fixture.config.clone(), move |_: &LoopCancel| -> Result<_> {
            Ok(LoopPorts::boxed(
                Box::new(model(script, model_calls)?),
                Box::new(Tools::new(tool_calls)?),
                approver(approve),
            ))
        });
        if fixture.cancelled {
            task.cancel_handle().cancel();
        }
        let ran = block_on(task.run_in(Vec::new(), "synthetic input", VecLoopSink::new()))?;
        block_on(task.shutdown())?;
        fixture.response(ran.result, &ran.items, &ran.spend)
    }

    /// `harness.loop.Call` through `LoopTask::call`.
    fn call_through_task(input: &Value) -> Result<Value> {
        let fixture = CallFixture::read(input)?;
        let (approve, tool_calls) = (fixture.approve, Arc::clone(&fixture.tool_calls));
        let task = LoopTask::spawn(CallFixture::config(), move |_: &LoopCancel| -> Result<_> {
            Ok(LoopPorts::boxed(
                Box::new(model(
                    std::collections::VecDeque::new(),
                    super::Count::default(),
                )?),
                Box::new(Tools::new(tool_calls)?),
                approver(approve),
            ))
        });
        let (outcome, _) = block_on(task.call(fixture.call.clone(), VecLoopSink::new()))?;
        block_on(task.shutdown())?;
        Ok(fixture.response(&outcome))
    }

    #[test]
    fn loop_task_and_agent_loop_agree_on_every_run_and_call_scenario() -> Result<()> {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/scenarios");
        let mut paths: Vec<_> = std::fs::read_dir(&directory)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<_>>()?;
        paths.sort();
        let mut compared = 0;
        for path in paths {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("scenario file name")?
                .to_owned();
            if !(name.starts_with("run-") || name.starts_with("call-")) {
                continue;
            }
            let scenario: Value = serde_yaml_ng::from_str(&std::fs::read_to_string(&path)?)?;
            for step in scenario["timeline"].as_array().ok_or("timeline")? {
                let command = step["command"].as_str().ok_or("command")?;
                let input = &step["input"];
                let through_task = match command {
                    "harness.loop.Run" => run_through_task(input)?,
                    "harness.loop.Call" => call_through_task(input)?,
                    other => return Err(format!("{name}: unexpected command {other}").into()),
                };
                assert_eq!(
                    through_task,
                    execute(command, input)?,
                    "{name}: LoopTask and AgentLoop disagree"
                );
            }
            compared += 1;
        }
        assert_eq!(compared, 17, "the run-* and call-* scenarios compared");
        Ok(())
    }
}
