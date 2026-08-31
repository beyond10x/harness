use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use harness_wire::{
    AccessKind, CallId, Effect, Envelope, Idempotency, Item, ModelPort, Risk, StopReason,
    StreamEvent, StreamSink, ToolCall, ToolName, ToolOutcome, ToolPort, ToolSpec, TurnOutcome,
    TurnRequest, Usage, WireError, WireErrorCode, WireId,
};
use serde_json::{Value, json};

use super::*;

const WIRE: &str = "scripted";

/// The verb a call names when the entry it wants is an argument.
///
/// `harness_tools::verbs::INVOKE_VERB` spelled again rather than depended on: `harness-loop` does
/// not know `harness-tools` exists, and a test that imported it would be asserting over the very
/// coupling this crate refuses.
const INVOKE_VERB: &str = "tool_invoke";

fn wire() -> WireId {
    WireId::new(WIRE).expect("the test wire id is valid")
}

fn tool_name(name: &str) -> ToolName {
    ToolName::new(name).expect("test tool names are valid")
}

fn call_id(id: &str) -> CallId {
    CallId::new(id).expect("test call ids are valid")
}

fn spec(name: &str, approval: Approval) -> ToolSpec {
    ToolSpec {
        name: tool_name(name),
        description: format!("the {name} tool"),
        envelope: Envelope::default(),
        input_schema: json!({"type": "object"}),
        approval,
    }
}

fn usage(input: u64, output: u64) -> Usage {
    Usage {
        model: "scripted-model".to_owned(),
        input_tokens: input,
        output_tokens: output,
        cached_input_tokens: 0,
        cache_creation_input_tokens: None,
    }
}

fn answer(text: &str) -> TurnOutcome {
    TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::assistant(text)],
        usage: Some(usage(10, 5)),
    }
}

/// The same turn, with the input count the provider reported for it.
///
/// The figure a token-aware compaction reads: the provider counts the instruction and the tool
/// schemas too, so it is always larger than anything measured from the items alone.
fn reporting(mut turn: TurnOutcome, input: u64) -> TurnOutcome {
    turn.usage = Some(usage(input, 5));
    turn
}

/// A turn that says something long and then asks for tools — the shape whose weight elision
/// cannot reach, because only tool-result payloads are elidable.
fn says_and_asks(text: &str, calls: &[(&str, &str, Value)]) -> TurnOutcome {
    let mut turn = asks_for(calls);
    turn.items.insert(0, Item::assistant(text));
    turn
}

fn asks_for(calls: &[(&str, &str, Value)]) -> TurnOutcome {
    TurnOutcome {
        stop_reason: StopReason::ToolCalls,
        items: calls
            .iter()
            .map(|(id, name, arguments)| {
                Item::ToolCall(ToolCall {
                    call_id: call_id(id),
                    name: tool_name(name),
                    arguments: arguments.clone(),
                })
            })
            .collect(),
        usage: Some(usage(10, 5)),
    }
}

/// A model that replays a script and records what it was asked.
struct ScriptedModel {
    wire: WireId,
    script: VecDeque<Result<TurnOutcome, WireError>>,
    seen: Vec<TurnRequest>,
    text_per_turn: Option<String>,
    cancel_after: Option<(usize, LoopCancel)>,
}

impl ScriptedModel {
    fn new(script: Vec<Result<TurnOutcome, WireError>>) -> Self {
        Self {
            wire: wire(),
            script: script.into(),
            seen: Vec::new(),
            text_per_turn: None,
            cancel_after: None,
        }
    }

    fn streaming(mut self, text: &str) -> Self {
        self.text_per_turn = Some(text.to_owned());
        self
    }

    fn cancelling_after(mut self, turn: usize, cancel: LoopCancel) -> Self {
        self.cancel_after = Some((turn, cancel));
        self
    }
}

impl ModelPort for ScriptedModel {
    fn wire(&self) -> &WireId {
        &self.wire
    }

    fn turn(
        &mut self,
        request: &TurnRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        request.validate()?;
        request.check_opaque_items(&self.wire)?;
        self.seen.push(request.clone());
        if let Some(text) = &self.text_per_turn {
            sink.emit(StreamEvent::TextDelta { text: text.clone() });
        }
        if let Some((turn, cancel)) = &self.cancel_after
            && self.seen.len() == *turn
        {
            cancel.cancel();
        }
        self.script
            .pop_front()
            .unwrap_or_else(|| Err(WireError::protocol("the script ran out of turns")))
    }
}

/// A tool port that answers from a table and records every call.
struct ScriptedTools {
    specs: Vec<ToolSpec>,
    outcomes: BTreeMap<String, ToolOutcome>,
    calls: Vec<ToolCall>,
    cancel_after: Option<(usize, LoopCancel)>,
    envelope: Option<Envelope>,
    /// A different envelope per tool name, for a turn that mixes reads with a write.
    envelopes: BTreeMap<String, Envelope>,
    invoked: Option<ToolSpec>,
    /// Specs for names this port never published — catalogue entries reachable behind a verb.
    routed: BTreeMap<String, ToolSpec>,
    /// The catalogue a verb surface publishes its three verbs over, by entry name.
    catalogue: BTreeMap<String, ToolSpec>,
    /// Answer `reachable` with nothing, the way a port that cannot tell would.
    hides_reach: bool,
    /// Answer `reachable` with *part* of what it publishes, the way a port that half-tells would.
    reach_only: Option<Vec<ToolName>>,
    delay: Option<Duration>,
    /// What the loop said was left on the clock, per call.
    remaining: Vec<Option<Duration>>,
    /// How many calls each `call_batch` was handed, in order. Empty when nothing was batched.
    batches: Vec<usize>,
    /// Answer this many outcomes however many calls arrive, to exercise the loop's own count check.
    miscount: Option<usize>,
}

impl ScriptedTools {
    fn new(specs: Vec<ToolSpec>) -> Self {
        Self {
            specs,
            outcomes: BTreeMap::new(),
            calls: Vec::new(),
            cancel_after: None,
            envelope: None,
            envelopes: BTreeMap::new(),
            invoked: None,
            routed: BTreeMap::new(),
            catalogue: BTreeMap::new(),
            hides_reach: false,
            reach_only: None,
            delay: None,
            remaining: Vec::new(),
            batches: Vec::new(),
            miscount: None,
        }
    }

    fn answering(mut self, name: &str, outcome: ToolOutcome) -> Self {
        self.outcomes.insert(name.to_owned(), outcome);
        self
    }

    fn cancelling_after(mut self, calls: usize, cancel: LoopCancel) -> Self {
        self.cancel_after = Some((calls, cancel));
        self
    }

    /// Answers for the **call** rather than for the spec, which is the shape a port publishing
    /// verbs over a catalogue has: one spec, a different envelope per entry behind it.
    fn enveloped(mut self, envelope: Envelope) -> Self {
        self.envelope = Some(envelope);
        self
    }

    /// Answers a different envelope for one tool name, so a turn can mix reads with a write.
    fn enveloping(mut self, name: &str, envelope: Envelope) -> Self {
        self.envelopes.insert(name.to_owned(), envelope);
        self
    }

    /// Answers a whole other spec for every call — the entry behind a verb, under its own name.
    fn invoking(mut self, spec: ToolSpec) -> Self {
        self.invoked = Some(spec);
        self
    }

    /// Reaches an entry this port never published, the way a verb port reaches its catalogue.
    fn routing(mut self, spec: ToolSpec) -> Self {
        self.routed.insert(spec.name.as_str().to_owned(), spec);
        self
    }

    /// The catalogue this port's verbs stand over, which is what makes it a verb surface.
    ///
    /// Both routes `harness_tools::Verbs::invoked_entry` answers: `tool_invoke {"name": …}`
    /// reaches the entry the argument names, and a bare entry name reaches it directly. Neither
    /// entry is published — [`ToolPort::specs`] stays the verbs, whatever the catalogue holds.
    fn over(mut self, entries: Vec<ToolSpec>) -> Self {
        for entry in entries {
            self.catalogue.insert(entry.name.as_str().to_owned(), entry);
        }
        self
    }

    /// Under-reports its reach: `reachable` answers nothing while `specs` still publishes.
    ///
    /// The shape any `ToolPort` outside this repository can have, because `reachable` is a
    /// **defaulted** trait method and this is what a port that overrode it wrongly looks like.
    fn hiding_reach(mut self) -> Self {
        self.hides_reach = true;
        self
    }

    /// Under-reports its reach in *part*: `reachable` names some of what `specs` publishes.
    ///
    /// The half-wrong answer, which is the likelier one — a port author who overrode `reachable`
    /// and missed an entry, rather than one who returned nothing at all. It leaves the run's grant
    /// non-empty, so nothing about an empty grant can be what saves it.
    fn reaching_only(mut self, names: &[&str]) -> Self {
        self.reach_only = Some(names.iter().map(|name| tool_name(name)).collect());
        self
    }

    /// Answers `count` outcomes to every batch, whatever it was asked.
    fn miscounting(mut self, count: usize) -> Self {
        self.miscount = Some(count);
        self
    }

    /// Makes every call block, so a wall-clock bound can be reached inside a turn.
    fn taking(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }
}

impl ToolPort for ScriptedTools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    /// The catalogue where there is one, and the published names where there is not.
    ///
    /// The same split the two shipped surfaces make: `harness_tools::Verbs` answers its entries and
    /// `harness_tools::Flat` takes the trait's default, because what it publishes is what it
    /// performs.
    fn reachable(&self) -> Vec<ToolName> {
        if self.hides_reach {
            return Vec::new();
        }
        if let Some(only) = &self.reach_only {
            return only.clone();
        }
        if self.catalogue.is_empty() {
            return self.specs.iter().map(|spec| spec.name.clone()).collect();
        }
        self.catalogue
            .values()
            .map(|entry| entry.name.clone())
            .collect()
    }

    fn invoked(&self, call: &ToolCall) -> Option<ToolSpec> {
        if let Some(invoked) = &self.invoked {
            return Some(invoked.clone());
        }
        if let Some(routed) = self.routed.get(call.name.as_str()) {
            return Some(routed.clone());
        }
        let named = if call.name.as_str() == INVOKE_VERB {
            call.arguments.get("name").and_then(Value::as_str)
        } else {
            Some(call.name.as_str())
        };
        if let Some(entry) = named.and_then(|name| self.catalogue.get(name)) {
            return Some(entry.clone());
        }
        let published = self.specs.iter().find(|spec| spec.name == call.name)?;
        Some(
            match self
                .envelopes
                .get(call.name.as_str())
                .or(self.envelope.as_ref())
            {
                Some(envelope) => ToolSpec {
                    envelope: envelope.clone(),
                    ..published.clone()
                },
                None => published.clone(),
            },
        )
    }

    fn call_within(&mut self, call: &ToolCall, remaining: Option<Duration>) -> ToolOutcome {
        self.remaining.push(remaining);
        self.call(call)
    }

    fn call_batch(&mut self, calls: &[ToolCall], remaining: Option<Duration>) -> Vec<ToolOutcome> {
        self.batches.push(calls.len());
        let mut outcomes: Vec<ToolOutcome> = calls
            .iter()
            .map(|call| self.call_within(call, remaining))
            .collect();
        if let Some(count) = self.miscount {
            outcomes.truncate(count);
        }
        outcomes
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        self.calls.push(call.clone());
        if let Some(delay) = self.delay {
            std::thread::sleep(delay);
        }
        if let Some((after, cancel)) = &self.cancel_after
            && self.calls.len() == *after
        {
            cancel.cancel();
        }
        self.outcomes
            .get(call.name.as_str())
            .cloned()
            .unwrap_or_else(|| ToolOutcome::ok(json!({"ok": true})))
    }
}

struct Harness {
    model: ScriptedModel,
    tools: ScriptedTools,
    approvals: Box<dyn ApprovalPort>,
    config: LoopConfig,
    cancel: LoopCancel,
}

impl Harness {
    fn new(model: ScriptedModel, tools: ScriptedTools) -> Self {
        Self {
            model,
            tools,
            approvals: Box::new(DenyAll),
            config: LoopConfig::new("scripted-model", "be useful"),
            cancel: LoopCancel::new(),
        }
    }

    fn approving(mut self, approvals: Box<dyn ApprovalPort>) -> Self {
        self.approvals = approvals;
        self
    }

    fn budgeted(mut self, budget: Budget) -> Self {
        self.config.budget = budget;
        self
    }

    fn priced(mut self, card: RateCard) -> Self {
        self.config.prices = Some(card);
        self
    }

    /// Raises the risk this run acts on without asking anybody.
    fn unattended_above(mut self, ceiling: Risk) -> Self {
        self.config = self.config.with_unattended_ceiling(ceiling);
        self
    }

    /// Asks this run for its answer in one shape, published as a tool the loop owns.
    fn answering_in(mut self, schema: OutputSchema) -> Self {
        self.config = self.config.with_output_schema(Some(schema));
        self
    }

    /// Lets this run hand a sub-task to a fresh context over the same ports.
    fn delegating(mut self, delegation: Delegation) -> Self {
        self.config = self.config.with_delegation(Some(delegation));
        self
    }

    /// Declares the model's context window, which is what makes compaction token-aware.
    fn windowed(mut self, tokens: u64) -> Self {
        self.config = self.config.with_context_window(Some(tokens));
        self
    }

    fn retrying_after(mut self, backoff: Duration) -> Self {
        self.config = self.config.with_retry_backoff(backoff);
        self
    }

    /// Shortens the pause between attempts, so an exhaustion test costs milliseconds.
    ///
    /// The shipped 500 ms doubling would spend 3.5 seconds proving that three retries are three.
    fn impatient(self) -> Self {
        self.retrying_after(Duration::from_millis(1))
    }

    fn run(&mut self) -> (Result<LoopOutcome, LoopError>, VecLoopSink) {
        let mut sink = VecLoopSink::new();
        let outcome = AgentLoop::new(
            &mut self.model,
            &mut self.tools,
            self.approvals.as_mut(),
            self.config.clone(),
        )
        .with_cancel(self.cancel.clone())
        .run("do the thing", &mut sink);
        (outcome, sink)
    }

    /// The same run, over a conversation and a ledger the caller keeps hold of.
    fn run_in(
        &mut self,
        items: &mut Vec<Item>,
    ) -> (Result<LoopOutcome, LoopError>, RunLedger, VecLoopSink) {
        let mut sink = VecLoopSink::new();
        let mut spend = RunLedger::default();
        let outcome = AgentLoop::new(
            &mut self.model,
            &mut self.tools,
            self.approvals.as_mut(),
            self.config.clone(),
        )
        .with_cancel(self.cancel.clone())
        .run_in(items, &mut spend, "do the thing", &mut sink);
        (outcome, spend, sink)
    }
}

#[test]
fn a_text_only_answer_finishes_in_one_turn() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("here you go"))]).streaming("here you go"),
        ScriptedTools::new(Vec::new()),
    );
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("a scripted answer completes");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.turns, 1);
    assert_eq!(outcome.text, "here you go");
    assert_eq!(sink.text(), "here you go");
    assert_eq!(outcome.total_tokens(), Some((10, 5)));
}

#[test]
fn a_tool_call_round_trips_and_the_result_is_replayed_next_turn() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "workspace_read",
                json!({"path": "README.md"}),
            )])),
            Ok(answer("the file says hello")),
        ]),
        ScriptedTools::new(vec![spec("workspace_read", Approval::NotRequired)])
            .answering("workspace_read", ToolOutcome::ok(json!({"text": "hello"}))),
    );
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("the round trip completes");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.turns, 2);
    assert_eq!(harness.tools.calls.len(), 1);

    // The second request must carry the call and its result, or the model answers blind.
    let second = &harness.model.seen[1];
    assert!(
        second.items.iter().any(
            |item| matches!(item, Item::ToolResult { call_id, .. } if call_id.as_str() == "call-1")
        ),
        "{:?}",
        second.items
    );
    assert!(
        sink.events()
            .iter()
            .any(|event| matches!(event, LoopEvent::ToolCompleted { failed: false, .. }))
    );
}

#[test]
fn a_program_outside_the_declared_set_is_a_named_refusal_before_its_result() {
    // The row that read `0 refusal(s)` on a run where the refusal happened. A refused program came
    // back as `ToolCompleted { failed: true }` — the shape of a compile error — so the only way to
    // count it was to match the sentence, and downstream (where every result's content is `null`)
    // there was no sentence to match.
    let refusal = harness_wire::Refusal::ProgramNotDeclared {
        program: "sh".to_owned(),
        declared: vec!["cargo".to_owned(), "git".to_owned()],
    };
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "run",
                json!({"argv": ["sh", "-c", "id"]}),
            )])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("run", Approval::NotRequired)])
            .answering("run", ToolOutcome::refused(refusal.clone())),
    );
    let (outcome, sink) = harness.run();
    outcome.expect("a refusal is an outcome, so the run keeps turning");

    // The order is `unpublished-tool`'s: the warning, then the result it explains.
    let sequence: Vec<String> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::Warning { code, .. } => Some(format!("warning:{code}")),
            LoopEvent::ToolCompleted { call_id, failed } => {
                Some(format!("completed:{call_id}:{failed}"))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        sequence,
        vec![
            "warning:program-refused".to_owned(),
            "completed:call-1:true".to_owned()
        ],
        "{:?}",
        sink.events()
    );

    // The warning's words are the refusal's own, which are the words the model read.
    let (code, message) = sink.warnings().next().expect("one warning");
    assert_eq!(code, "program-refused");
    assert_eq!(message, refusal.message());
    assert!(message.contains("`sh` is not a program this run may start"));
    assert!(
        message.contains("cargo, git"),
        "the set is named: {message}"
    );
}

#[test]
fn a_declared_program_completes_with_no_refusal_warning() {
    // The other half: if the code fired on any failed `run` it would say nothing about the surface.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "run",
                json!({"argv": ["cargo", "test"]}),
            )])),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(vec![spec("run", Approval::NotRequired)])
            .answering("run", ToolOutcome::ok(json!({"exit": 0}))),
    );
    let (outcome, sink) = harness.run();
    outcome.expect("it runs");
    assert!(
        !sink.warnings().any(|(code, _)| code == "program-refused"),
        "{:?}",
        sink.events()
    );
}

#[test]
fn a_run_that_failed_on_its_own_terms_is_not_reported_as_a_refusal() {
    // A program that was allowed to start and exited non-zero, or could not be launched at all, is
    // the tool failing. Naming that a refusal would make the code useless for the question it was
    // added to answer.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "run",
                json!({"argv": ["cargo", "test"]}),
            )])),
            Ok(answer("noted")),
        ]),
        ScriptedTools::new(vec![spec("run", Approval::NotRequired)]).answering(
            "run",
            ToolOutcome::failed("`cargo`: No such file or directory"),
        ),
    );
    let (outcome, sink) = harness.run();
    outcome.expect("it runs");
    assert!(
        sink.events()
            .iter()
            .any(|event| matches!(event, LoopEvent::ToolCompleted { failed: true, .. })),
        "the failure is still reported"
    );
    assert!(
        !sink.warnings().any(|(code, _)| code == "program-refused"),
        "{:?}",
        sink.events()
    );
}

#[test]
fn the_first_request_carries_the_person_input_and_the_published_tools() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("ok"))]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    );
    let _ = harness.run();
    let first = &harness.model.seen[0];
    assert_eq!(first.items, vec![Item::user("do the thing")]);
    assert_eq!(first.tools.len(), 1);
    assert_eq!(first.instructions, "be useful");
}

#[test]
fn a_call_to_an_unpublished_tool_is_refused_back_to_the_model_and_warned_about() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "shell.exec",
                json!({"cmd": "rm -rf /"}),
            )])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("workspace_read", Approval::NotRequired)]),
    );
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("the run recovers");

    assert!(
        harness.tools.calls.is_empty(),
        "an unpublished tool must not run"
    );
    assert_eq!(
        sink.warnings().map(|(code, _)| code).collect::<Vec<_>>(),
        vec!["unpublished-tool"]
    );
    let failed = outcome
        .items
        .iter()
        .find_map(|item| match item {
            Item::ToolResult { failed, output, .. } => Some((*failed, output.clone())),
            _ => None,
        })
        .expect("the model is told what happened");
    assert!(failed.0);
    assert!(
        failed
            .1
            .as_str()
            .is_some_and(|text| text.contains("shell.exec")),
        "{:?}",
        failed.1
    );
}

#[test]
fn a_tool_needing_approval_does_not_run_when_the_decision_is_no() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "fs.write", json!({"path": "a"}))])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("fs.write", Approval::Required)]),
    );
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("a denial is not a failure");

    assert!(harness.tools.calls.is_empty(), "a denied tool must not run");
    assert!(sink.events().iter().any(|event| matches!(
        event,
        LoopEvent::ApprovalResolved {
            approved: false,
            ..
        }
    )));
    assert!(
        outcome
            .items
            .iter()
            .any(|item| matches!(item, Item::ToolResult { failed: true, .. }))
    );
}

#[test]
fn a_tool_needing_approval_runs_once_a_person_says_yes() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "fs.write", json!({"path": "a"}))])),
            Ok(answer("written")),
        ]),
        ScriptedTools::new(vec![spec("fs.write", Approval::Required)]),
    )
    .approving(Box::new(ApproveAll));
    let (outcome, sink) = harness.run();

    assert_eq!(harness.tools.calls.len(), 1);
    assert_eq!(outcome.expect("completes").text, "written");
    assert!(
        sink.events()
            .iter()
            .any(|event| matches!(event, LoopEvent::ApprovalResolved { approved: true, .. }))
    );
}

#[test]
fn approval_is_never_asked_for_a_tool_that_does_not_need_it() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "workspace_read", json!({}))])),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(vec![spec("workspace_read", Approval::NotRequired)]),
    );
    let (_, sink) = harness.run();
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| matches!(event, LoopEvent::ApprovalRequired { .. })),
        "{:?}",
        sink.events()
    );
}

/// The approval traffic in order, flattened so a test can compare the whole exchange at once.
fn approvals(sink: &VecLoopSink) -> Vec<String> {
    sink.events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::ApprovalRequired { call_id, .. } => {
                Some(format!("asked {}", call_id.as_str()))
            }
            LoopEvent::ApprovalResolved { call_id, approved } => {
                Some(format!("{} approved={approved}", call_id.as_str()))
            }
            _ => None,
        })
        .collect()
}

/// A call that mutates without asking anybody: a cheap, visible write.
///
/// The loop hands consecutive **pure** calls to the port as one batch, and a batch is one port call
/// with one cancellation and one deadline check in front of it. A test about what the loop does
/// *between* the calls of a turn therefore needs calls that are not batchable, and mutating is what
/// makes a call its own barrier — the read after a write may be reading what the write wrote.
fn writes() -> Envelope {
    Envelope {
        effects: vec![Effect::Write],
        risk: Risk::Low,
        idempotency: Idempotency::Idempotent,
        access: Vec::new(),
    }
}

/// What a `run` catalogue entry looks like: it starts a process, and wrong is not cheap.
fn starts_a_process() -> Envelope {
    Envelope {
        effects: vec![Effect::Process],
        risk: Risk::High,
        idempotency: Idempotency::Conditional,
        access: Vec::new(),
    }
}

fn results(outcome: &LoopOutcome) -> Vec<(bool, String)> {
    outcome
        .items
        .iter()
        .filter_map(|item| match item {
            Item::ToolResult { failed, output, .. } => {
                Some((*failed, output.as_str().unwrap_or_default().to_owned()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_high_risk_call_under_the_default_approver_is_refused_and_the_model_is_told() {
    // The defect this pins: every tool this harness ships declares `NotRequired`, so a gate that
    // read only the spec never decided anything and a `run` entry executed unasked.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "tool_invoke",
                json!({"tool": "run", "argv": ["rm", "-rf", "/"]}),
            )])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("tool_invoke", Approval::NotRequired)])
            .enveloped(starts_a_process()),
    );
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("a denial is not a failure");

    assert!(
        harness.tools.calls.is_empty(),
        "the effect must not happen before the decision"
    );
    assert_eq!(
        approvals(&sink),
        vec!["asked call-1", "call-1 approved=false"]
    );
    let told = results(&outcome);
    assert_eq!(told.len(), 1);
    assert!(told[0].0, "the model has to see that the call did not run");
    assert!(told[0].1.contains("not approved"), "{:?}", told[0].1);
}

#[test]
fn what_is_asked_about_and_refused_is_the_entry_and_not_the_verb_it_came_through() {
    // The gate decided on the entry's envelope and then reported the verb: the event said
    // `tool_invoke`, the approver was handed `tool_invoke`'s spec, and the model read
    // "`tool_invoke` was not approved" — and either stopped calling `tool_invoke` at all, losing
    // every read behind it, or retried the same entry against the same refusal.
    let entry = ToolSpec {
        envelope: starts_a_process(),
        ..spec("run", Approval::NotRequired)
    };
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "tool_invoke",
                json!({"name": "run", "arguments": {"argv": ["cargo", "test"]}}),
            )])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("tool_invoke", Approval::NotRequired)]).invoking(entry),
    );
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("a denial is not a failure");

    assert!(harness.tools.calls.is_empty());
    let asked: Vec<&str> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::ApprovalRequired { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(asked, vec!["run"], "the event names what is being decided");
    let told = results(&outcome);
    assert!(told[0].1.contains("`run`"), "{:?}", told[0].1);
    assert!(
        told[0].1.contains("`tool_invoke`"),
        "and the verb it came through, so the model can still use the verb: {:?}",
        told[0].1
    );
}

#[test]
fn the_same_call_runs_under_approve_all() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "tool_invoke",
                json!({"tool": "run"}),
            )])),
            Ok(answer("it ran")),
        ]),
        ScriptedTools::new(vec![spec("tool_invoke", Approval::NotRequired)])
            .enveloped(starts_a_process()),
    )
    .approving(Box::new(ApproveAll));
    let (outcome, sink) = harness.run();

    assert_eq!(harness.tools.calls.len(), 1);
    assert_eq!(outcome.expect("completes").text, "it ran");
    assert_eq!(
        approvals(&sink),
        vec!["asked call-1", "call-1 approved=true"]
    );
}

#[test]
fn a_low_risk_call_never_asks() {
    // Under `DenyAll`, so a gate that asked about reads would refuse this and the run would be
    // unable to do anything at all.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "workspace_read", json!({}))])),
            Ok(answer("read it")),
        ]),
        ScriptedTools::new(vec![spec("workspace_read", Approval::NotRequired)])
            .enveloped(Envelope::read_only()),
    );
    let (outcome, sink) = harness.run();

    assert_eq!(harness.tools.calls.len(), 1);
    assert_eq!(outcome.expect("completes").text, "read it");
    assert!(approvals(&sink).is_empty(), "{:?}", sink.events());
}

#[test]
fn raising_the_ceiling_stops_the_asking() {
    /// One idempotent, medium-risk write, run under the ceiling the caller declared.
    fn write_under(ceiling: Option<Risk>) -> (usize, Vec<String>) {
        let mut harness = Harness::new(
            ScriptedModel::new(vec![
                Ok(asks_for(&[("call-1", "edit", json!({"path": "a"}))])),
                Ok(answer("done")),
            ]),
            ScriptedTools::new(vec![spec("edit", Approval::NotRequired)]).enveloped(Envelope {
                effects: vec![Effect::Write, Effect::Filesystem],
                risk: Risk::Medium,
                idempotency: Idempotency::Idempotent,
                access: vec![AccessKind::Filesystem],
            }),
        );
        if let Some(ceiling) = ceiling {
            harness = harness.unattended_above(ceiling);
        }
        let (outcome, sink) = harness.run();
        assert!(outcome.expect("both arms are outcomes").stop.is_completed());
        (harness.tools.calls.len(), approvals(&sink))
    }

    let (ran, asked) = write_under(Some(Risk::Medium));
    assert_eq!(ran, 1, "at the ceiling, not above it");
    assert!(asked.is_empty(), "{asked:?}");

    let (ran, asked) = write_under(None);
    assert_eq!(
        ran, 0,
        "the default ceiling is Low, and the default approver denies"
    );
    assert_eq!(asked, vec!["asked call-1", "call-1 approved=false"]);
}

#[test]
fn a_non_idempotent_write_is_judged_on_its_risk_and_not_on_its_idempotency() {
    // Idempotency is a retry question, not an approval one. Until 2026-08-29 the loop asked about
    // every non-idempotent mutation whatever the ceiling, which let a whole-file write through at
    // `--approve-up-to medium` and refused the narrower edit — backwards.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "append", json!({"path": "log"}))])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("append", Approval::NotRequired)]).enveloped(Envelope {
            effects: vec![Effect::Write],
            risk: Risk::Medium,
            idempotency: Idempotency::NonIdempotent,
            access: Vec::new(),
        }),
    )
    .unattended_above(Risk::Medium);
    let (outcome, sink) = harness.run();

    assert_eq!(
        harness.tools.calls.len(),
        1,
        "at the ceiling the edit runs without asking"
    );
    assert!(approvals(&sink).is_empty(), "{:?}", approvals(&sink));
    assert_eq!(results(&outcome.expect("the run completes")).len(), 1);
}

#[test]
fn a_turn_ceiling_stops_the_loop_and_names_itself() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "a", json!({}))])),
            Ok(asks_for(&[("call-2", "a", json!({}))])),
            Ok(asks_for(&[("call-3", "a", json!({}))])),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    )
    .budgeted(Budget::default().with_max_turns(2));
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("a bound that binds is an outcome");

    assert_eq!(outcome.stop, LoopStop::MaxTurns { limit: 2 });
    assert_eq!(outcome.turns, 2);
}

#[test]
fn an_input_token_ceiling_stops_the_loop() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "a", json!({}))])),
            Ok(asks_for(&[("call-2", "a", json!({}))])),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    )
    .budgeted(Budget {
        max_input_tokens: Some(10),
        ..Budget::default()
    });
    let (outcome, _) = harness.run();
    assert_eq!(
        outcome.expect("bound binds").stop,
        LoopStop::MaxInputTokens {
            limit: 10,
            reported: 10
        }
    );
    assert_eq!(
        harness.model.seen.len(),
        1,
        "equality binds before another turn"
    );
}

#[test]
fn an_output_token_ceiling_stops_the_loop() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "a", json!({}))])),
            Ok(asks_for(&[("call-2", "a", json!({}))])),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    )
    .budgeted(Budget {
        max_output_tokens: Some(5),
        ..Budget::default()
    });
    let (outcome, _) = harness.run();
    assert_eq!(
        outcome.expect("bound binds").stop,
        LoopStop::MaxOutputTokens {
            limit: 5,
            reported: 5
        }
    );
    assert_eq!(
        harness.model.seen.len(),
        1,
        "equality binds before another turn"
    );
}

#[test]
fn a_spend_ceiling_is_refused_before_the_first_request() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("never reached"))]),
        ScriptedTools::new(Vec::new()),
    )
    .budgeted(Budget {
        max_cost_microunits: Some(1_000),
        ..Budget::default()
    });
    let (outcome, _) = harness.run();

    assert_eq!(
        outcome.expect_err("an unenforceable bound refuses"),
        LoopError::Budget(BudgetError::Unenforceable {
            name: "max_cost_microunits"
        })
    );
    assert!(
        harness.model.seen.is_empty(),
        "nothing may be sent once a bound is refused"
    );
}

#[test]
fn cancellation_between_turns_ends_the_run() {
    let cancel = LoopCancel::new();
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "a", json!({}))])),
            Ok(answer("never reached")),
        ])
        .cancelling_after(1, cancel.clone()),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    );
    harness.cancel = cancel;
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("cancellation is an outcome");

    assert!(matches!(outcome.stop, LoopStop::Cancelled { .. }));
    assert_eq!(
        harness.model.seen.len(),
        1,
        "no turn may start after a cancel"
    );
}

#[test]
fn cancellation_between_tool_calls_stops_before_the_next_effect() {
    let cancel = LoopCancel::new();
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(asks_for(&[
            ("call-1", "a", json!({})),
            ("call-2", "a", json!({})),
        ]))]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)])
            .enveloped(writes())
            .cancelling_after(1, cancel.clone()),
    );
    harness.cancel = cancel;
    let (outcome, _) = harness.run();

    assert!(matches!(
        outcome.expect("cancellation is an outcome").stop,
        LoopStop::Cancelled { .. }
    ));
    assert_eq!(
        harness.tools.calls.len(),
        1,
        "the second effect must not happen after a cancel"
    );
}

/// A wall-clock budget wide enough that the loop's own setup cannot spend it before the first
/// call, paired with a call slow enough to spend all of it in one. A budget of a millisecond would
/// race the machine rather than test the loop — and 40 ms raced a shared CI runner, where one
/// scheduling stall between the deadline being set and the first call being checked is enough to
/// skip the call the test expects to see run.
const DEADLINE_MS: u64 = 200;
const SLOW_CALL: Duration = Duration::from_millis(300);

fn deadlined() -> Budget {
    Budget {
        max_duration_ms: Some(DEADLINE_MS),
        ..Budget::default()
    }
}

/// How many calls the conversation carries, and how many answers. They must be equal: a
/// `function_call` replayed without its output is a provider error on the next turn.
fn calls_and_results(outcome: &LoopOutcome) -> (usize, usize) {
    let count =
        |wanted: fn(&Item) -> bool| outcome.items.iter().filter(|item| wanted(item)).count();
    (
        count(|item| matches!(item, Item::ToolCall(_))),
        count(|item| matches!(item, Item::ToolResult { .. })),
    )
}

#[test]
fn the_deadline_ends_the_run_between_turns() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "slow", json!({}))])),
            Ok(asks_for(&[("call-2", "slow", json!({}))])),
        ]),
        ScriptedTools::new(vec![spec("slow", Approval::NotRequired)]).taking(SLOW_CALL),
    )
    .budgeted(deadlined());
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("a bound that binds is an outcome");

    assert_eq!(
        outcome.stop,
        LoopStop::Deadline {
            limit_ms: DEADLINE_MS
        }
    );
    assert_eq!(
        harness.tools.calls.len(),
        1,
        "no turn may start once the clock has run out"
    );
    assert_eq!(calls_and_results(&outcome), (1, 1));
}

#[test]
fn the_deadline_is_checked_between_calls_in_one_turn() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(asks_for(&[
            ("call-1", "slow", json!({})),
            ("call-2", "slow", json!({})),
            ("call-3", "slow", json!({})),
        ]))]),
        ScriptedTools::new(vec![spec("slow", Approval::NotRequired)])
            .enveloped(writes())
            .taking(SLOW_CALL),
    )
    .budgeted(deadlined());
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("a bound that binds is an outcome");

    assert_eq!(
        outcome.stop,
        LoopStop::Deadline {
            limit_ms: DEADLINE_MS
        }
    );
    assert_eq!(
        harness.tools.calls.len(),
        1,
        "a turn of six slow calls would overshoot the budget six times over"
    );
    assert_eq!(calls_and_results(&outcome), (3, 3));
    let refused: Vec<&str> = outcome
        .items
        .iter()
        .filter_map(|item| match item {
            Item::ToolResult {
                failed: true,
                output,
                ..
            } => output.as_str(),
            _ => None,
        })
        .collect();
    assert_eq!(refused.len(), 2);
    assert!(
        refused.iter().all(|text| text.contains("deadline")),
        "{refused:?}"
    );
}

#[test]
fn the_time_left_on_the_clock_reaches_the_tool_that_runs_the_call() {
    // The deadline check between calls cannot reach into a call already running, so the loop
    // says how long is left and the tool bounds what it starts by that.
    let scripted = || {
        (
            ScriptedModel::new(vec![
                Ok(asks_for(&[("call-1", "a", json!({}))])),
                Ok(answer("done")),
            ]),
            ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
        )
    };

    let (model, tools) = scripted();
    let mut harness = Harness::new(model, tools).budgeted(Budget {
        max_duration_ms: Some(10_000),
        ..Budget::default()
    });
    let _ = harness.run();
    let left = harness.tools.remaining[0].expect("a deadline is a bound on every call");
    assert!(
        left <= Duration::from_millis(10_000) && left > Duration::from_millis(5_000),
        "what is left is the budget less what the loop has spent: {left:?}"
    );

    let (model, tools) = scripted();
    let mut harness = Harness::new(model, tools);
    let _ = harness.run();
    assert_eq!(
        harness.tools.remaining,
        vec![None],
        "no deadline is no bound, not a bound of zero"
    );
}

#[test]
fn no_deadline_means_no_deadline_stop() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[
                ("call-1", "slow", json!({})),
                ("call-2", "slow", json!({})),
            ])),
            Ok(answer("both ran")),
        ]),
        ScriptedTools::new(vec![spec("slow", Approval::NotRequired)]).taking(SLOW_CALL),
    );
    let (outcome, _) = harness.run();

    assert_eq!(outcome.expect("completes").stop, LoopStop::Completed);
    assert_eq!(
        harness.tools.calls.len(),
        2,
        "an absent bound binds nothing"
    );
}

#[test]
fn unreported_usage_stays_unknown_rather_than_zero() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(TurnOutcome {
            stop_reason: StopReason::EndTurn,
            items: vec![Item::assistant("no usage here")],
            usage: None,
        })]),
        ScriptedTools::new(Vec::new()),
    );
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("completes");

    assert!(outcome.usage.is_empty());
    assert_eq!(outcome.total_tokens(), None);
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| matches!(event, LoopEvent::Usage(_))),
        "an unreported turn must produce no usage event"
    );
}

#[test]
fn a_token_ceiling_stops_by_name_when_a_turn_omits_usage() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(TurnOutcome {
            stop_reason: StopReason::EndTurn,
            items: vec![Item::assistant("no usage here")],
            usage: None,
        })]),
        ScriptedTools::new(Vec::new()),
    )
    .budgeted(Budget {
        max_input_tokens: Some(100),
        ..Budget::default()
    });
    let (outcome, _) = harness.run();

    assert_eq!(
        outcome.expect("an unobservable budget is an outcome").stop,
        LoopStop::BudgetUnobservable {
            name: "max_input_tokens".to_owned(),
            reason: "a model request omitted usage".to_owned(),
        }
    );
}

#[test]
fn an_oversized_tool_result_is_refused_rather_than_truncated() {
    let huge = json!("x".repeat(MAX_TOOL_RESULT_BYTES));
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "a", json!({}))])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)])
            .answering("a", ToolOutcome::ok(huge)),
    );
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("the run recovers");

    let Some(Item::ToolResult { failed, output, .. }) = outcome
        .items
        .iter()
        .find(|item| matches!(item, Item::ToolResult { .. }))
    else {
        panic!("the model is told what happened");
    };
    assert!(failed);
    let text = output.as_str().expect("the refusal is text");
    assert!(text.contains("bound"), "{text}");
    assert!(
        text.len() < MAX_TOOL_RESULT_BYTES,
        "the oversized payload must not be forwarded"
    );
}

#[test]
fn oversized_arguments_never_reach_the_tool() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "a",
                json!({"blob": "x".repeat(MAX_TOOL_ARGUMENT_BYTES)}),
            )])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    );
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("the run recovers");

    assert!(harness.tools.calls.is_empty());
    assert!(
        outcome
            .items
            .iter()
            .any(|item| matches!(item, Item::ToolResult { failed: true, .. }))
    );
    // The run must survive one bad call. Retaining the payload would make every later turn refuse
    // for the same reason, so the second request is where the recovery actually shows.
    assert_eq!(harness.model.seen.len(), 2);
    let replayed = harness.model.seen[1]
        .items
        .iter()
        .find_map(Item::as_tool_call)
        .expect("the call is still visible to the model");
    assert!(
        replayed.arguments.get("omitted").is_some(),
        "{:?}",
        replayed.arguments
    );
    assert!(harness.model.seen[1].validate().is_ok());
}

#[test]
fn a_wire_failure_ends_the_run_as_an_error() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Err(WireError::new(
            WireErrorCode::Unauthorized,
            "the key was rejected",
            false,
        ))]),
        ScriptedTools::new(Vec::new()),
    );
    let (outcome, _) = harness.run();
    let LoopError::Wire(error) = outcome.expect_err("a wire failure is an error") else {
        panic!("a wire failure surfaces as a wire error");
    };
    assert_eq!(error.code, WireErrorCode::Unauthorized);
}

#[test]
fn a_provider_cut_turn_is_reported_with_its_reason() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(TurnOutcome {
            stop_reason: StopReason::MaxOutputTokens,
            items: vec![Item::assistant("truncat")],
            usage: None,
        })]),
        ScriptedTools::new(Vec::new()),
    );
    let (outcome, _) = harness.run();
    assert_eq!(
        outcome.expect("completes").stop,
        LoopStop::ProviderIncomplete {
            reason: "max_output_tokens".to_owned()
        }
    );
}

#[test]
fn the_per_turn_output_bound_reaches_the_provider() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("ok"))]),
        ScriptedTools::new(Vec::new()),
    )
    .budgeted(Budget {
        max_output_tokens_per_turn: Some(512),
        ..Budget::default()
    });
    let _ = harness.run();
    assert_eq!(harness.model.seen[0].max_output_tokens, Some(512));
}

#[test]
fn the_run_announces_what_it_published_before_the_first_turn() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("ok"))]),
        ScriptedTools::new(vec![
            spec("a", Approval::NotRequired),
            spec("b", Approval::Required),
        ]),
    );
    let (_, sink) = harness.run();
    let Some(LoopEvent::Started {
        published_tools, ..
    }) = sink.events().first()
    else {
        panic!("the run starts by saying what it can do");
    };
    assert_eq!(published_tools, &vec![tool_name("a"), tool_name("b")]);
}

#[test]
fn the_run_also_announces_what_it_asked_for_and_was_not_given() {
    // The other half of the same sentence, and the one that was missing. What a run *has* was
    // always in this event; what it was *refused* was in nothing at all, so a catalogue short of
    // the one tool the task needed looked exactly like a catalogue nobody had asked more of.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("ok"))]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    );
    harness.config = harness.config.clone().with_withheld(vec![Withheld {
        tool: "run".to_owned(),
        reason: "`exec.argv-only` must be true and this machine says nothing.".to_owned(),
    }]);
    let (_, sink) = harness.run();
    let Some(LoopEvent::Started { withheld, .. }) = sink.events().first() else {
        panic!("the run starts by saying what it can do");
    };
    assert_eq!(withheld.len(), 1, "{withheld:?}");
    assert_eq!(withheld[0].tool, "run");
    assert!(
        withheld[0].reason.contains("exec.argv-only"),
        "{withheld:?}"
    );
}

#[test]
fn a_run_refused_nothing_announces_nothing() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("ok"))]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    );
    let (_, sink) = harness.run();
    let Some(LoopEvent::Started { withheld, .. }) = sink.events().first() else {
        panic!("the run starts by saying what it can do");
    };
    assert!(withheld.is_empty(), "absence stays absence: {withheld:?}");
}

#[test]
fn a_cancelled_model_read_is_an_outcome_rather_than_a_failure() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Err(WireError::cancelled())]),
        ScriptedTools::new(Vec::new()),
    );
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("a cancelled read is not an error");

    assert!(matches!(outcome.stop, LoopStop::Cancelled { .. }));
    assert!(
        sink.events()
            .iter()
            .any(|event| matches!(event, LoopEvent::Finished { stop, .. } if matches!(stop, LoopStop::Cancelled { .. }))),
        "the terminal event says cancelled: {:?}",
        sink.events()
    );
}

#[test]
fn a_cancelled_read_after_a_tool_call_still_reports_the_work_done() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "a", json!({}))])),
            Err(WireError::cancelled()),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    );
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("a cancelled read is not an error");

    assert!(matches!(outcome.stop, LoopStop::Cancelled { .. }));
    assert_eq!(outcome.turns, 2);
    assert_eq!(
        harness.tools.calls.len(),
        1,
        "the call it did make is not erased"
    );
    assert_eq!(outcome.total_tokens(), Some((10, 5)));
}

// --- what a run cost ------------------------------------------------------------------------

/// A card that prices the scripted model at round numbers: $1/Mtok in, $2/Mtok out.
fn scripted_card() -> RateCard {
    RateCard::parse(
        r#"{
            "source": "a table the test declares",
            "as_of": "2026-08-24",
            "models": {"scripted-model": {
                "input_usd_per_mtok": 1.0,
                "cached_input_usd_per_mtok": 0.1,
                "output_usd_per_mtok": 2.0
            }}
        }"#,
    )
    .expect("a valid card")
}

fn costs(sink: &VecLoopSink) -> Vec<u64> {
    sink.events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::Cost { micro_usd, .. } => Some(*micro_usd),
            _ => None,
        })
        .collect()
}

#[test]
fn a_priced_run_states_what_it_cost_and_names_the_rates_that_priced_it() {
    // The figure a comparison actually needs. A subscription reports no price on the wire and the
    // run states one anyway, exactly as every other harness in the matrix does.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "a", json!({}))])),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    )
    .priced(scripted_card());
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("the run completes");

    // Two turns of 10 in / 5 out: 10 µ$ + 10 µ$ input, 10 µ$ + 10 µ$ output.
    assert_eq!(costs(&sink), vec![20, 20]);
    assert_eq!(
        outcome.cost_micro_usd,
        Some(40),
        "the total is the sum of the turns a reader can see"
    );
    assert!(
        sink.events().iter().any(|event| matches!(
            event,
            LoopEvent::Rates { as_of, .. } if as_of == "2026-08-24"
        )),
        "the record carries the card that priced it"
    );
}

#[test]
fn an_unpriced_run_reports_no_cost_at_all_rather_than_a_zero() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("done"))]),
        ScriptedTools::new(Vec::new()),
    );
    let (outcome, sink) = harness.run();

    assert_eq!(outcome.expect("completes").cost_micro_usd, None);
    assert!(costs(&sink).is_empty());
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| matches!(event, LoopEvent::Rates { .. })),
        "no card, nothing to attribute a figure to"
    );
}

#[test]
fn a_card_that_misses_this_model_says_so_by_name_instead_of_going_quiet() {
    // Silence would be indistinguishable from a run that cost nothing, and the one number an
    // evaluation compares arms on would be missing without anybody noticing.
    let card = RateCard::parse(
        r#"{"source": "s", "as_of": "2026-08-24", "models": {"some-other-model": {
            "input_usd_per_mtok": 1.0, "cached_input_usd_per_mtok": 0.1, "output_usd_per_mtok": 2.0
        }}}"#,
    )
    .expect("a valid card");
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("done"))]),
        ScriptedTools::new(Vec::new()),
    )
    .priced(card);
    let (outcome, sink) = harness.run();

    assert_eq!(outcome.expect("completes").cost_micro_usd, None);
    let warnings: Vec<_> = sink.warnings().collect();
    let (code, message) = warnings.first().expect("one warning");
    assert_eq!(*code, "unpriced-model");
    assert!(message.contains("scripted-model"), "{message}");
    assert!(
        message.contains("some-other-model"),
        "and it names what the card does price: {message}"
    );
}

#[test]
fn a_spend_ceiling_ends_the_run_once_the_declared_rates_say_it_was_reached() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "a", json!({}))])),
            Ok(asks_for(&[("call-2", "a", json!({}))])),
            Ok(answer("never reached")),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    )
    .priced(scripted_card())
    .budgeted(Budget {
        max_cost_microunits: Some(15),
        ..Budget::default()
    });
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("a budget that binds is an outcome, not an error");

    assert_eq!(
        outcome.stop,
        LoopStop::MaxCost {
            limit_micro_usd: 15,
            spent_micro_usd: 20,
        }
    );
    assert_eq!(outcome.turns, 1, "it stops after the turn that crossed");
}

#[test]
fn a_spend_ceiling_on_a_run_that_cannot_price_itself_is_refused_before_the_first_request() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("never sent"))]),
        ScriptedTools::new(Vec::new()),
    )
    .budgeted(Budget {
        max_cost_microunits: Some(1_000),
        ..Budget::default()
    });
    let (outcome, _) = harness.run();

    assert!(matches!(
        outcome.expect_err("refused"),
        LoopError::Budget(BudgetError::Unenforceable {
            name: "max_cost_microunits"
        })
    ));
    assert!(
        harness.model.seen.is_empty(),
        "nothing was spent proving the ceiling could not be kept"
    );
}

// --- keeping the conversation inside a bound ----------------------------------------------------

/// Refuses an item list a provider would refuse, and says which rule it broke.
///
/// The three shapes both wires reject, in one place: a `function_call` with no output after it, an
/// output with no call before it, and a reasoning item the item it precedes does not follow. Every
/// compaction has to leave a list that satisfies all three — a fold that does not costs the run a
/// 400 on the very next turn, which is the failure compaction exists to avoid — so this is what a
/// compaction test asserts, rather than the particular index it happened to expect.
fn assert_replayable(items: &[Item], what: &str) {
    for (index, item) in items.iter().enumerate() {
        match item {
            Item::ToolCall(call) => assert!(
                items[index + 1..].iter().any(
                    |later| matches!(later, Item::ToolResult { call_id, .. } if call_id == &call.call_id)
                ),
                "{what}: `{}` is a call with no result after it, and a `function_call` replayed \
                 without its output is a provider error on the next turn",
                call.call_id
            ),
            Item::ToolResult { call_id, .. } => assert!(
                items[..index].iter().any(
                    |earlier| matches!(earlier, Item::ToolCall(call) if &call.call_id == call_id)
                ),
                "{what}: `{call_id}` is a result with no call before it, and nothing says what it \
                 answers"
            ),
            Item::Opaque { .. } => assert!(
                index + 1 < items.len(),
                "{what}: the record ends on a reasoning item, and the Responses route requires \
                 the item one precedes"
            ),
            Item::UserText { .. } | Item::AssistantText { .. } => {}
        }
    }
}

/// One call of the tool the compaction fixtures use.
fn a_call(id: &str, arguments: Value) -> Item {
    Item::ToolCall(ToolCall {
        call_id: call_id(id),
        name: tool_name("a"),
        arguments,
    })
}

/// The answer to `id`, `bytes` of it.
fn an_answer(id: &str, bytes: usize) -> Item {
    Item::result(
        call_id(id),
        ToolOutcome::ok(json!({ "text": "y".repeat(bytes) })),
    )
}

/// A provider reasoning item, the kind carried verbatim across a tool round trip.
fn reasoning(bytes: usize) -> Item {
    Item::Opaque {
        wire: wire(),
        payload: json!({ "encrypted": "r".repeat(bytes) }),
    }
}

/// A conversation with `n` big tool results, in call order.
fn fat_conversation(n: usize) -> Vec<Item> {
    let mut items = vec![Item::user("do the thing")];
    for index in 0..n {
        items.push(Item::ToolCall(ToolCall {
            call_id: call_id(&format!("c{index}")),
            name: tool_name("a"),
            arguments: json!({}),
        }));
        items.push(Item::result(
            call_id(&format!("c{index}")),
            ToolOutcome::ok(json!({ "text": "x".repeat(40_000) })),
        ));
    }
    items
}

fn elided(items: &[Item]) -> usize {
    items
        .iter()
        .filter(|item| matches!(item, Item::ToolResult { output, .. } if output.get("elided").is_some()))
        .count()
}

#[test]
fn a_conversation_past_its_bound_loses_the_oldest_tool_output_and_nothing_else() {
    // A stateless loop replays everything every turn, so cost is quadratic in length and the
    // context window is a hard ceiling on it. What goes is bytes of old tool output - a file read
    // whose contents were then edited is dead weight from the moment the edit landed.
    let mut items = fat_conversation(10);
    let before = items.len();
    let mut sink = VecLoopSink::new();
    super::compact(&mut items, &mut sink);

    assert_eq!(items.len(), before, "every item stays; only payloads go");
    assert!(elided(&items) > 0, "something was elided");
    assert_replayable(&items, "the compacted conversation");
    assert!(
        matches!(&items[0], Item::UserText { .. }),
        "the request itself is never touched"
    );
    assert_eq!(
        sink.warnings()
            .filter(|(code, _)| *code == "conversation-compacted")
            .count(),
        1,
        "a model that suddenly cannot see a file it read has a right to a reason"
    );
}

#[test]
fn the_most_recent_results_survive_however_long_the_conversation_is() {
    // The model is usually working from what it just read. Eliding that makes it read the file
    // again, which costs more than the elision saved.
    let mut items = fat_conversation(10);
    super::compact(&mut items, &mut NullLoopSink);
    assert_replayable(&items, "the compacted conversation");

    let intact: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item, Item::ToolResult { output, .. } if output.get("elided").is_none()))
        .map(|(index, _)| index)
        .collect();
    assert!(
        !intact.is_empty(),
        "a model that cannot see the result of the call it just made is stuck"
    );
    let last = items.len() - 1;
    assert!(intact.contains(&last), "the newest result above all");
}

/// A conversation of `n` results of `bytes` each, in call order.
///
/// Separate from [`fat_conversation`] because the newest [`super::KEPT_RESULT_BYTES`] are never
/// elided: a fixture of few very large results tests the floor, and this one tests the target.
fn conversation_of(n: usize, bytes: usize) -> Vec<Item> {
    let mut items = vec![Item::user("do the thing")];
    for index in 0..n {
        items.push(Item::ToolCall(ToolCall {
            call_id: call_id(&format!("c{index}")),
            name: tool_name("a"),
            arguments: json!({}),
        }));
        items.push(Item::result(
            call_id(&format!("c{index}")),
            ToolOutcome::ok(json!({ "text": "x".repeat(bytes) })),
        ));
    }
    items
}

/// The conversation's serialized size, the way `compact` measures it.
fn measured(items: &[Item]) -> usize {
    items
        .iter()
        .map(|item| serde_json::to_string(item).map_or(0, |json| json.len()))
        .sum()
}

#[test]
fn a_compaction_goes_below_the_bound_so_the_next_result_does_not_pay_for_another_one() {
    // Measured on a live 24-turn run: compacting to the bound fired twice, and the two turns after
    // replayed 43,203 and 58,448 tokens uncached — about a third of that run's bill. The bytes
    // dropped are the same either way; what changes is how many times the cache is thrown away.
    let mut items = conversation_of(40, 8_000);
    super::compact(&mut items, &mut NullLoopSink);
    assert_replayable(&items, "the compacted conversation");

    let after = measured(&items);
    assert!(
        after <= super::COMPACTED_TARGET_BYTES,
        "compaction stopped at {after} bytes, above the {} low-water mark",
        super::COMPACTED_TARGET_BYTES
    );
}

#[test]
fn one_more_large_result_after_a_compaction_does_not_trigger_a_second_one() {
    // The defect this exists to catch: stopping at the bound leaves the next result to cross it
    // again, and the second prefix rewrite costs a whole uncached replay.
    let mut items = conversation_of(40, 8_000);
    super::compact(&mut items, &mut NullLoopSink);

    items.push(Item::ToolCall(ToolCall {
        call_id: call_id("next"),
        name: tool_name("a"),
        arguments: json!({}),
    }));
    items.push(Item::result(
        call_id("next"),
        ToolOutcome::ok(json!({ "text": "x".repeat(40_000) })),
    ));

    let mut sink = VecLoopSink::new();
    super::compact(&mut items, &mut sink);
    assert_replayable(&items, "the compacted conversation");
    assert_eq!(
        sink.warnings()
            .filter(|(code, _)| *code == "conversation-compacted")
            .count(),
        0,
        "one 40kB result after a compaction must fit under the bound, not buy another rewrite"
    );
}

#[test]
fn the_newest_results_are_kept_by_size_so_the_floor_never_sits_above_the_target() {
    // A live run kept six recent results whole, they came to about 130kB, and compaction could then
    // only reach 177,915 bytes — above the low-water mark and barely under the bound. It fired
    // again on the next result, and again on the one after.
    let mut items = fat_conversation(10);
    super::compact(&mut items, &mut NullLoopSink);
    assert_replayable(&items, "the compacted conversation");

    let after = measured(&items);
    assert!(
        after <= super::COMPACTED_TARGET_BYTES,
        "40kB results must not hold the floor above the target: left {after} bytes"
    );
    let intact = items
        .iter()
        .filter(
            |item| matches!(item, Item::ToolResult { output, .. } if output.get("elided").is_none()),
        )
        .count();
    assert!(intact >= 1, "the newest result survives whatever its size");
}

#[test]
fn a_conversation_inside_its_bound_is_left_exactly_alone() {
    // The threshold is a bound, not a target. Compaction rewrites the prefix and the turn after one
    // pays full rate for everything, so doing it when nothing needs it would buy a cache miss for
    // no saving at all.
    let mut items = fat_conversation(1);
    let before = items.clone();
    let mut sink = VecLoopSink::new();
    super::compact(&mut items, &mut sink);

    assert_eq!(items, before);
    assert_replayable(&items, "the untouched conversation");
    assert_eq!(sink.warnings().count(), 0);
}

#[test]
fn eliding_is_monotone_so_the_prefix_settles_again() {
    // An item elided once stays elided. That is what makes the cost of a compaction one uncached
    // turn rather than one per turn from then on.
    let mut items = fat_conversation(10);
    super::compact(&mut items, &mut NullLoopSink);
    let after_first = items.clone();
    super::compact(&mut items, &mut NullLoopSink);

    assert_eq!(items, after_first, "a second pass changes nothing");
    assert_replayable(&items, "the twice-compacted conversation");
}

// --- a turn whose stream broke ------------------------------------------------------------------

/// Every retry the run announced, as (turn, attempt).
fn retried(sink: &VecLoopSink) -> Vec<(u64, u32)> {
    sink.events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::TurnRetried { turn, attempt, .. } => Some((*turn, *attempt)),
            _ => None,
        })
        .collect()
}

fn broke() -> Result<TurnOutcome, WireError> {
    Err(WireError::transport("the connection dropped mid-stream"))
}

#[test]
fn a_stream_that_broke_after_its_first_byte_is_attempted_again_instead_of_ending_the_run() {
    // A network blip on turn 20 of a $1 run used to lose the run: a wire will not retry once it has
    // emitted anything — a second attempt would append a second copy of text a person already read
    // — and the loop mapped every `Err` to `LoopError::Wire` and exited.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![broke(), Ok(answer("the second attempt"))]).streaming("half an "),
        ScriptedTools::new(Vec::new()),
    )
    .impatient();
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("a retriable failure is not the end of a run");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.text, "the second attempt");
    assert_eq!(
        retried(&sink),
        vec![(1, 1)],
        "one retry, announced on turn one"
    );
    assert_eq!(
        outcome.turns, 1,
        "a second attempt at a turn is not a second turn"
    );
    assert_eq!(harness.model.seen.len(), 2);
    assert_eq!(
        harness.model.seen[0].items, harness.model.seen[1].items,
        "a failed turn leaves the conversation untouched, so the retry is the same request"
    );
}

#[test]
fn what_streamed_for_a_retried_turn_is_named_so_a_renderer_can_discard_it() {
    // Without the event a person sees half an answer, then a whole answer, and no reason for
    // either. The wire cannot help: it is the one thing it deliberately refuses to decide.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![broke(), Ok(answer("done"))]).streaming("half an "),
        ScriptedTools::new(Vec::new()),
    )
    .impatient();
    let (_, sink) = harness.run();

    let Some(LoopEvent::TurnRetried { reason, .. }) = sink
        .events()
        .iter()
        .find(|event| matches!(event, LoopEvent::TurnRetried { .. }))
    else {
        panic!("the retry is in the record: {:?}", sink.events());
    };
    assert!(reason.contains("dropped mid-stream"), "{reason}");
    assert_eq!(
        sink.text(),
        "half an half an ",
        "both attempts streamed, which is exactly why the event has to say so"
    );
}

#[test]
fn a_failure_the_wire_calls_final_is_never_attempted_again() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Err(WireError::unauthorized("the key was rejected")),
            Ok(answer("never reached")),
        ]),
        ScriptedTools::new(Vec::new()),
    )
    .impatient();
    let (outcome, sink) = harness.run();

    let LoopError::Wire(error) = outcome.expect_err("a final failure ends the run") else {
        panic!("a wire failure surfaces as a wire error");
    };
    assert_eq!(error.code, WireErrorCode::Unauthorized);
    assert!(retried(&sink).is_empty(), "{:?}", retried(&sink));
    assert_eq!(
        harness.model.seen.len(),
        1,
        "no amount of waiting changes a rejected key"
    );
}

#[test]
fn a_turn_gets_three_further_attempts_and_then_the_failure_it_had_stands() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            broke(),
            broke(),
            broke(),
            broke(),
            Ok(answer("never reached")),
        ]),
        ScriptedTools::new(Vec::new()),
    )
    .impatient();
    let (outcome, sink) = harness.run();

    assert!(matches!(
        outcome.expect_err("the run ends with the failure it already had"),
        LoopError::Wire(_)
    ));
    assert_eq!(retried(&sink), vec![(1, 1), (1, 2), (1, 3)]);
    assert_eq!(
        harness.model.seen.len(),
        1 + MAX_TURN_RETRIES as usize,
        "the first attempt and MAX_TURN_RETRIES more, and no fifth"
    );
}

#[test]
fn cancelling_during_the_pause_between_attempts_ends_the_run_as_cancelled() {
    // The pause is slept in slices for exactly this: a person who presses Ctrl-C during an
    // eight-second back-off should not wait out the eight seconds to be heard.
    let cancel = LoopCancel::new();
    let ticker = cancel.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        ticker.cancel();
    });
    let mut harness = Harness::new(
        ScriptedModel::new(vec![broke(), Ok(answer("never reached"))]),
        ScriptedTools::new(Vec::new()),
    )
    .retrying_after(Duration::from_secs(30));
    harness.cancel = cancel;
    let started = std::time::Instant::now();
    let (outcome, sink) = harness.run();
    stopper.join().expect("the canceller finishes");
    let waited = started.elapsed();

    assert!(matches!(
        outcome.expect("cancellation is an outcome").stop,
        LoopStop::Cancelled { .. }
    ));
    assert_eq!(
        retried(&sink),
        vec![(1, 1)],
        "the retry was announced, and then the pause was interrupted"
    );
    assert_eq!(
        harness.model.seen.len(),
        1,
        "the second attempt never started"
    );
    assert!(
        waited < Duration::from_secs(5),
        "the pause was cut short, not waited out: {waited:?}"
    );
}

// --- the pure calls of one turn -----------------------------------------------------------------

/// The call ids answered, in the order the conversation carries them.
fn answered(outcome: &LoopOutcome) -> Vec<String> {
    outcome
        .items
        .iter()
        .filter_map(|item| match item {
            Item::ToolResult { call_id, .. } => Some(call_id.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn the_pure_calls_of_one_turn_go_to_the_port_together_and_a_write_is_a_barrier() {
    // N independent reads used to cost N round trips of tool latency, one after another, for no
    // reason: two reads of the same file in either order read the same bytes. A write between them
    // does not, so it ends the group — and a group of one goes down the single-call path unchanged.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[
                ("call-1", "read", json!({})),
                ("call-2", "read", json!({})),
                ("call-3", "write", json!({})),
                ("call-4", "read", json!({})),
            ])),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(vec![
            spec("read", Approval::NotRequired),
            spec("write", Approval::NotRequired),
        ])
        .enveloping("write", writes()),
    );
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("the turn completes");

    assert_eq!(
        harness.tools.batches,
        vec![2],
        "the two leading reads, and nothing else"
    );
    assert_eq!(harness.tools.calls.len(), 4, "every call runs exactly once");
    assert_eq!(
        answered(&outcome),
        vec!["call-1", "call-2", "call-3", "call-4"],
        "outcomes are positional, and the order the model asked in is the order it reads"
    );
    let requested: Vec<&str> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::ToolRequested(call) => Some(call.call_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        requested,
        vec!["call-1", "call-2", "call-3", "call-4"],
        "a batch is still announced call by call, before any of it runs"
    );
}

#[test]
fn a_call_that_would_ask_a_person_is_never_folded_into_a_batch() {
    // A batch cannot stop halfway to ask, and a gate that ran the call and asked afterwards would
    // not be a gate.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[
                ("call-1", "run", json!({})),
                ("call-2", "run", json!({})),
            ])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("run", Approval::NotRequired)]).enveloped(starts_a_process()),
    );
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("a denial is not a failure");

    assert!(
        harness.tools.batches.is_empty(),
        "{:?}",
        harness.tools.batches
    );
    assert!(harness.tools.calls.is_empty(), "and neither call ran");
    assert_eq!(
        approvals(&sink),
        vec![
            "asked call-1",
            "call-1 approved=false",
            "asked call-2",
            "call-2 approved=false"
        ]
    );
    assert_eq!(results(&outcome).len(), 2);
}

#[test]
fn a_port_that_answers_a_different_number_of_outcomes_is_not_trusted_with_any_of_them() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[
                ("call-1", "read", json!({})),
                ("call-2", "read", json!({})),
            ])),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]).miscounting(1),
    );
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("the run recovers");

    assert_eq!(harness.tools.batches, vec![2]);
    assert_eq!(
        sink.warnings()
            .filter(|(code, _)| *code == "batch-miscounted")
            .count(),
        1,
        "said by name: outcomes are positional, so one answer for two calls matches neither"
    );
    assert_eq!(
        calls_and_results(&outcome),
        (2, 2),
        "every call still gets exactly one answer"
    );
    assert_eq!(
        harness.tools.calls.len(),
        4,
        "two in the batch nobody could read, two run again on their own"
    );
}

#[test]
fn a_cancel_raised_inside_a_batch_is_honoured_before_the_next_call() {
    // Cancel and the deadline are checked in front of a group and behind it, never between its
    // calls: the group goes to the port in one call and there is no "between". What that must not
    // cost is an effect after the cancel, and it does not — the group is pure reads by
    // construction, and everything after it is refused without running.
    let cancel = LoopCancel::new();
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(asks_for(&[
            ("call-1", "read", json!({})),
            ("call-2", "read", json!({})),
            ("call-3", "write", json!({})),
            ("call-4", "read", json!({})),
        ]))]),
        ScriptedTools::new(vec![
            spec("read", Approval::NotRequired),
            spec("write", Approval::NotRequired),
        ])
        .enveloping("write", writes())
        .cancelling_after(1, cancel.clone()),
    );
    harness.cancel = cancel;
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("cancellation is an outcome");

    assert!(matches!(outcome.stop, LoopStop::Cancelled { .. }));
    assert_eq!(harness.tools.batches, vec![2]);
    assert_eq!(
        harness.tools.calls.len(),
        2,
        "the group already handed over finished; the write after it never started"
    );
    assert_eq!(
        calls_and_results(&outcome),
        (4, 4),
        "every call the model made is answered, or the run could not be resumed at all"
    );
    let told = results(&outcome);
    assert!(
        !told[0].0 && !told[1].0,
        "the group that was already running answered normally: {told:?}"
    );
    assert!(
        told[2..]
            .iter()
            .all(|(failed, text)| *failed && text.contains("cancelled before this call ran")),
        "{told:?}"
    );
}

#[test]
fn an_oversized_result_from_a_batch_is_refused_by_name_like_any_other() {
    // A bound a faster path can get around is not a bound.
    let huge = json!("x".repeat(MAX_TOOL_RESULT_BYTES));
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[
                ("call-1", "read", json!({})),
                ("call-2", "read", json!({})),
            ])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)])
            .answering("read", ToolOutcome::ok(huge)),
    );
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("the run recovers");

    assert_eq!(harness.tools.batches, vec![2]);
    let told = results(&outcome);
    assert_eq!(told.len(), 2);
    assert!(
        told.iter()
            .all(|(failed, text)| *failed && text.contains("bound")),
        "{told:?}"
    );
    assert!(
        told.iter()
            .all(|(_, text)| text.len() < MAX_TOOL_RESULT_BYTES),
        "the oversized payload must not be forwarded"
    );
}

// --- an entry called by its own name ------------------------------------------------------------

#[test]
fn an_entry_called_by_its_bare_name_is_routed_to_it_rather_than_refused() {
    // Measured on a live run: 10 of 82 tool calls (12.2 %) named a catalogue entry directly instead
    // of the verb that publishes it — `file_read`, `dir_list`, `run` — and every one came back
    // `unpublished-tool`. A dead turn each, re-learnt per state.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "file_read",
                json!({"path": "README.md"}),
            )])),
            Ok(answer("the file says hello")),
        ]),
        ScriptedTools::new(vec![spec("tool_invoke", Approval::NotRequired)])
            .routing(spec("file_read", Approval::NotRequired)),
    );
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("the run completes");

    assert_eq!(
        harness.tools.calls.len(),
        1,
        "the entry the port can reach was reached"
    );
    assert_eq!(
        sink.warnings().map(|(code, _)| code).collect::<Vec<_>>(),
        vec!["unpublished-tool-routed"],
        "routed, and still counted: the waste stays measurable"
    );
    let told = results(&outcome);
    assert_eq!(told.len(), 1);
    assert!(!told[0].0, "the call ran and did not fail");
}

#[test]
fn a_routed_entry_meets_exactly_the_gate_the_verb_would_have_met() {
    // Routing does not widen what the turn admits: the entry was already reachable through the
    // verb, and the decision is the same decision on the same spec.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "run",
                json!({"argv": ["rm", "-rf", "/"]}),
            )])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("tool_invoke", Approval::NotRequired)]).routing(ToolSpec {
            envelope: starts_a_process(),
            ..spec("run", Approval::NotRequired)
        }),
    );
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("a denial is not a failure");

    assert!(
        harness.tools.calls.is_empty(),
        "the effect must not happen before the decision"
    );
    assert_eq!(
        approvals(&sink),
        vec!["asked call-1", "call-1 approved=false"]
    );
    assert!(results(&outcome)[0].1.contains("not approved"));
}

// --- compaction against a declared context window -----------------------------------------------

/// Every compaction the run reported, as (elided results, summarised items, summary turn spent).
fn compacted(sink: &VecLoopSink) -> Vec<(usize, usize, bool)> {
    sink.events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::Compacted {
                elided_results,
                summarised_items,
                summary_turn,
                ..
            } => Some((*elided_results, *summarised_items, *summary_turn)),
            _ => None,
        })
        .collect()
}

/// A turn that asks for two calls the tools answer at length.
fn two_fat_calls() -> TurnOutcome {
    asks_for(&[("call-1", "a", json!({})), ("call-2", "a", json!({}))])
}

fn fat_answer() -> ToolOutcome {
    ToolOutcome::ok(json!({ "text": "x".repeat(6_000) }))
}

#[test]
fn a_declared_window_compacts_on_the_count_the_provider_reported() {
    // The byte rule is 192 KiB — about 50k tokens — so about 60 % of a 128k window was unreachable
    // and a longer run met the provider's wall as a hard error. This conversation is 12 kB and
    // would never have crossed it; what fires is the provider's own count of the window.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(reporting(two_fat_calls(), 85_000)),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]).answering("a", fat_answer()),
    )
    .windowed(100_000);
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("the run completes");

    assert_eq!(
        compacted(&sink),
        vec![(1, 0, false)],
        "one old result elided, and no turn spent: the weight was in tool output"
    );
    assert_eq!(
        elided(&harness.model.seen[1].items),
        1,
        "and the request the model saw is the compacted one"
    );
    assert_replayable(&harness.model.seen[1].items, "the compacted request");
    assert_eq!(outcome.turns, 2);
}

#[test]
fn a_window_the_run_is_nowhere_near_is_left_exactly_alone() {
    // The threshold is a bound, not a target: compaction rewrites the prefix a prompt cache is
    // keyed on, so doing it when nothing needs it buys a cache miss for no saving at all.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(reporting(two_fat_calls(), 10_000)),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]).answering("a", fat_answer()),
    )
    .windowed(100_000);
    let (_, sink) = harness.run();

    assert!(compacted(&sink).is_empty(), "{:?}", compacted(&sink));
    assert_eq!(elided(&harness.model.seen[1].items), 0);
    assert_replayable(&harness.model.seen[1].items, "the untouched request");
}

/// An assistant turn heavy enough that elision cannot reach its weight: 12 kB of argument.
fn long_argument() -> String {
    "the plan, at length. ".repeat(600)
}

#[test]
fn a_conversation_whose_weight_is_text_is_summarised_and_the_task_survives_it() {
    // Elision only drops tool-result payloads, so a run whose weight is user, assistant or opaque
    // reasoning items had no strategy at all. Folding costs one turn and keeps the facts.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(says_and_asks(
                &long_argument(),
                &[("call-1", "a", json!({}))],
            )),
            Ok(answer(
                "a dense summary of what was asked, decided and done",
            )),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    )
    .windowed(1_000);
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("the run completes");

    assert_eq!(
        compacted(&sink),
        vec![(0, 1, true)],
        "nothing to elide, so a turn was spent folding the argument"
    );

    let next = &harness.model.seen[2];
    assert_replayable(&next.items, "the summarised conversation");
    assert_eq!(
        next.items[0],
        Item::user("do the thing"),
        "the task is what the run is for and is never folded"
    );
    let Item::UserText { text } = &next.items[1] else {
        panic!("the summary stands where the prefix was: {:?}", next.items);
    };
    assert!(text.starts_with(SUMMARY_MARKER), "{text}");
    assert!(text.contains("a dense summary"), "{text}");
    assert!(
        !next
            .items
            .iter()
            .any(|item| matches!(item, Item::AssistantText { text } if text.len() > 8_000)),
        "the argument itself is gone"
    );

    let summarising = &harness.model.seen[1];
    assert!(
        summarising.tools.is_empty(),
        "a turn a person cannot see may not call a tool"
    );
    assert_replayable(&summarising.items, "the summary request");
    assert_eq!(summarising.instructions, super::SUMMARY_INSTRUCTION);
    assert_eq!(
        outcome.turns, 3,
        "every model request consumes the run's turn budget, including a summary"
    );
    assert_eq!(
        outcome.total_tokens(),
        Some((30, 15)),
        "but the provider charged for it like any other turn, so the run counts it"
    );
}

#[test]
fn a_summary_never_folds_a_call_away_from_its_result() {
    // A `function_call` replayed without its output is a provider error on the next turn, and an
    // output replayed without its call is worse: nothing says what it answers.
    let items = vec![
        Item::user("do the thing"),
        Item::assistant("x".repeat(4_000)),
        Item::ToolCall(ToolCall {
            call_id: call_id("c0"),
            name: tool_name("a"),
            arguments: json!({}),
        }),
        Item::result(
            call_id("c0"),
            ToolOutcome::ok(json!({ "text": "y".repeat(3_000) })),
        ),
    ];
    let end = super::fold_end(&items, 50).expect("there is a prefix worth folding");

    assert!(
        !matches!(items[end - 1], Item::ToolCall(_)),
        "the fold stopped at {end}, which would orphan the result after it"
    );
    assert_eq!(end, 2, "the call and its result stay together, outside it");
}

/// The conversation a summary leaves behind: the task, the summary item, then the tail.
fn after_folding(items: &[Item], end: usize) -> Vec<Item> {
    let mut left = items[..super::FIRST_KEPT_ITEM].to_vec();
    left.push(Item::user(format!("{SUMMARY_MARKER}\na summary")));
    left.extend_from_slice(&items[end..]);
    left
}

#[test]
fn a_fold_never_falls_between_a_turns_calls_and_the_results_that_answer_them() {
    // The walk-back stepped over `ToolCall`s alone, and the boundary this conversation puts in
    // front of it stands over no call at all: between the two results. Folded there, B's call goes
    // into the summary without B's result and the tail begins with a result whose call is gone —
    // two provider 400s from one compaction.
    let items = vec![
        Item::user("do the thing"),
        Item::assistant("x".repeat(4_000)),
        a_call("c-a", json!({})),
        a_call("c-b", json!({})),
        an_answer("c-a", 30_000),
        an_answer("c-b", 100),
        Item::assistant("done"),
    ];
    let end = super::fold_end(&items, 1_000).expect("there is a prefix worth folding");

    assert_eq!(
        end, 2,
        "the whole round trip is one group and the fold stops in front of it"
    );
    assert_replayable(&items[super::FIRST_KEPT_ITEM..end], "the folded prefix");
    assert_replayable(&after_folding(&items, end), "what the summary leaves");
}

#[test]
fn a_fold_never_ends_on_the_reasoning_item_whose_call_is_in_the_tail() {
    // A reasoning item the Responses route requires an item to follow, folded away from the call
    // that followed it. The byte walk stops between the two; the group is what puts it back.
    let items = vec![
        Item::user("do the thing"),
        Item::assistant("x".repeat(4_000)),
        reasoning(500),
        a_call("c-a", json!({})),
        an_answer("c-a", 3_000),
    ];
    let end = super::fold_end(&items, 200).expect("there is a prefix worth folding");

    assert_eq!(
        end, 2,
        "the reasoning item belongs to the call it precedes and leaves with it"
    );
    assert_replayable(&after_folding(&items, end), "what the summary leaves");
}

#[test]
fn a_fold_of_a_conversation_that_answers_each_call_before_the_next_stops_between_two_round_trips() {
    // The other shape a provider produces: call, result, call, result, rather than all the calls
    // and then all the results. Each round trip is its own group, so a fold may stop between them
    // — and may not stop inside one, which is where 40 kB of arguments would otherwise put it.
    let items = vec![
        Item::user("do the thing"),
        Item::assistant("x".repeat(4_000)),
        reasoning(200),
        a_call("c-a", json!({})),
        an_answer("c-a", 2_000),
        a_call("c-b", json!({ "plan": "p".repeat(3_000) })),
        an_answer("c-b", 100),
        Item::assistant("done"),
    ];
    let end = super::fold_end(&items, 2_500).expect("there is a prefix worth folding");

    assert_eq!(
        end, 5,
        "the boundary the bytes chose sat between B's call and B's result, and moved back to the \
         start of B's round trip"
    );
    assert_replayable(&items[super::FIRST_KEPT_ITEM..end], "the folded prefix");
    assert_replayable(&after_folding(&items, end), "what the summary leaves");
}

#[test]
fn a_conversation_whose_tail_is_one_unbreakable_group_folds_nothing_rather_than_splitting_it() {
    // Snapping only ever makes a fold smaller, so it can reach the task and leave nothing. That is
    // the right answer: there is no boundary here that a provider would accept.
    let items = vec![
        Item::user("do the thing"),
        a_call("c-a", json!({})),
        an_answer("c-a", 40_000),
    ];

    assert!(
        super::fold_end(&items, 100).is_none(),
        "a summary that cannot be taken without breaking the record is not taken"
    );
}

// --- what a summary turn actually sends ---------------------------------------------------------

/// The one item a summary request carries, as text.
fn summary_text(folded: &[Item]) -> String {
    let items = super::summary_request_items(folded);
    assert_eq!(
        items.len(),
        1,
        "one message, so the first one is `user` on every wire"
    );
    match items.into_iter().next() {
        Some(Item::UserText { text }) => text,
        other => panic!("a summary request is one user item, not {other:?}"),
    }
}

#[test]
fn a_summary_request_is_one_user_item_carrying_the_record_as_text() {
    // Sent as items, the fold began with an assistant-side item and carried `tool_use` and
    // `tool_result` blocks while publishing no tools: on the Messages route that is a 400 twice
    // over, so every compaction on that wire bought a turn that could not be answered.
    let folded = vec![
        Item::assistant("the plan"),
        reasoning(20),
        a_call("c-a", json!({"path": "README.md"})),
        an_answer("c-a", 20),
        Item::user("and also this"),
    ];
    let text = summary_text(&folded);

    assert!(text.contains("[assistant] the plan"), "{text}");
    assert!(
        text.contains(r#"[tool call a {"path":"README.md"}]"#),
        "{text}"
    );
    assert!(text.contains("[tool result c-a "), "{text}");
    assert!(text.contains("[user] and also this"), "{text}");
    assert!(
        text.contains("1 provider reasoning item(s) are not shown"),
        "an omission is stated once, never silent: {text}"
    );
    assert!(
        !text.contains("rrrrrrrrrrrrrrrrrrrr"),
        "and the payload of an opaque item is never rendered"
    );
    assert!(
        text.trim_end().ends_with(super::SUMMARY_INSTRUCTION),
        "the ask comes after the record, where the model reads last"
    );
}

#[test]
fn a_failed_result_reads_as_failed_in_the_transcript() {
    let folded = vec![
        a_call("c-a", json!({})),
        Item::result(call_id("c-a"), ToolOutcome::failed("no such path")),
    ];
    let text = summary_text(&folded);

    assert!(text.contains("[tool result c-a failed "), "{text}");
    assert!(text.contains("no such path"), "{text}");
}

#[test]
fn a_result_too_large_for_the_transcript_says_how_much_of_it_was_cut() {
    let folded = vec![a_call("c-a", json!({})), an_answer("c-a", 20_000)];
    let text = summary_text(&folded);

    assert!(
        text.len() < 20_000,
        "one result must not be able to fill the whole transcript: {} bytes",
        text.len()
    );
    assert!(
        text.contains("bytes were cut"),
        "a shortened result the model reads as complete is what invariant 8 forbids: {text}"
    );
}

#[test]
fn a_transcript_over_its_bound_loses_its_oldest_items_and_says_how_many() {
    // The request that asks for a summary is itself a request. Unbounded, the turn meant to fit
    // the run back inside the window is the one the provider refuses.
    let folded: Vec<Item> = (0..40)
        .map(|turn| Item::assistant(format!("{turn}: {}", "x".repeat(8_000))))
        .collect();
    let text = summary_text(&folded);

    assert!(
        text.len() < SUMMARY_TRANSCRIPT_BYTES + super::SUMMARY_INSTRUCTION.len() + 1_024,
        "{} bytes, over the {SUMMARY_TRANSCRIPT_BYTES} byte bound",
        text.len()
    );
    assert!(
        text.contains("oldest item(s) of this record"),
        "never silently: {}",
        &text[..200.min(text.len())]
    );
    assert!(
        !text.contains("[assistant] 0: "),
        "the oldest goes first, because the newest is what the next turn is about"
    );
    assert!(text.contains("[assistant] 39: "), "and the newest survives");
}

#[test]
fn a_spend_ceiling_the_summary_turn_crosses_ends_the_run_before_the_next_turn() {
    // The ceiling was read only after the next turn's tool calls, so a run overshot by the summary
    // turn *and* a whole conversation turn — the one case where the loop spent money nothing was
    // checking.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(says_and_asks(
                &long_argument(),
                &[("call-1", "a", json!({}))],
            )),
            Ok(answer("a dense summary of what was asked and done")),
            Ok(answer("never reached")),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    )
    .windowed(1_000)
    .priced(scripted_card())
    .budgeted(Budget {
        max_cost_microunits: Some(25),
        ..Budget::default()
    });
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("a budget that binds is an outcome, not an error");

    assert_eq!(
        outcome.stop,
        LoopStop::MaxCost {
            limit_micro_usd: 25,
            spent_micro_usd: 40,
        },
        "20 µ$ for the turn and 20 µ$ for the summary that followed it"
    );
    assert_eq!(
        harness.model.seen.len(),
        2,
        "the turn the summary made room for never started"
    );
    assert_eq!(outcome.turns, 2, "the summary is the second model request");
}

#[test]
fn a_summary_turn_that_fails_on_the_wire_leaves_the_run_alive() {
    // A compaction that fails leaves a conversation that is merely larger than wanted. Ending a
    // long run over that would be the defect this whole change exists to remove.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(says_and_asks(
                &long_argument(),
                &[("call-1", "a", json!({}))],
            )),
            Err(WireError::protocol("the summary went wrong")),
            Ok(answer("done anyway")),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    )
    .windowed(1_000);
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("a failed summary is not a failed run");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.text, "done anyway");
    assert_eq!(
        sink.warnings()
            .filter(|(code, _)| *code == "summary-failed")
            .count(),
        1
    );
    assert_eq!(
        compacted(&sink),
        vec![(0, 0, true)],
        "a turn was spent and nothing was folded, and the record says so"
    );
    assert!(
        harness.model.seen[2]
            .items
            .iter()
            .any(|item| matches!(item, Item::AssistantText { text } if text.len() > 8_000)),
        "the conversation kept the form elision left it in"
    );
    assert_replayable(
        &harness.model.seen[2].items,
        "the conversation a failed summary left",
    );
}

#[test]
fn cancelling_during_the_summary_turn_ends_the_run_as_cancelled() {
    let cancel = LoopCancel::new();
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(says_and_asks(
                &long_argument(),
                &[("call-1", "a", json!({}))],
            )),
            Ok(answer("never used")),
        ])
        .cancelling_after(2, cancel.clone()),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    )
    .windowed(1_000);
    harness.cancel = cancel;
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("cancellation is an outcome");

    assert!(matches!(outcome.stop, LoopStop::Cancelled { .. }));
    assert_replayable(&outcome.items, "the conversation a cancelled summary left");
    assert_eq!(
        harness.model.seen.len(),
        2,
        "the turn the summary was making room for never started"
    );
    assert_eq!(
        outcome.total_tokens(),
        Some((20, 10)),
        "what the summary turn spent is still counted"
    );
}

#[test]
fn a_run_with_no_declared_window_keeps_the_byte_rule_exactly() {
    // 12 kB of argument and a reported 85_000 tokens: the token rule would have compacted twice
    // over. Without a window nothing here is a threshold, and the conversation is untouched.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(reporting(
                says_and_asks(&long_argument(), &[("call-1", "a", json!({}))]),
                85_000,
            )),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    );
    let (_, sink) = harness.run();

    assert!(compacted(&sink).is_empty(), "{:?}", compacted(&sink));
    assert_replayable(
        &harness.model.seen[1].items,
        "the request the byte rule left",
    );
    assert_eq!(
        harness.model.seen.len(),
        2,
        "and no turn was spent on a summary"
    );
}

#[test]
fn a_resumed_run_carries_the_prior_conversation_before_the_new_input() {
    // What `--resume` is: the session's items go in, the new question goes after them, and the
    // first request of the second run already holds both. A loop that started from the input
    // alone would make the model pay to be told what it already knew.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("and this is the follow-up"))]),
        ScriptedTools::new(Vec::new()),
    );
    let mut items = vec![
        Item::user("what does this workspace do?"),
        Item::assistant("it runs an agent loop"),
    ];
    let (outcome, _, _) = harness.run_in(&mut items);
    let outcome = outcome.expect("the resumed run completes");

    let first = &harness.model.seen[0];
    assert_eq!(
        first.items[0],
        Item::user("what does this workspace do?"),
        "the prior conversation comes first: {:?}",
        first.items
    );
    assert_eq!(first.items[1], Item::assistant("it runs an agent loop"));
    assert_eq!(
        first.items[2],
        Item::user("do the thing"),
        "and the new input is one more user item after it"
    );
    assert_eq!(
        items, outcome.items,
        "the caller's vector holds what the run ended with, ready to be saved"
    );
    assert!(items.len() > 3, "including this run's own turn: {items:?}");
}

#[test]
fn a_run_that_fails_on_the_wire_leaves_the_conversation_it_had_with_the_caller() {
    // The twenty-turn run a network blip threw away. `LoopError` carries no items, so a shell
    // that only read the outcome had nothing to save; the vector it lent the loop is what it
    // saves instead.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "a", json!({"path": "README.md"}))])),
            Err(WireError::protocol(
                "the stream stopped speaking the protocol",
            )),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)])
            .answering("a", ToolOutcome::ok(json!({"text": "hello"}))),
    );
    let mut items = vec![Item::user("an earlier question")];
    let (outcome, _, _) = harness.run_in(&mut items);

    assert!(matches!(outcome, Err(LoopError::Wire(_))), "{outcome:?}");
    assert_eq!(
        items[0],
        Item::user("an earlier question"),
        "what was resumed is still there: {items:?}"
    );
    assert!(
        items.iter().any(
            |item| matches!(item, Item::ToolResult { call_id, .. } if call_id.as_str() == "call-1")
        ),
        "and so is the turn that succeeded before the wire broke: {items:?}"
    );
}

#[test]
fn a_run_that_fails_on_the_wire_leaves_the_spend_of_the_turns_it_bought() {
    // The same defect as `absorb_child`, one level up. Turn one's `Usage` and `Cost` events are
    // already on the sink when turn two breaks, so a caller handed the conversation but no figures
    // would file the failed run as free — and the session file is all that is left of the run.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "a", json!({"path": "README.md"}))])),
            Err(WireError::protocol(
                "the stream stopped speaking the protocol",
            )),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)])
            .answering("a", ToolOutcome::ok(json!({"text": "hello"}))),
    )
    .priced(scripted_card());
    let mut items = Vec::new();
    let (outcome, spend, sink) = harness.run_in(&mut items);

    assert!(matches!(outcome, Err(LoopError::Wire(_))), "{outcome:?}");
    // Turns started, which is what `turns` counts everywhere else in this loop — the delegate's
    // failed path included. Turn two was started and broke; only turn one was ever reported for,
    // which is why the usage list below has one entry and not two.
    assert_eq!(spend.turns, 2, "a failed run reports the turns it started");
    assert_eq!(
        spend.usage,
        vec![usage(10, 5)],
        "one entry per turn the provider reported for"
    );
    assert_eq!(spend.total_tokens(), Some((10, 5)));
    assert_eq!(
        spend.cost_micro_usd,
        Some(20),
        "and it is the figure the record already carries: {:?}",
        costs(&sink)
    );
    assert_eq!(
        costs(&sink),
        vec![20],
        "the ledger reports neither more nor less than the events did"
    );
}

#[test]
fn a_run_that_answers_reports_the_same_figures_to_the_ledger_as_to_the_outcome() {
    // The other half of the write-back: the two records of one run must agree, and neither may
    // count a turn the other has already counted.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "a", json!({}))])),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    )
    .priced(scripted_card());
    let mut items = Vec::new();
    let (outcome, spend, sink) = harness.run_in(&mut items);
    let outcome = outcome.expect("the run completes");

    assert_eq!(
        spend.turns, outcome.turns,
        "the ledger and the outcome are two records of one run"
    );
    assert_eq!(spend.usage, outcome.usage);
    assert_eq!(spend.cost_micro_usd, outcome.cost_micro_usd);
    assert_eq!(spend.total_tokens(), outcome.total_tokens());
    assert_eq!(spend.turns, 2, "two turns, and not four");
    assert_eq!(
        spend.usage.len(),
        2,
        "one entry per billed turn, not one per record: {:?}",
        spend.usage
    );
    assert_eq!(spend.cost_micro_usd, Some(40));
    assert_eq!(costs(&sink), vec![20, 20], "and the sink saw the same two");
}

#[test]
fn a_budget_refused_before_the_first_request_hands_the_conversation_straight_back() {
    // The other error path out of a run. Nothing was spent, so what comes back is exactly what
    // went in plus the input — and a shell that saved it would lose nothing.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("never asked"))]),
        ScriptedTools::new(Vec::new()),
    )
    .budgeted(Budget {
        max_cost_microunits: Some(1_000),
        ..Budget::default()
    });
    let mut items = vec![Item::user("an earlier question")];
    let (outcome, _, _) = harness.run_in(&mut items);

    assert!(matches!(outcome, Err(LoopError::Budget(_))), "{outcome:?}");
    assert_eq!(
        items,
        vec![
            Item::user("an earlier question"),
            Item::user("do the thing")
        ],
        "the vector is never left empty by a run that could not start"
    );
}

/// A hook port that answers from a script and records everything it was asked.
///
/// Every deque answers *proceed* (or *no note*) once it runs out, so a test scripts only the
/// firings it is about.
#[derive(Default)]
struct ScriptedHooks {
    before: VecDeque<HookDecision>,
    after: VecDeque<AfterCall>,
    stops: VecDeque<HookDecision>,
    /// Every consultation in order, as `(point, the name the model called, the entry that runs)`.
    seen: Vec<(HookPoint, String, String)>,
    /// The outcomes the after-call hook was shown, before any note was added to them.
    outcomes: Vec<ToolOutcome>,
    /// What the stop hook was told the run would answer with.
    stop_texts: Vec<String>,
}

impl ScriptedHooks {
    fn before(mut self, decisions: Vec<HookDecision>) -> Self {
        self.before = decisions.into();
        self
    }

    /// Scripts what the `after-call` point answers, firing by firing.
    fn noting(mut self, spoken: Vec<AfterCall>) -> Self {
        self.after = spoken.into();
        self
    }

    fn stopping(mut self, decisions: Vec<HookDecision>) -> Self {
        self.stops = decisions.into();
        self
    }

    /// What the port was asked at one point, in order.
    fn asked_at(&self, point: HookPoint) -> Vec<(&str, &str)> {
        self.seen
            .iter()
            .filter(|(seen, _, _)| *seen == point)
            .map(|(_, called, invoked)| (called.as_str(), invoked.as_str()))
            .collect()
    }
}

impl HookPort for ScriptedHooks {
    fn before_call(&mut self, call: &ToolCall, invoked: &ToolSpec) -> HookDecision {
        self.seen.push((
            HookPoint::BeforeCall,
            call.name.as_str().to_owned(),
            invoked.name.as_str().to_owned(),
        ));
        self.before.pop_front().unwrap_or(HookDecision::Proceed)
    }

    fn after_call(
        &mut self,
        call: &ToolCall,
        invoked: &ToolSpec,
        outcome: &ToolOutcome,
    ) -> AfterCall {
        self.seen.push((
            HookPoint::AfterCall,
            call.name.as_str().to_owned(),
            invoked.name.as_str().to_owned(),
        ));
        self.outcomes.push(outcome.clone());
        self.after.pop_front().unwrap_or_default()
    }

    fn on_stop(&mut self, text: &str) -> HookDecision {
        self.stop_texts.push(text.to_owned());
        self.stops.pop_front().unwrap_or(HookDecision::Proceed)
    }
}

/// The same run as [`Harness::run`], with the operator's hooks attached.
///
/// A free function rather than a method, so a test can keep hold of the hook port and read what it
/// was asked after the run is over.
fn run_hooked(
    harness: &mut Harness,
    hooks: &mut dyn HookPort,
) -> (Result<LoopOutcome, LoopError>, VecLoopSink) {
    let mut sink = VecLoopSink::new();
    let outcome = AgentLoop::new(
        &mut harness.model,
        &mut harness.tools,
        harness.approvals.as_mut(),
        harness.config.clone(),
    )
    .with_cancel(harness.cancel.clone())
    .with_hooks(hooks)
    .run("do the thing", &mut sink);
    (outcome, sink)
}

/// Every hook firing in order, as `(point, call id, decision)`.
fn hook_runs(sink: &VecLoopSink) -> Vec<(&str, Option<&str>, HookDecision)> {
    sink.events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::HookRan {
                point,
                call_id,
                decision,
            } => Some((
                point.as_str(),
                call_id.as_ref().map(CallId::as_str),
                decision.clone(),
            )),
            _ => None,
        })
        .collect()
}

/// The tool results as the model reads them, payload and all.
///
/// Unlike [`results`] this keeps the output as JSON, because what an after-call note does is add a
/// field to it.
fn outputs(outcome: &LoopOutcome) -> Vec<(bool, Value)> {
    outcome
        .items
        .iter()
        .filter_map(|item| match item {
            Item::ToolResult { failed, output, .. } => Some((*failed, output.clone())),
            _ => None,
        })
        .collect()
}

/// One turn calling `a`, then an answer — the shape most hook tests need.
fn one_call_then_answers() -> ScriptedModel {
    ScriptedModel::new(vec![
        Ok(asks_for(&[("call-1", "a", json!({}))])),
        Ok(answer("understood")),
    ])
}

#[test]
fn a_before_call_hook_that_blocks_keeps_the_call_from_happening_at_all() {
    let mut harness = Harness::new(
        one_call_then_answers(),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    );
    let mut hooks = ScriptedHooks::default().before(vec![HookDecision::block("no writes today")]);
    let (outcome, sink) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("a block is not a failure");

    assert!(
        harness.tools.calls.is_empty(),
        "the port is never reached for a blocked call"
    );
    let told = results(&outcome);
    assert_eq!(told.len(), 1);
    assert!(
        told[0].0,
        "the model has to learn the effect did not happen"
    );
    assert_eq!(told[0].1, "`a` was blocked by a hook: no writes today");
    assert_eq!(
        hook_runs(&sink),
        vec![
            (
                "before-call",
                Some("call-1"),
                HookDecision::block("no writes today")
            ),
            ("stop", None, HookDecision::Proceed),
        ],
        "the record says which hook decided what"
    );
    assert!(
        hooks.asked_at(HookPoint::AfterCall).is_empty(),
        "there is no outcome to read when nothing ran"
    );
}

#[test]
fn a_before_call_hook_that_could_not_decide_fails_closed() {
    // A hook that could not run did not say yes. The distinct sentence is the point: *the guard
    // is broken* and *the guard said no* are different things for the model to act on.
    let mut harness = Harness::new(
        one_call_then_answers(),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)]),
    );
    let mut hooks =
        ScriptedHooks::default().before(vec![HookDecision::failed("the guard exited 3")]);
    let (outcome, sink) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("a hook that broke is not a run that broke");

    assert!(harness.tools.calls.is_empty(), "fail closed, not open");
    let told = results(&outcome);
    assert!(told[0].0);
    assert_eq!(
        told[0].1,
        "`a` did not run because a hook could not check it: the guard exited 3"
    );
    assert_eq!(
        hook_runs(&sink)[0],
        (
            "before-call",
            Some("call-1"),
            HookDecision::failed("the guard exited 3")
        )
    );
}

#[test]
fn a_hook_is_asked_about_the_entry_and_not_the_verb_it_came_through() {
    // `tools: ["run"]` in an operator's hook file has to mean the entry that runs, or a hook on
    // `run` would never fire on a surface that publishes it behind `tool_invoke` — and one on
    // `tool_invoke` would fire on every read.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "tool_invoke",
                json!({"name": "run", "arguments": {"argv": ["cargo", "test"]}}),
            )])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("tool_invoke", Approval::NotRequired)])
            .invoking(spec("run", Approval::NotRequired)),
    );
    let mut hooks = ScriptedHooks::default().before(vec![HookDecision::block("not in this run")]);
    let (outcome, _) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("a block is not a failure");

    assert_eq!(
        hooks.asked_at(HookPoint::BeforeCall),
        vec![("tool_invoke", "run")],
        "the entry's own spec is what the hook decides on"
    );
    assert_eq!(
        results(&outcome)[0].1,
        "`run` (called through `tool_invoke`) was blocked by a hook: not in this run",
        "and the refusal names both, so the model does not abandon the verb"
    );
}

#[test]
fn a_call_a_person_refused_never_reaches_a_hook() {
    // Order is the safety argument: a hook that saw a denied call could only ever say yes to it,
    // which would be a second gate reversing the first.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "tool_invoke",
                json!({"tool": "run", "argv": ["rm", "-rf", "/"]}),
            )])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("tool_invoke", Approval::NotRequired)])
            .enveloped(starts_a_process()),
    );
    let mut hooks = ScriptedHooks::default();
    let (outcome, sink) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("a denial is not a failure");

    assert!(harness.tools.calls.is_empty());
    assert!(results(&outcome)[0].1.contains("not approved"));
    assert!(
        hooks.asked_at(HookPoint::BeforeCall).is_empty(),
        "the approver had already refused it"
    );
    assert_eq!(
        hook_runs(&sink),
        vec![("stop", None, HookDecision::Proceed)],
        "the only hook this run consulted is the one at its end"
    );
}

#[test]
fn an_after_call_note_joins_an_object_result_under_hook_notes() {
    let mut harness = Harness::new(
        one_call_then_answers(),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)])
            .answering("a", ToolOutcome::ok(json!({"text": "hello"}))),
    );
    let mut hooks =
        ScriptedHooks::default().noting(vec![AfterCall::note("rustfmt rewrote two files")]);
    let (outcome, sink) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("a note is not a failure");

    assert_eq!(
        outputs(&outcome),
        vec![(
            false,
            json!({"text": "hello", "hook_notes": ["rustfmt rewrote two files"]})
        )],
        "the note is beside the result, and `failed` is still the tool's own"
    );
    assert_eq!(
        hooks.outcomes,
        vec![ToolOutcome::ok(json!({"text": "hello"}))],
        "what the hook read is what the tool said, before its own note was added"
    );
    assert_eq!(
        hook_runs(&sink)[1],
        ("after-call", Some("call-1"), HookDecision::Proceed),
        "the point fired, and the record says so"
    );
}

#[test]
fn an_after_call_hook_that_failed_is_recorded_as_failed_and_its_reason_reaches_the_model() {
    // Two readers, two halves of one firing. The model needs the reason as a note, because the
    // check it would have read never happened; a reader of the JSONL record needs the decision,
    // because `failed` on the outcome is the tool's own and an after-call hook may not touch it.
    // Folding the failure into the note alone left the record saying `proceed` about a hook that
    // crashed.
    let mut harness = Harness::new(
        one_call_then_answers(),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)])
            .answering("a", ToolOutcome::ok(json!({"text": "hello"}))),
    );
    let mut hooks = ScriptedHooks::default().noting(vec![AfterCall::failed(
        "`rustfmt-check` could not be started",
    )]);
    let (outcome, sink) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("a hook that crashed after the call does not fail the run");

    assert_eq!(
        outputs(&outcome),
        vec![(
            false,
            json!({
                "text": "hello",
                "hook_notes": ["`rustfmt-check` could not be started"],
            })
        )],
        "the reason is beside the result, and `failed` is still the tool's own"
    );
    assert_eq!(
        hook_runs(&sink)[1],
        (
            "after-call",
            Some("call-1"),
            HookDecision::failed("`rustfmt-check` could not be started")
        ),
        "the record says the hook failed, which only the decision can carry"
    );
}

#[test]
fn an_after_call_note_wraps_a_result_that_is_not_an_object() {
    // Appended to a string, a note would read to the model exactly like something the tool said.
    let mut harness = Harness::new(
        one_call_then_answers(),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)])
            .answering("a", ToolOutcome::ok(json!("the file is empty"))),
    );
    let mut hooks = ScriptedHooks::default().noting(vec![AfterCall::note("checked by policy")]);
    let (outcome, _) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("a note is not a failure");

    assert_eq!(
        outputs(&outcome),
        vec![(
            false,
            json!({"output": "the file is empty", "hook_notes": ["checked by policy"]})
        )]
    );
}

#[test]
fn a_note_that_pushes_a_result_over_the_bound_is_refused_and_names_the_note() {
    // A note must not become the one way an oversized payload reaches the model, and a model told
    // to narrow a request it never oversized would narrow the wrong thing.
    let nearly_full = json!({"text": "x".repeat(MAX_TOOL_RESULT_BYTES - 100)});
    assert!(!exceeds(&nearly_full, MAX_TOOL_RESULT_BYTES));
    let mut harness = Harness::new(
        one_call_then_answers(),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)])
            .answering("a", ToolOutcome::ok(nearly_full)),
    );
    let mut hooks = ScriptedHooks::default().noting(vec![AfterCall::note("n".repeat(500))]);
    let (outcome, _) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("the run recovers");

    let told = results(&outcome);
    assert!(told[0].0, "the model reads a refusal, not a trimmed result");
    assert!(
        told[0].1.contains("after-call hook's note"),
        "the refusal names the note as the cause: {}",
        told[0].1
    );
    assert!(
        told[0].1.len() < MAX_TOOL_RESULT_BYTES,
        "and neither the result nor the note is forwarded"
    );
}

#[test]
fn a_stop_hook_that_blocks_puts_its_reason_to_the_model_and_the_run_turns_again() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("all done")), Ok(answer("now it passes"))]),
        ScriptedTools::new(Vec::new()),
    );
    // One block, and then the deque's default: proceed.
    let mut hooks = ScriptedHooks::default().stopping(vec![HookDecision::block(
        "the tests do not pass; fix them before you stop",
    )]);
    let (outcome, sink) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("a stop hook does not fail a run");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.turns, 2, "exactly one more turn");
    assert_eq!(outcome.text, "now it passes");
    assert!(
        harness.model.seen[1].items.contains(&Item::user(
            "the tests do not pass; fix them before you stop"
        )),
        "the reason reaches the model as one user item: {:?}",
        harness.model.seen[1].items
    );
    assert_eq!(
        hooks.stop_texts,
        vec!["all done".to_owned(), "now it passes".to_owned()],
        "the hook reads what a consumer would have read"
    );
    assert_eq!(hook_runs(&sink).len(), 2);
}

#[test]
fn a_stop_hook_that_never_lets_go_runs_out_of_continues_and_the_run_ends() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(answer("done")),
            Ok(answer("done")),
            Ok(answer("done")),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(Vec::new()),
    );
    let mut hooks =
        ScriptedHooks::default().stopping(vec![HookDecision::block("still not green"); 6]);
    let (outcome, sink) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("an exhausted hook is not a failure");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(
        outcome.turns,
        1 + u64::from(MAX_STOP_HOOK_CONTINUES),
        "the first turn, and exactly {MAX_STOP_HOOK_CONTINUES} more"
    );
    let exhausted: Vec<&str> = sink
        .warnings()
        .filter(|(code, _)| *code == "stop-hook-exhausted")
        .map(|(_, message)| message)
        .collect();
    assert_eq!(exhausted.len(), 1, "said once, at the end");
    assert!(
        exhausted[0].contains("still not green"),
        "and it carries the hook's last reason: {}",
        exhausted[0]
    );
    assert_eq!(
        hook_runs(&sink).len(),
        1 + MAX_STOP_HOOK_CONTINUES as usize,
        "the hook is consulted on every attempted stop, including the one it loses"
    );
}

#[test]
fn a_stop_hook_that_could_not_decide_fails_open_and_says_so() {
    // The one point that fails open: a crashed hook must not keep a run alive for ever.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("done"))]),
        ScriptedTools::new(Vec::new()),
    );
    let mut hooks =
        ScriptedHooks::default().stopping(vec![HookDecision::failed("the checker crashed")]);
    let (outcome, sink) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("a crashed hook is not a failed run");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.turns, 1);
    let failed: Vec<&str> = sink
        .warnings()
        .filter(|(code, _)| *code == "hook-failed")
        .map(|(_, message)| message)
        .collect();
    assert_eq!(failed.len(), 1);
    assert!(failed[0].contains("the checker crashed"), "{}", failed[0]);
}

#[test]
fn a_run_with_hooks_attached_batches_nothing() {
    // Without hooks these three reads go to the port as one batch — that is
    // `the_pure_calls_of_one_turn_go_to_the_port_together_and_a_write_is_a_barrier`. With hooks
    // every call takes the single-call path, so a hook fires exactly once per call and never twice
    // on a group the port miscounted.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[
                ("call-1", "read", json!({})),
                ("call-2", "read", json!({})),
                ("call-3", "read", json!({})),
            ])),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]),
    );
    let mut hooks = ScriptedHooks::default();
    let (outcome, sink) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("the turn completes");

    assert!(
        harness.tools.batches.is_empty(),
        "no group was ever handed over: {:?}",
        harness.tools.batches
    );
    assert_eq!(harness.tools.calls.len(), 3, "one port call each");
    assert_eq!(answered(&outcome), vec!["call-1", "call-2", "call-3"]);
    let per_call: Vec<(&str, Option<&str>)> = hook_runs(&sink)
        .iter()
        .filter(|(point, _, _)| *point != "stop")
        .map(|(point, call_id, _)| (*point, *call_id))
        .collect();
    assert_eq!(
        per_call,
        vec![
            ("before-call", Some("call-1")),
            ("after-call", Some("call-1")),
            ("before-call", Some("call-2")),
            ("after-call", Some("call-2")),
            ("before-call", Some("call-3")),
            ("after-call", Some("call-3")),
        ],
        "exactly once per call, at each point"
    );
}

#[test]
fn hooks_that_say_nothing_change_the_run_not_at_all_but_still_enter_the_record() {
    // `NoHooks` is the port's every default. Nothing is refused and no note is added — but the
    // firings are still emitted, because the hooks *were* consulted and a point that fired
    // silently would read exactly like one that never ran.
    let mut harness = Harness::new(
        one_call_then_answers(),
        ScriptedTools::new(vec![spec("a", Approval::NotRequired)])
            .answering("a", ToolOutcome::ok(json!({"text": "hello"}))),
    );
    let (outcome, sink) = run_hooked(&mut harness, &mut NoHooks);
    let outcome = outcome.expect("no hooks, no difference");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(harness.tools.calls.len(), 1);
    assert_eq!(
        outputs(&outcome),
        vec![(false, json!({"text": "hello"}))],
        "the result reaches the model exactly as the tool gave it"
    );
    assert_eq!(
        hook_runs(&sink),
        vec![
            ("before-call", Some("call-1"), HookDecision::Proceed),
            ("after-call", Some("call-1"), HookDecision::Proceed),
            ("stop", None, HookDecision::Proceed),
        ]
    );
}

// --- the loop's own tools: `answer` and `delegate` ------------------------------------------------

/// A schema with one required field, which is all a test needs to tell an answer from prose.
fn verdict_schema() -> OutputSchema {
    OutputSchema::new(json!({
        "type": "object",
        "properties": {"verdict": {"type": "string"}},
        "required": ["verdict"],
    }))
    .expect("an object schema is accepted")
}

/// The names a request published, in the order the model reads them.
fn published_names(request: &TurnRequest) -> Vec<&str> {
    request
        .tools
        .iter()
        .map(|spec| spec.name.as_str())
        .collect()
}

/// One event's tag, so a test can say what reached a sink without matching every shape.
fn kind(event: &LoopEvent) -> String {
    serde_json::to_value(event).expect("every event serializes")["kind"]
        .as_str()
        .expect("every event is tagged by its kind")
        .to_owned()
}

/// What reached the parent's sink **bare** — everything a child emitted should be missing here.
fn bare_kinds(sink: &VecLoopSink) -> Vec<String> {
    sink.events().iter().map(kind).collect()
}

/// The child's events, unwrapped, in the order the parent's sink saw them.
fn delegated(sink: &VecLoopSink) -> Vec<&LoopEvent> {
    sink.events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::Delegated { event, .. } => Some(event.as_ref()),
            _ => None,
        })
        .collect()
}

/// The outcome of the call with this id, as the model reads it.
fn result_of(outcome: &LoopOutcome, wanted: &str) -> (bool, Value) {
    outcome
        .items
        .iter()
        .find_map(|item| match item {
            Item::ToolResult {
                call_id,
                output,
                failed,
            } if call_id.as_str() == wanted => Some((*failed, output.clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("every call is answered, and `{wanted}` was not"))
}

#[test]
fn an_answer_call_is_the_run_s_result_and_ends_it() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(asks_for(&[(
            "call-1",
            "answer",
            json!({"verdict": "green", "notes": ["one"]}),
        )]))]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]),
    )
    .answering_in(verdict_schema());
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("an answered run completes");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.turns, 1);
    assert_eq!(
        outcome.structured,
        Some(json!({"verdict": "green", "notes": ["one"]})),
        "the locally validated arguments are the answer"
    );
    assert!(
        harness.tools.calls.is_empty(),
        "an owned call never reaches the tool port"
    );
    assert_eq!(
        result_of(&outcome, "call-1"),
        (false, json!({"accepted": true})),
        "a call replayed without its result is a provider error on the next turn"
    );
    assert!(
        sink.events().iter().any(|event| matches!(
            event,
            LoopEvent::Answered { call_id, value }
                if call_id.as_str() == "call-1" && value["verdict"] == json!("green")
        )),
        "the record of a run carries its answer: {:?}",
        bare_kinds(&sink)
    );
}

#[test]
fn every_other_call_in_the_answering_turn_is_refused_by_name() {
    // The tool was described as the last thing, so a call beside it asks for an effect after the
    // run was declared over — and the model has to learn it did not happen.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(asks_for(&[
            ("call-1", "answer", json!({"verdict": "green"})),
            ("call-2", "read", json!({"path": "README.md"})),
        ]))]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]),
    )
    .answering_in(verdict_schema());
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("an answered run completes");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert!(harness.tools.calls.is_empty(), "the read never ran");
    assert_eq!(
        answered(&outcome),
        vec!["call-1", "call-2"],
        "both calls are answered, or the conversation cannot be replayed"
    );
    assert_eq!(
        result_of(&outcome, "call-2"),
        (
            true,
            json!("refused: made in the same turn as `answer`, which must be called alone")
        ),
        "and the refusal says why it was refused, not that the run is over: the answer's own \
         outcome is not known when this one is written"
    );
}

#[test]
fn a_call_made_before_the_answer_in_the_same_turn_never_runs_either() {
    // The tool's own description is *call it alone, as the last thing: any other call in the same
    // turn is refused*. A loop that ran the write and refused the read kept that promise for half
    // the turn: the write is as much an effect after the run was declared over as the read is.
    // Nothing else here would have stopped it — the approver says yes to everything.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(asks_for(&[
            ("call-1", "file_write", json!({"path": "a", "text": "x"})),
            ("call-2", "answer", json!({"verdict": "green"})),
            ("call-3", "read", json!({"path": "b"})),
        ]))]),
        ScriptedTools::new(vec![
            spec("file_write", Approval::NotRequired),
            spec("read", Approval::NotRequired),
        ])
        .enveloping("file_write", writes()),
    )
    .approving(Box::new(ApproveAll))
    .unattended_above(Risk::High)
    .answering_in(verdict_schema());
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("an answered run completes");

    assert!(
        harness.tools.calls.is_empty(),
        "the write never reached the port, approved or not"
    );
    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.structured, Some(json!({"verdict": "green"})));
    assert_eq!(
        answered(&outcome),
        vec!["call-1", "call-2", "call-3"],
        "every call is answered, in the order the model made them"
    );
    for refused in ["call-1", "call-3"] {
        assert_eq!(
            result_of(&outcome, refused),
            (
                true,
                json!("refused: made in the same turn as `answer`, which must be called alone")
            ),
            "{refused}"
        );
    }
}

#[test]
fn a_failed_answer_beside_other_calls_refuses_them_without_claiming_the_run_ended() {
    // The siblings are refused *before* the answer is tried — that is the whole point, so the
    // write cannot happen after the run was declared over — and an answer can still fail. Here it
    // fails on arguments that are not an object, so the run turns again. A refusal saying *the run
    // ended with the answer* would be false in the same turn that tells the model to call `answer`
    // again, and a model reading both may redo the write or stop.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[
                ("call-1", "file_write", json!({"path": "a", "text": "x"})),
                ("call-2", "answer", json!("green")),
            ])),
            Ok(asks_for(&[(
                "call-3",
                "answer",
                json!({"verdict": "green"}),
            )])),
        ]),
        ScriptedTools::new(vec![spec("file_write", Approval::NotRequired)])
            .enveloping("file_write", writes()),
    )
    .approving(Box::new(ApproveAll))
    .unattended_above(Risk::High)
    .answering_in(verdict_schema());
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("the run recovers on the next turn");

    assert!(
        harness.tools.calls.is_empty(),
        "the write never reached the port, approved or not"
    );
    let (failed, refusal) = result_of(&outcome, "call-1");
    assert!(failed);
    assert!(
        !refusal
            .as_str()
            .unwrap_or_default()
            .contains("the run ended"),
        "the run did not end — it turned again and answered — so the refusal beside a failed \
         answer must not say it did: {refusal}"
    );
    assert_eq!(
        refusal,
        json!("refused: made in the same turn as `answer`, which must be called alone")
    );
    let (failed, answer) = result_of(&outcome, "call-2");
    assert!(failed);
    assert!(
        answer
            .as_str()
            .unwrap_or_default()
            .contains("not an object"),
        "{answer}"
    );
    assert_eq!(outcome.turns, 2, "the run went on, as the refusal implies");
    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(
        outcome.structured,
        Some(json!({"verdict": "green"})),
        "and the second answer is the one it ends with"
    );
}

#[test]
fn oversized_arguments_are_refused_for_the_loops_own_tools_too() {
    // `recordable` replaces arguments over the bound with `{"omitted": …}` in the conversation, so
    // an answer accepted past it would put a value in `structured` that the record of the run does
    // not carry — and a delegate past it would hand a child a task nobody can read back.
    let huge = "x".repeat(MAX_TOOL_ARGUMENT_BYTES);
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "answer", json!({"verdict": huge}))])),
            Ok(asks_for(&[("call-2", "delegate", json!({"task": huge}))])),
            Ok(asks_for(&[(
                "call-3",
                "answer",
                json!({"verdict": "green"}),
            )])),
        ]),
        ScriptedTools::new(Vec::new()),
    )
    .answering_in(verdict_schema())
    .delegating(Delegation::default());
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("the run recovers on the next turn");

    for id in ["call-1", "call-2"] {
        let (failed, output) = result_of(&outcome, id);
        assert!(failed, "{id}");
        assert!(
            output.as_str().unwrap_or_default().contains("byte bound"),
            "{id}: {output}"
        );
    }
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| matches!(event, LoopEvent::DelegateStarted { .. })),
        "no child is started on a task nobody could read back"
    );
    assert_eq!(harness.model.seen.len(), 3, "three turns, all the parent's");
    assert_eq!(
        outcome.structured,
        Some(json!({"verdict": "green"})),
        "and the answer the run reports is the one its record carries whole"
    );
    let replayed = harness.model.seen[1]
        .items
        .iter()
        .find_map(Item::as_tool_call)
        .expect("the call is still visible to the model");
    assert!(
        replayed.arguments.get("omitted").is_some(),
        "{:?}",
        replayed.arguments
    );
}

#[test]
fn arguments_that_are_not_an_object_are_refused_back_to_the_model_rather_than_accepted() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "answer", json!("green"))])),
            Ok(asks_for(&[(
                "call-2",
                "answer",
                json!({"verdict": "green"}),
            )])),
        ]),
        ScriptedTools::new(Vec::new()),
    )
    .answering_in(verdict_schema());
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("the run recovers on the next turn");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.turns, 2, "a refusal is a turn the model can act on");
    let (failed, output) = result_of(&outcome, "call-1");
    assert!(failed);
    assert!(
        output
            .as_str()
            .unwrap_or_default()
            .contains("not an object"),
        "{output}"
    );
    assert_eq!(outcome.structured, Some(json!({"verdict": "green"})));
}

#[test]
fn an_answer_that_misses_its_schema_is_refused_back_to_the_model() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "answer", json!({"notes": []}))])),
            Ok(asks_for(&[(
                "call-2",
                "answer",
                json!({"verdict": "green"}),
            )])),
        ]),
        ScriptedTools::new(Vec::new()),
    )
    .answering_in(verdict_schema());
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("the model repairs the answer on the next turn");

    let (failed, output) = result_of(&outcome, "call-1");
    assert!(failed);
    assert!(
        output
            .as_str()
            .is_some_and(|text| text.contains("published schema")),
        "{output}"
    );
    assert_eq!(outcome.structured, Some(json!({"verdict": "green"})));
}

#[test]
fn a_run_that_ended_in_prose_is_told_once_more_and_then_answers() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(answer("the verdict is green")),
            Ok(asks_for(&[(
                "call-1",
                "answer",
                json!({"verdict": "green"}),
            )])),
        ]),
        ScriptedTools::new(Vec::new()),
    )
    .answering_in(verdict_schema());
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("the nudged run completes");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(outcome.turns, 2, "the nudge is a turn like any other");
    assert_eq!(outcome.structured, Some(json!({"verdict": "green"})));
    assert_eq!(
        sink.warnings().map(|(code, _)| code).collect::<Vec<_>>(),
        vec!["answer-nudged"]
    );
    assert!(
        harness.model.seen[1].items.iter().any(|item| matches!(
            item,
            Item::UserText { text } if text.contains("Finish by calling `answer`")
        )),
        "the nudge is one user item: {:?}",
        harness.model.seen[1].items
    );
}

/// The nudge is **asked twice**: once in words, once as the provider's own constraint.
///
/// A model that ended in prose has already read the answer tool's description and the nudge's
/// sentence; asking a third time in words buys nothing. The turn the nudge opens is therefore held
/// to that tool at the wire, which is the difference between asking and requiring. Measured, not
/// assumed: the seventh paid native walk (2026-08-30) ended in prose on three of four attempts at
/// one section under the nudge alone.
#[test]
fn the_turn_a_nudge_opens_is_held_to_the_answer_tool_and_no_earlier_turn_is() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(answer("the verdict is green")),
            Ok(asks_for(&[(
                "call-1",
                "answer",
                json!({"verdict": "green"}),
            )])),
        ]),
        ScriptedTools::new(Vec::new()),
    )
    .answering_in(verdict_schema());
    let (outcome, _) = harness.run();
    outcome.expect("the nudged run completes");

    assert!(
        harness.model.seen[0].tool_choice.is_auto(),
        "the first turn is the run doing its work, and a held turn would answer instead of doing it"
    );
    assert_eq!(
        harness.model.seen[1].tool_choice,
        harness_wire::ToolChoice::Named(tool_name("answer")),
        "the turn after the nudge is held to the tool the nudge asked for"
    );
    // And what it is held to is published on that same turn, which is what `validate` requires.
    assert!(
        harness.model.seen[1]
            .tools
            .iter()
            .any(|tool| tool.name.as_str() == "answer")
    );
}

/// A run with no output schema is never held to anything: there is no tool to be held to, and a
/// run that published none and named one would be refused by `TurnRequest::validate`.
#[test]
fn a_run_that_asked_for_no_shape_is_never_held_to_a_tool() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(answer("done"))]),
        ScriptedTools::new(Vec::new()),
    );
    let (outcome, _) = harness.run();
    outcome.expect("a prose run is a run");
    assert!(
        harness
            .model
            .seen
            .iter()
            .all(|turn| turn.tool_choice.is_auto())
    );
}

#[test]
fn prose_twice_stops_unstructured_rather_than_completed_and_carries_no_answer() {
    // A consumer that piped stdout to a JSON reader and got prose with a success status would be
    // exactly the silent failure invariant 8 forbids.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(answer("the verdict is green")),
            Ok(answer("really, it is green")),
        ]),
        ScriptedTools::new(Vec::new()),
    )
    .answering_in(verdict_schema());
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("an unstructured stop is an outcome, not an error");

    assert_eq!(outcome.stop, LoopStop::Unstructured { asked_again: 1 });
    assert_eq!(outcome.structured, None);
    assert_eq!(outcome.turns, 2, "one nudge, and no second one");
    assert_eq!(outcome.text, "really, it is green");
}

#[test]
fn a_stop_hook_continuation_earns_the_run_a_fresh_nudge_rather_than_ending_it_unstructured() {
    // The sequence this protects: prose, nudge, `answer`, a stop hook that sends the run back to
    // work — which withdraws that answer — and then prose again. Counting nudges per run rather
    // than per ending spent the only one on the first ending, so the second ended `Unstructured`
    // with empty stdout and exit 2, having never asked. A continuation is a new ending, and a new
    // ending is owed the nudge.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(answer("the verdict is green")),
            Ok(asks_for(&[(
                "call-1",
                "answer",
                json!({"verdict": "green"}),
            )])),
            Ok(answer("still green, I promise")),
            Ok(asks_for(&[(
                "call-2",
                "answer",
                json!({"verdict": "amber"}),
            )])),
        ]),
        ScriptedTools::new(Vec::new()),
    )
    .answering_in(verdict_schema());
    let mut hooks = ScriptedHooks::default().stopping(vec![HookDecision::block("look again")]);
    let (outcome, sink) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("the twice-nudged run completes");

    assert_eq!(
        outcome.stop,
        LoopStop::Completed,
        "a nudge counted per run and not per ending stops the second prose turn `Unstructured` \
         without ever asking again"
    );
    assert_eq!(
        outcome.structured,
        Some(json!({"verdict": "amber"})),
        "the answer the run ends with is the one after the hook let go"
    );
    assert_eq!(
        sink.warnings().map(|(code, _)| code).collect::<Vec<_>>(),
        vec!["answer-nudged", "answer-nudged"],
        "one nudge per ending, and there were two endings"
    );
    assert_eq!(
        outcome.turns, 4,
        "prose, answer, prose after the block, answer again"
    );
    assert_eq!(
        hook_runs(&sink)
            .into_iter()
            .filter(|(point, _, _)| *point == "stop")
            .collect::<Vec<_>>(),
        vec![
            ("stop", None, HookDecision::block("look again")),
            ("stop", None, HookDecision::Proceed),
        ],
        "and the hook was asked about both endings, never about a nudge"
    );
}

#[test]
fn a_port_that_publishes_an_owned_name_refuses_the_run_before_any_byte_goes_out() {
    let mut clashing = Harness::new(
        ScriptedModel::new(vec![Ok(answer("never asked"))]),
        ScriptedTools::new(vec![spec("answer", Approval::NotRequired)]),
    )
    .answering_in(verdict_schema());
    let (outcome, sink) = clashing.run();

    let Err(LoopError::Config(reason)) = outcome else {
        panic!("a name the model could not address is a configuration refusal: {outcome:?}");
    };
    assert!(reason.contains("`answer`"), "named by name: {reason}");
    assert!(
        clashing.model.seen.is_empty(),
        "the model port is never reached"
    );
    assert!(sink.events().is_empty(), "not even a `Started`");

    let mut delegating = Harness::new(
        ScriptedModel::new(vec![Ok(answer("never asked"))]),
        ScriptedTools::new(vec![spec("delegate", Approval::NotRequired)]),
    )
    .delegating(Delegation::default());
    let (outcome, _) = delegating.run();
    let Err(LoopError::Config(reason)) = outcome else {
        panic!("the same holds for the other owned tool: {outcome:?}");
    };
    assert!(reason.contains("`delegate`"), "{reason}");
    assert!(delegating.model.seen.is_empty());

    // And a caller who named both the same thing is refused with nobody else to blame.
    let schema = OutputSchema::named(
        tool_name("delegate"),
        "answer here",
        json!({"type": "object"}),
    )
    .expect("an object schema");
    let mut colliding = Harness::new(
        ScriptedModel::new(vec![Ok(answer("never asked"))]),
        ScriptedTools::new(Vec::new()),
    )
    .answering_in(schema)
    .delegating(Delegation::default());
    let (outcome, _) = colliding.run();
    let Err(LoopError::Config(reason)) = outcome else {
        panic!("two owned tools of one name is the same unaddressable request: {outcome:?}");
    };
    assert!(reason.contains("both published as `delegate`"), "{reason}");
    assert!(colliding.model.seen.is_empty());
}

#[test]
fn a_port_that_would_route_an_owned_name_refuses_the_run_though_it_publishes_no_such_tool() {
    // Under the three-verb surface `specs()` is three verbs, and the port still answers to a bare
    // entry name — the routed path, 12 % of the calls in a measured run. A catalogue entry called
    // `delegate` puts nothing in the request twice, so the duplicate check sees nothing; it would
    // simply never be reachable, because the loop resolves its own tools first and the call would
    // never arrive at the port. Silently unreachable is what this refuses.
    let mut shadowed = Harness::new(
        ScriptedModel::new(vec![Ok(answer("never asked"))]),
        ScriptedTools::new(vec![spec("tool_invoke", Approval::NotRequired)])
            .routing(spec("delegate", Approval::NotRequired)),
    )
    .delegating(Delegation::default());
    let (outcome, sink) = shadowed.run();

    let Err(LoopError::Config(reason)) = outcome else {
        panic!("a tool nothing could ever reach is a configuration refusal: {outcome:?}");
    };
    assert!(reason.contains("`delegate`"), "named by name: {reason}");
    assert!(
        shadowed.model.seen.is_empty(),
        "the model port is never reached"
    );
    assert!(sink.events().is_empty(), "not even a `Started`");
}

#[test]
fn the_owned_specs_are_published_after_the_ports_and_only_when_the_run_asked_for_them() {
    let mut plain = Harness::new(
        ScriptedModel::new(vec![Ok(answer("ok"))]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]),
    );
    let _ = plain.run();
    assert_eq!(
        published_names(&plain.model.seen[0]),
        vec!["read"],
        "a run that asked for neither publishes exactly what it always did"
    );

    let mut both = Harness::new(
        ScriptedModel::new(vec![Ok(asks_for(&[(
            "call-1",
            "answer",
            json!({"verdict": "green"}),
        )]))]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]),
    )
    .answering_in(verdict_schema())
    .delegating(Delegation::default());
    let (_, sink) = both.run();
    assert_eq!(
        published_names(&both.model.seen[0]),
        vec!["read", "answer", "delegate"],
        "the catalogue the run is for reads first"
    );
    let Some(LoopEvent::Started {
        published_tools, ..
    }) = sink.events().first()
    else {
        panic!("the run starts by saying what it can do");
    };
    assert_eq!(
        published_tools,
        &vec![
            tool_name("read"),
            tool_name("answer"),
            tool_name("delegate")
        ],
        "a reader who saw `answer` in the record must be able to see where it came from"
    );
}

/// A parent that delegates once and then answers, and a child that reads a file and reports.
///
/// One [`ScriptedModel`] serves both, so the script is the two loops interleaved in the order the
/// turns actually happen: the parent's first, the child's whole run inside it, the parent's second.
fn delegating_pair() -> Harness {
    Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "survey the three files and report"}),
            )])),
            Ok(asks_for(&[("child-1", "read", json!({"path": "a"}))])),
            Ok(answer("three files, all green")),
            Ok(asks_for(&[(
                "call-2",
                "answer",
                json!({"verdict": "green"}),
            )])),
        ]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]),
    )
    .answering_in(verdict_schema())
    .delegating(Delegation::default())
}

#[test]
fn a_delegate_runs_a_whole_second_loop_and_the_parent_reads_only_its_report() {
    let mut harness = delegating_pair();
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("the delegating run completes");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(
        outcome.turns, 4,
        "`turns` is the whole run's spend, including the child's model requests"
    );
    assert_eq!(
        result_of(&outcome, "call-1"),
        (
            false,
            json!({
                "stop": {"kind": "completed"},
                "turns": 2,
                "text": "three files, all green",
            })
        ),
        "one tool result, not the forty reads it took to write it"
    );
    assert_eq!(outcome.structured, Some(json!({"verdict": "green"})));
    assert_eq!(
        outcome.usage.len(),
        4,
        "the parent's two turns and the child's two: a delegate spends the run's budget"
    );
    assert_eq!(outcome.total_tokens(), Some((40, 20)));
    assert!(
        sink.events().iter().any(|event| matches!(
            event,
            LoopEvent::DelegateStarted { call_id, task }
                if call_id.as_str() == "call-1" && task == "survey the three files and report"
        )),
        "the record says what was handed over, because nothing else will"
    );
}

#[test]
fn every_event_a_delegate_emits_reaches_the_parents_sink_wrapped_and_none_of_it_bare() {
    let mut harness = delegating_pair();
    let (_, sink) = harness.run();

    assert_eq!(
        bare_kinds(&sink),
        vec![
            "started",
            "turn-started",
            "usage",
            "tool-requested",
            "delegate-started",
            // Everything between these two is the child's whole run, and every one of them is
            // `delegated`: eight events, listed in full below.
            "delegated",
            "delegated",
            "delegated",
            "delegated",
            "delegated",
            "delegated",
            "delegated",
            "delegated",
            "delegate-finished",
            "tool-completed",
            "turn-started",
            "usage",
            "tool-requested",
            "tool-completed",
            "answered",
            "finished",
        ],
        "the child's text is not the parent's answer and its usage is not the parent's turns"
    );
    let child: Vec<String> = delegated(&sink).into_iter().map(kind).collect();
    assert_eq!(
        child,
        vec![
            "started",
            "turn-started",
            "usage",
            "tool-requested",
            "tool-completed",
            "turn-started",
            "usage",
            "finished",
        ]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect::<Vec<String>>(),
        "a whole run, nested: a renderer indents it and a JSONL record nests it"
    );
    assert!(
        sink.events().iter().any(|event| matches!(
            event,
            LoopEvent::DelegateFinished { call_id, stop, turns }
                if call_id.as_str() == "call-1" && *stop == LoopStop::Completed && *turns == 2
        )),
        "the brackets carry what the child did"
    );
}

#[test]
fn a_child_publishes_neither_delegate_nor_answer_and_is_told_it_is_one() {
    let mut harness = delegating_pair();
    let _ = harness.run();

    let child = &harness.model.seen[1];
    assert_eq!(
        published_names(child),
        vec!["read"],
        "depth 1: a delegate has no delegate of its own, and its report is its text"
    );
    assert_eq!(
        child.instructions,
        format!("be useful\n\n{DELEGATE_PREAMBLE}"),
        "the parent's standing instruction whole, so the delegate knows where it is"
    );
    assert_eq!(
        child.items,
        vec![Item::user("survey the three files and report")],
        "a conversation that starts empty: the task, and nothing of the parent's"
    );
}

#[test]
fn a_high_risk_call_inside_a_delegate_meets_the_same_gate_and_is_refused_there() {
    // Delegation widens nothing: the child can do exactly what the parent's catalogue admits, and
    // every call inside it meets the run's own approver.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "run the suite"}),
            )])),
            Ok(asks_for(&[(
                "child-1",
                "run",
                json!({"cmd": "cargo test"}),
            )])),
            Ok(answer("the suite could not be run")),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("run", Approval::NotRequired)]).enveloped(starts_a_process()),
    )
    .delegating(Delegation::default());
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("the run completes");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert!(
        harness.tools.calls.is_empty(),
        "the default approver is `DenyAll`, inside a delegate as outside one"
    );
    let child = delegated(&sink);
    assert!(
        child.iter().any(|event| matches!(
            event,
            LoopEvent::ApprovalResolved {
                approved: false,
                ..
            }
        )),
        "the refusal happened inside the child: {:?}",
        child.iter().map(|event| kind(event)).collect::<Vec<_>>()
    );
    assert!(
        !sink.events().iter().any(|event| matches!(
            event,
            LoopEvent::ApprovalRequired { .. } | LoopEvent::ApprovalResolved { .. }
        )),
        "and a renderer can say who was asking, because it arrived wrapped"
    );
}

#[test]
fn a_delegate_that_runs_out_of_turns_comes_back_failed_carrying_the_bound() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "read every file"}),
            )])),
            Ok(asks_for(&[("child-1", "read", json!({"path": "a"}))])),
            Ok(answer("the delegate did not finish")),
        ]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]),
    )
    .delegating(Delegation::default().with_max_turns(1));
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("the parent completes even where its delegate did not");

    let (failed, output) = result_of(&outcome, "call-1");
    assert!(
        failed,
        "the parent has to learn the sub-task did not finish, or it reads a half-answer as whole"
    );
    assert_eq!(output["stop"], json!({"kind": "max-turns", "limit": 1}));
    assert_eq!(output["turns"], json!(1));
    assert_eq!(
        outcome.stop,
        LoopStop::Completed,
        "a child's own ceiling is not the parent's"
    );
}

#[test]
fn a_delegate_gets_what_is_left_of_the_parents_token_ceiling_and_the_parent_gets_the_bill() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            // The parent's turn reports 10 input tokens of its 100.
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "read every file"}),
            )])),
            // The child's first turn spends 200 of the 90 it was carved.
            Ok(reporting(
                asks_for(&[("child-1", "read", json!({"path": "a"}))]),
                200,
            )),
        ]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]),
    )
    .delegating(Delegation::default())
    .budgeted(Budget {
        max_input_tokens: Some(100),
        ..Budget::default()
    });
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("a ceiling that binds is an outcome");

    let finished = sink
        .events()
        .iter()
        .find_map(|event| match event {
            LoopEvent::DelegateFinished { stop, .. } => Some(stop.clone()),
            _ => None,
        })
        .expect("the delegate finished, however it finished");
    assert_eq!(
        finished,
        LoopStop::MaxInputTokens {
            limit: 90,
            reported: 200
        },
        "100 the parent set, less the 10 it had already spent"
    );
    assert_eq!(
        outcome.stop,
        LoopStop::MaxInputTokens {
            limit: 100,
            reported: 210
        },
        "the parent absorbs what the child spent, so its own ceiling binds on the sum"
    );
    assert_eq!(
        outcome.usage.len(),
        2,
        "one turn each, both in the run's log"
    );
}

#[test]
fn a_parent_with_nothing_left_stops_before_starting_a_child() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "read every file"}),
            )])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(Vec::new()),
    )
    .delegating(Delegation::default())
    .budgeted(Budget {
        // Exactly what the first turn reports, so the remainder is nothing at all.
        max_input_tokens: Some(10),
        ..Budget::default()
    });
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("a budget that binds is an outcome");
    assert_eq!(
        outcome.stop,
        LoopStop::MaxInputTokens {
            limit: 10,
            reported: 10,
        },
        "the exact ceiling binds before an effectful child is started"
    );
    assert_eq!(
        harness.model.seen.len(),
        1,
        "the exact token ceiling binds after the first turn and no child ever ran"
    );
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| matches!(event, LoopEvent::DelegateStarted { .. })),
        "nothing started, so nothing says it did"
    );
}

#[test]
fn a_cancel_raised_inside_a_delegate_ends_the_child_and_then_the_parent() {
    // The token is shared, so nothing here special-cases it: the child stops on its own check and
    // the parent's next check — before the next call of this same turn — refuses the rest.
    let cancel = LoopCancel::new();
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[
                ("call-1", "delegate", json!({"task": "read every file"})),
                ("call-2", "read", json!({"path": "b"})),
            ])),
            Ok(asks_for(&[("child-1", "read", json!({"path": "a"}))])),
        ])
        .cancelling_after(2, cancel.clone()),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]),
    )
    .delegating(Delegation::default());
    harness.cancel = cancel;
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("a cancellation is an outcome, not a failure");

    assert_eq!(
        outcome.stop,
        LoopStop::Cancelled {
            reason: "the caller cancelled".to_owned()
        }
    );
    let (failed, output) = result_of(&outcome, "call-1");
    assert!(failed);
    assert_eq!(output["stop"]["kind"], json!("cancelled"));
    assert_eq!(
        result_of(&outcome, "call-2"),
        (true, json!("the run was cancelled before this call ran")),
        "every call the model made is answered, or the run could not be resumed at all"
    );
    assert!(
        harness.tools.calls.is_empty(),
        "neither the child's read nor the parent's ran"
    );
}

#[test]
fn a_delegate_without_a_usable_task_is_refused_and_no_child_runs() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[
                ("call-1", "delegate", json!({})),
                ("call-2", "delegate", json!({"task": ""})),
                ("call-3", "delegate", json!({"task": "   "})),
                ("call-4", "delegate", json!({"task": 7})),
            ])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(Vec::new()),
    )
    .delegating(Delegation::default());
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("the run recovers");

    for id in ["call-1", "call-2", "call-3", "call-4"] {
        let (failed, output) = result_of(&outcome, id);
        assert!(failed, "{id}");
        assert!(
            output
                .as_str()
                .unwrap_or_default()
                .contains("non-empty string"),
            "{id}: {output}"
        );
    }
    assert_eq!(harness.model.seen.len(), 2, "both turns are the parent's");
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| matches!(event, LoopEvent::DelegateStarted { .. }))
    );
}

#[test]
fn a_delegate_is_never_batched_with_the_reads_around_it() {
    // It is not a port call at all, and it holds the model port for a whole second run: a port
    // asked to run two of those side by side would be running two loops through one client.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[
                ("call-1", "read", json!({"path": "a"})),
                ("call-2", "read", json!({"path": "b"})),
                ("call-3", "delegate", json!({"task": "and the rest"})),
                ("call-4", "read", json!({"path": "c"})),
            ])),
            Ok(answer("the delegate reported")),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]),
    )
    .delegating(Delegation::default());
    let (outcome, _) = harness.run();
    let outcome = outcome.expect("the turn completes");

    assert_eq!(
        harness.tools.batches,
        vec![2],
        "the two leading reads, and nothing else: the delegate is its own barrier"
    );
    assert_eq!(
        harness.tools.calls.len(),
        3,
        "three reads reached the port and the delegate did not"
    );
    assert_eq!(
        answered(&outcome),
        vec!["call-1", "call-2", "call-3", "call-4"],
        "outcomes are positional, and the order the model asked in is the order it reads"
    );
}

/// What every delegate of a run reported as its own stop, in order.
fn delegate_stops(sink: &VecLoopSink) -> Vec<LoopStop> {
    sink.events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::DelegateFinished { stop, .. } => Some(stop.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_child_that_failed_still_spent_the_parents_budget_and_the_next_carve_knows_it() {
    // The child's `Usage` and `Cost` events went out, wrapped, as its turns happened. A parent
    // that absorbed only a child that *finished* would report totals smaller than the record it
    // emitted, hand the next delegate a remainder that is already gone, and never let a ceiling
    // see the spend at all.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            // Two sub-tasks in one turn, and 10 of the parent's 100 tokens reported for it.
            Ok(asks_for(&[
                (
                    "call-1",
                    "delegate",
                    json!({"task": "survey the first half"}),
                ),
                (
                    "call-2",
                    "delegate",
                    json!({"task": "survey the second half"}),
                ),
            ])),
            // The first child spends 50 of the 90 it was carved, and then its wire dies for good.
            Ok(reporting(
                asks_for(&[("child-1", "read", json!({"path": "a"}))]),
                50,
            )),
            Err(WireError::protocol("the stream closed mid-answer")),
            // The second child is carved 40 — the 100 less the parent's 10 and the first child's
            // 50 — and its first turn crosses it.
            Ok(reporting(
                asks_for(&[("child-2", "read", json!({"path": "b"}))]),
                45,
            )),
        ]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]),
    )
    .delegating(Delegation::default())
    .priced(scripted_card())
    .budgeted(Budget {
        max_input_tokens: Some(100),
        ..Budget::default()
    });
    let (outcome, sink) = harness.run();
    let outcome = outcome.expect("a ceiling that binds is an outcome, not an error");

    let (failed, output) = result_of(&outcome, "call-1");
    assert!(
        failed,
        "the parent has to learn the sub-task did not finish"
    );
    assert!(
        output
            .as_str()
            .unwrap_or_default()
            .contains("the delegate could not run"),
        "{output}"
    );
    let stops = delegate_stops(&sink);
    assert!(
        matches!(&stops[0], LoopStop::ProviderIncomplete { reason } if reason.contains("the stream closed mid-answer")),
        "{stops:?}"
    );
    assert_eq!(
        stops[1],
        LoopStop::MaxInputTokens {
            limit: 40,
            reported: 45
        },
        "the second delegate's carve is what the first one left, however the first one ended"
    );
    assert_eq!(
        outcome.usage.len(),
        3,
        "the parent's turn and the one each child paid for before it stopped"
    );
    assert_eq!(outcome.total_tokens(), Some((105, 15)));
    assert_eq!(
        outcome.cost_micro_usd,
        Some(135),
        "20 µ$ the parent, 60 the first child, 55 the second"
    );
    assert_eq!(
        outcome.stop,
        LoopStop::MaxInputTokens {
            limit: 100,
            reported: 105
        },
        "the parent's own ceiling binds on the sum, before its next turn"
    );
    assert_eq!(
        harness.model.seen.len(),
        4,
        "one parent turn and three child ones: the parent never got a second"
    );
}

#[test]
fn a_delegate_whose_report_is_too_large_comes_back_refused_by_name_rather_than_cut_down() {
    // A shortened report reads to the parent exactly like a whole one, and it would act on half a
    // survey believing it had all of it. The preamble is what tells a child to report well inside
    // the bound; this is what happens when it did not.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "read every file and quote all of it"}),
            )])),
            Ok(answer(&"x".repeat(MAX_TOOL_RESULT_BYTES))),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(Vec::new()),
    )
    .delegating(Delegation::default());
    let (outcome, sink) = harness.run();
    let outcome =
        outcome.expect("the parent completes even where its delegate's report did not fit");

    let (failed, output) = result_of(&outcome, "call-1");
    assert!(failed, "the parent has to learn it was given nothing");
    let text = output.as_str().expect("the refusal is text");
    assert!(text.contains("`delegate`"), "refused by name: {text}");
    assert!(text.contains("bound"), "{text}");
    assert!(
        text.len() < MAX_TOOL_RESULT_BYTES,
        "the oversized report must not be forwarded"
    );
    assert_eq!(
        delegate_stops(&sink),
        vec![LoopStop::Completed],
        "the child finished; it is the size of what it said that the parent is refused"
    );
}

// --- hooks and the tools the loop owns ------------------------------------------------------

#[test]
fn a_before_call_hook_speaks_about_an_owned_call_and_its_block_stops_a_whole_delegate() {
    // A `before-call` declaration with no `tools` filter means every call (design 0002 § 3), and
    // `delegate` is a call. Nothing else in the run would have stopped this one: the delegate's
    // own envelope asks nobody, so the operator's guard is the only thing that can say no to a
    // sub-agent before it starts.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "read every file"}),
            )])),
            Ok(answer("understood")),
        ]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]),
    )
    .delegating(Delegation::default());
    let mut hooks =
        ScriptedHooks::default().before(vec![HookDecision::block("no sub-agents today")]);
    let (outcome, sink) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("a block is not a failure");

    assert_eq!(
        harness.model.seen.len(),
        2,
        "both turns are the parent's: no child ever ran"
    );
    assert!(
        !sink
            .events()
            .iter()
            .any(|event| matches!(event, LoopEvent::DelegateStarted { .. })),
        "nothing started, so nothing says it did"
    );
    assert_eq!(
        result_of(&outcome, "call-1"),
        (
            true,
            json!("`delegate` was blocked by a hook: no sub-agents today")
        ),
        "the same refusal a port call gets, naming the tool the loop owns"
    );
    assert_eq!(
        hook_runs(&sink),
        vec![
            (
                "before-call",
                Some("call-1"),
                HookDecision::block("no sub-agents today")
            ),
            ("stop", None, HookDecision::Proceed),
        ],
        "the record says which hook decided what, for an owned call as for any other"
    );
    assert_eq!(
        hooks.asked_at(HookPoint::BeforeCall),
        vec![("delegate", "delegate")],
        "the loop's own tool is its own entry"
    );
    assert!(
        hooks.asked_at(HookPoint::AfterCall).is_empty(),
        "there is no outcome to read when nothing ran"
    );
}

#[test]
fn an_after_call_note_on_the_answer_reaches_the_model_beside_the_accepted_flag() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![Ok(asks_for(&[(
            "call-1",
            "answer",
            json!({"verdict": "green"}),
        )]))]),
        ScriptedTools::new(Vec::new()),
    )
    .answering_in(verdict_schema());
    let mut hooks =
        ScriptedHooks::default().noting(vec![AfterCall::note("the schema check passed")]);
    let (outcome, sink) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("the answered run completes");

    assert_eq!(
        result_of(&outcome, "call-1"),
        (
            false,
            json!({"accepted": true, "hook_notes": ["the schema check passed"]})
        ),
        "the note lands where the model reads it: beside the result, never instead of it"
    );
    assert_eq!(
        outcome.structured,
        Some(json!({"verdict": "green"})),
        "a note is not a verdict, and the answer is what the model sent"
    );
    assert_eq!(
        hooks.asked_at(HookPoint::AfterCall),
        vec![("answer", "answer")]
    );
    assert_eq!(
        hook_runs(&sink),
        vec![
            ("before-call", Some("call-1"), HookDecision::Proceed),
            ("after-call", Some("call-1"), HookDecision::Proceed),
            ("stop", None, HookDecision::Proceed),
        ]
    );
}

#[test]
fn the_stop_hook_is_not_asked_about_a_delegates_ending_though_the_childs_calls_still_meet_it() {
    // A child's ending is not the run's ending: the parent is still inside the tool call and will
    // go on turning afterwards. A stop hook consulted there would be asked *has this run
    // finished?* about a run nobody started, and a block would turn the child again — three times
    // per delegate, each time on budget the parent carved.
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "survey the files"}),
            )])),
            Ok(asks_for(&[("child-1", "read", json!({"path": "a"}))])),
            Ok(answer("all three are green")),
            Ok(answer("understood")),
            Ok(answer("still understood")),
            Ok(answer("understood a third time")),
            Ok(answer("and a fourth")),
        ]),
        ScriptedTools::new(vec![spec("read", Approval::NotRequired)]),
    )
    .delegating(Delegation::default());
    let mut hooks = ScriptedHooks::default().stopping(vec![
        HookDecision::block("keep going"),
        HookDecision::block("keep going"),
        HookDecision::block("keep going"),
        HookDecision::block("keep going"),
    ]);
    let (outcome, sink) = run_hooked(&mut harness, &mut hooks);
    let outcome = outcome.expect("an exhausted stop hook ends the run, it does not fail it");

    assert_eq!(outcome.stop, LoopStop::Completed);
    let inside: Vec<(&str, HookDecision)> = delegated(&sink)
        .into_iter()
        .filter_map(|event| match event {
            LoopEvent::HookRan {
                point, decision, ..
            } => Some((point.as_str(), decision.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        inside,
        vec![
            ("before-call", HookDecision::Proceed),
            ("after-call", HookDecision::Proceed),
        ],
        "the child's own call meets both other points; the child's ending meets none"
    );
    assert_eq!(
        delegate_stops(&sink),
        vec![LoopStop::Completed],
        "the child ended as it meant to"
    );
    assert!(
        sink.events().iter().any(|event| matches!(
            event,
            LoopEvent::DelegateFinished { turns, .. } if *turns == 2
        )),
        "exactly the two turns the script gave it: a block at its end would have added more"
    );
    assert_eq!(
        hook_runs(&sink),
        vec![
            ("before-call", Some("call-1"), HookDecision::Proceed),
            ("after-call", Some("call-1"), HookDecision::Proceed),
            ("stop", None, HookDecision::block("keep going")),
            ("stop", None, HookDecision::block("keep going")),
            ("stop", None, HookDecision::block("keep going")),
            ("stop", None, HookDecision::block("keep going")),
        ],
        "the parent's ending is the only one a stop hook is asked about"
    );
    assert!(
        sink.warnings()
            .any(|(code, _)| code == "stop-hook-exhausted"),
        "and it runs out of continues on the parent, as it always did"
    );
    assert_eq!(
        harness.model.seen.len(),
        7,
        "five parent turns and the child's two"
    );
}

#[test]
fn a_narrowed_run_is_offered_less_and_refused_the_rest_by_the_same_rule() {
    // The property a named agent turns on. `delegate.rs` says delegation widens nothing — the
    // child does exactly what the parent's catalogue admits — and an agent declaring a toolset
    // must only be able to take away from that. A filter that hid a tool from the list but let a
    // call through would be a permission boundary that is not one: the model has the name from
    // its own instructions, and guessing it would be enough.
    let mut narrowed = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[("call-1", "write", json!({}))])),
            Ok(answer("done")),
        ]),
        ScriptedTools::new(vec![
            spec("read", Approval::NotRequired),
            spec("write", Approval::NotRequired),
        ]),
    );
    narrowed.config = narrowed
        .config
        .clone()
        .with_admitted(Some(vec![tool_name("read")]));
    let (_, sink) = narrowed.run();

    assert_eq!(
        published_names(&narrowed.model.seen[0]),
        vec!["read"],
        "the model is offered only what it was admitted"
    );
    assert!(
        narrowed.tools.calls.is_empty(),
        "and a call naming the tool that was filtered out never reaches the port"
    );
    assert!(
        sink.events().iter().any(|event| matches!(
            event,
            LoopEvent::Warning { code, .. } if code == "unpublished-tool"
        )),
        "refused by the rule that already refuses a tool the run never published, rather than by \
         a second rule that could disagree with it: {:?}",
        bare_kinds(&sink)
    );
}

#[test]
fn a_narrowed_run_is_refused_the_entry_behind_a_verb_by_the_same_rule() {
    // The sibling above, under the surface where the call does not name the tool. Behind three
    // verbs the model calls `tool_invoke` and the entry is an argument, so a gate reading the
    // call's own name sees `tool_invoke` on every call and never `file_write` — a narrowing that
    // reports as one and is not, which is worse than none because the run's record claims it held.
    //
    // What decides is the entry the port says the call **invokes**, which is already the rule for
    // the approver, for the event, for the refusal text and — design 0002 § 2 — for a hook. This
    // is that same rule reaching one more gate, not a second gate beside it.
    let agents = Agents::new(vec![Agent {
        name: "reader".to_owned(),
        description: "Reads and reports.".to_owned(),
        tools: vec!["file_read".to_owned()],
        instructions: "You only read.".to_owned(),
    }]);
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "look", "agent": "reader"}),
            )])),
            // The child reaches, through the verb, the entry its author did not grant it.
            Ok(asks_for(&[(
                "call-2",
                INVOKE_VERB,
                json!({"name": "file_write", "arguments": {"path": "a", "text": "b"}}),
            )])),
            // And then the one it did.
            Ok(asks_for(&[(
                "call-3",
                INVOKE_VERB,
                json!({"name": "file_read", "arguments": {"path": "a"}}),
            )])),
            Ok(answer("child done")),
            Ok(answer("parent done")),
        ]),
        ScriptedTools::new(vec![
            spec("tool_search", Approval::NotRequired),
            spec("tool_describe", Approval::NotRequired),
            spec(INVOKE_VERB, Approval::NotRequired),
        ])
        .over(vec![
            spec("file_read", Approval::NotRequired),
            spec("file_write", Approval::NotRequired),
        ]),
    );
    harness.config = harness
        .config
        .clone()
        .with_delegation(Some(Delegation::default()))
        .with_agents(Some(agents));
    let (_, sink) = harness.run();

    let child = harness
        .model
        .seen
        .iter()
        .find(|request| request.instructions.contains("You only read."))
        .expect("the agent's own body reaches the child, after the delegate preamble");
    assert_eq!(
        delegated(&sink)
            .iter()
            .filter_map(|event| match event {
                LoopEvent::Warning { code, message } if code == "unpublished-tool" =>
                    Some(message.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec!["the model called `file_write`, which this run was not admitted"],
        "refused by the rule that already refuses a tool the run never published, naming the \
         entry rather than the verb it arrived through — the same message a flat surface gives: \
         {:?}",
        delegated(&sink)
            .iter()
            .map(|event| kind(event))
            .collect::<Vec<_>>()
    );
    let reached: Vec<&str> = harness
        .tools
        .calls
        .iter()
        .filter_map(|call| call.arguments.get("name").and_then(Value::as_str))
        .collect();
    assert_eq!(
        reached,
        vec!["file_read"],
        "the entry its author granted ran; the one it did not never reached the port"
    );
    assert_eq!(
        published_names(child),
        vec!["tool_search", "tool_describe", INVOKE_VERB],
        "the verbs are the route to every entry, so a narrowing takes entries away and never the \
         route: a child published nothing at all could not reach the entry it *was* granted"
    );
}

#[test]
fn a_narrowed_run_is_refused_an_ungranted_entry_behind_a_verb_beside_a_neighbour() {
    // The same narrowing as the test above, in the one shape the model chooses freely: **two calls
    // in one turn**. `AgentLoop::run_calls` sends a run of neighbouring `batchable` calls to
    // `run_batch`, which hands them to the port directly — `AgentLoop::invoke` is the only place
    // the narrowing is checked, and a batched call never goes down it.
    //
    // Under a flat surface that is harmless, because `batchable` first asks `published`, and
    // `port_specs` has already taken an unadmitted tool out of the published list. Under a verb
    // surface the published name is `tool_invoke`, which `AgentLoop::routes` keeps published for
    // every narrowing however tight — so `batchable` says yes to a call whose *entry* the run was
    // never admitted, and the entry runs.
    //
    // An agent granted `[Grep]` must not be able to read a file. Here it asks for one beside a
    // search it *was* granted, and the pair is a batch.
    let agents = Agents::new(vec![Agent {
        name: "grepper".to_owned(),
        description: "Searches and reports.".to_owned(),
        tools: vec!["search".to_owned()],
        instructions: "You only search.".to_owned(),
    }]);
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "look", "agent": "grepper"}),
            )])),
            // One turn, two calls, neighbours: the entry it was never granted first, the one it
            // was second. Both are reads, so neither ends the group.
            Ok(asks_for(&[
                (
                    "call-2",
                    INVOKE_VERB,
                    json!({"name": "file_read", "arguments": {"path": "/etc/shadow"}}),
                ),
                (
                    "call-3",
                    INVOKE_VERB,
                    json!({"name": "search", "arguments": {"query": "x"}}),
                ),
            ])),
            Ok(answer("child done")),
            Ok(answer("parent done")),
        ]),
        ScriptedTools::new(vec![
            spec("tool_search", Approval::NotRequired),
            spec("tool_describe", Approval::NotRequired),
            spec(INVOKE_VERB, Approval::NotRequired),
        ])
        .over(vec![
            spec("file_read", Approval::NotRequired),
            spec("search", Approval::NotRequired),
            spec("file_write", Approval::NotRequired),
        ]),
    );
    harness.config = harness
        .config
        .clone()
        .with_delegation(Some(Delegation::default()))
        .with_agents(Some(agents));
    let (_, sink) = harness.run();

    let reached: Vec<&str> = harness
        .tools
        .calls
        .iter()
        .filter_map(|call| call.arguments.get("name").and_then(Value::as_str))
        .collect();
    assert_eq!(
        reached,
        vec!["search"],
        "the entry its author granted ran; `file_read` was never granted and must not reach the \
         port because it happened to be asked for beside one that was. Batches: {:?}",
        harness.tools.batches
    );
    assert_eq!(
        delegated(&sink)
            .iter()
            .filter_map(|event| match event {
                LoopEvent::Warning { code, message } if code == "unpublished-tool" =>
                    Some(message.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec!["the model called `file_read`, which this run was not admitted"],
        "the same rule, and so the same message, whether the call stood alone or beside another"
    );
}

#[test]
fn a_narrowed_flat_run_refuses_an_ungranted_read_beside_a_neighbour() {
    // The control for the test above, and the half of the acceptance that says *one rule, not two
    // that can drift*: the identical turn under a flat surface. This one is green, which is what
    // makes the verb surface's answer a drift and not a shared limitation.
    let agents = Agents::new(vec![Agent {
        name: "grepper".to_owned(),
        description: "Searches and reports.".to_owned(),
        tools: vec!["search".to_owned()],
        instructions: "You only search.".to_owned(),
    }]);
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "look", "agent": "grepper"}),
            )])),
            Ok(asks_for(&[
                ("call-2", "file_read", json!({"path": "/etc/shadow"})),
                ("call-3", "search", json!({"query": "x"})),
            ])),
            Ok(answer("child done")),
            Ok(answer("parent done")),
        ]),
        ScriptedTools::new(vec![
            spec("file_read", Approval::NotRequired),
            spec("search", Approval::NotRequired),
            spec("file_write", Approval::NotRequired),
        ]),
    );
    harness.config = harness
        .config
        .clone()
        .with_delegation(Some(Delegation::default()))
        .with_agents(Some(agents));
    let (_, sink) = harness.run();

    let reached: Vec<&str> = harness
        .tools
        .calls
        .iter()
        .map(|call| call.name.as_str())
        .collect();
    assert_eq!(
        reached,
        vec!["search"],
        "flat: the ungranted read never reaches the port, batched or not. Batches: {:?}",
        harness.tools.batches
    );
    assert_eq!(
        delegated(&sink)
            .iter()
            .filter_map(|event| match event {
                LoopEvent::Warning { code, message } if code == "unpublished-tool" =>
                    Some(message.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec!["the model called `file_read`, which this run was not admitted"],
    );
}

#[test]
fn a_narrowed_child_cannot_widen_itself_by_delegating_again_without_an_agent() {
    // `delegate.rs`: *"Delegation widens nothing: the child can do exactly what the parent's
    // catalogue admits, entry for entry."* `AgentLoop::agent_for` says the same in its own words —
    // *"a child of a narrowed run must not be able to climb back out by naming an agent"* — and
    // the `delegate` tool's published description promises it to the model: *"Omit for a delegate
    // with these same tools."*
    //
    // A child *can* climb back out by **not** naming one. `prepare_child` sets
    // `admits: agent.as_ref().map(...)`, so an agentless delegate is handed `None` — the whole
    // port — whatever its parent was narrowed to. The intersection `agent_for` computed for that
    // case is thrown away.
    //
    // Two levels, because that is where a narrowing already exists to be escaped from: the run
    // delegates to `reader`, granted `file_read` alone, and `reader` delegates once more with no
    // agent. The grandchild writes.
    let agents = Agents::new(vec![Agent {
        name: "reader".to_owned(),
        description: "Reads and reports.".to_owned(),
        tools: vec!["file_read".to_owned()],
        instructions: "You only read.".to_owned(),
    }]);
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "look", "agent": "reader"}),
            )])),
            // The narrowed child, delegating again and naming no agent.
            Ok(asks_for(&[(
                "call-2",
                "delegate",
                json!({"task": "keep looking"}),
            )])),
            // The grandchild, doing what neither it nor its parent was granted.
            Ok(asks_for(&[(
                "call-3",
                "file_write",
                json!({"path": "a", "text": "b"}),
            )])),
            Ok(answer("grandchild done")),
            Ok(answer("child done")),
            Ok(answer("parent done")),
        ]),
        ScriptedTools::new(vec![
            spec("file_read", Approval::NotRequired),
            spec("file_write", Approval::NotRequired),
        ]),
    );
    harness.config = harness
        .config
        .clone()
        .with_delegation(Some(Delegation {
            depth: 2,
            ..Delegation::default()
        }))
        .with_agents(Some(agents));
    let (_, _sink) = harness.run();

    let reached: Vec<&str> = harness
        .tools
        .calls
        .iter()
        .map(|call| call.name.as_str())
        .collect();
    assert_eq!(
        reached,
        Vec::<&str>::new(),
        "a grandchild of a run narrowed to `file_read` may do no more than `file_read`; nothing \
         here should have reached the port at all"
    );
}

#[test]
fn a_port_that_under_reports_its_reach_narrows_a_run_to_nothing_and_not_to_everything() {
    // `ToolPort::reachable`'s own doc, which is the safety argument for defaulting the method:
    // *"A port that under-reports here narrows more than it should and never less: a published
    // name absent from this list is a route, and a route publishes nothing of its own, so the
    // caller ends up admitting nothing rather than admitting everything."* `AgentLoop::routes`
    // repeats it: *"Empty, too, for a port that under-reports `reachable`."*
    //
    // `routes` is `specs() \ reachable()`. Under-reporting makes that set **larger**, not empty —
    // a port that answers `reachable` with nothing turns every tool it publishes into a route, and
    // `AgentLoop::admits` admits every route whatever the narrowing says. The direction the doc
    // rules out is the direction it goes.
    //
    // This matters because `reachable` is defaulted: any port outside this crate can get it wrong,
    // and the doc tells its author the cost is reach.
    let agents = Agents::new(vec![Agent {
        name: "reader".to_owned(),
        description: "Reads and reports.".to_owned(),
        tools: vec!["file_read".to_owned()],
        instructions: "You only read.".to_owned(),
    }]);
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "look", "agent": "reader"}),
            )])),
            Ok(asks_for(&[(
                "call-2",
                "file_write",
                json!({"path": "a", "text": "b"}),
            )])),
            Ok(answer("child done")),
            Ok(answer("parent done")),
        ]),
        ScriptedTools::new(vec![
            spec("file_read", Approval::NotRequired),
            spec("file_write", Approval::NotRequired),
        ])
        .hiding_reach(),
    );
    harness.config = harness
        .config
        .clone()
        .with_delegation(Some(Delegation::default()))
        .with_agents(Some(agents));
    let (_, _sink) = harness.run();

    let child = harness
        .model
        .seen
        .iter()
        .find(|request| request.instructions.contains("You only read."))
        .expect("the agent's own body reaches the child");
    let reached: Vec<&str> = harness
        .tools
        .calls
        .iter()
        .map(|call| call.name.as_str())
        .collect();
    assert_eq!(
        reached,
        Vec::<&str>::new(),
        "a wrong `reachable` costs reach and never boundary: this child's grant met nothing, so \
         nothing may run. It was published {:?} and its record withholds `file_read`",
        published_names(child)
    );
}

#[test]
fn a_port_that_under_reports_part_of_its_reach_still_costs_reach_and_not_boundary() {
    // The sibling of `a_port_that_under_reports_its_reach_narrows_a_run_to_nothing...`, for the
    // half-wrong answer rather than the wholly wrong one — and the case that decides whether
    // `ToolPort::reachable`'s stated failure direction is true or merely true of one input.
    //
    // The port publishes `file_read` and `file_write` and names only `file_read` as reachable. The
    // grant that comes out of that is `[file_read]`: **non-empty**, so nothing about an empty grant
    // can be what refuses the write. `AgentLoop::routes` computed as `specs() \ reachable()` made
    // `file_write` a route here and admitted it unconditionally — a port that said less about
    // itself got a child that could do more. Asked positively — *does this grant hold a name the
    // port does not publish?* — it cannot: `file_read` is published, so the run needs no
    // indirection and is given none.
    let agents = Agents::new(vec![Agent {
        name: "reader".to_owned(),
        description: "Reads and reports.".to_owned(),
        tools: vec!["file_read".to_owned()],
        instructions: "You only read.".to_owned(),
    }]);
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "look", "agent": "reader"}),
            )])),
            Ok(asks_for(&[(
                "call-2",
                "file_write",
                json!({"path": "a", "text": "b"}),
            )])),
            // And the one it was granted, so this cannot pass by refusing everything.
            Ok(asks_for(&[("call-3", "file_read", json!({"path": "a"}))])),
            Ok(answer("child done")),
            Ok(answer("parent done")),
        ]),
        ScriptedTools::new(vec![
            spec("file_read", Approval::NotRequired),
            spec("file_write", Approval::NotRequired),
        ])
        .reaching_only(&["file_read"]),
    );
    harness.config = harness
        .config
        .clone()
        .with_delegation(Some(Delegation::default()))
        .with_agents(Some(agents));
    let (_, sink) = harness.run();

    assert_eq!(
        harness
            .tools
            .calls
            .iter()
            .map(|call| call.name.as_str())
            .collect::<Vec<_>>(),
        vec!["file_read"],
        "the granted read ran and the ungranted write did not: a port that under-reports its \
         reach costs the run reach, never boundary"
    );
    assert_eq!(
        delegated(&sink)
            .iter()
            .filter_map(|event| match event {
                LoopEvent::Warning { code, message } if code == "unpublished-tool" =>
                    Some(message.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        vec!["the model called `file_write`, which this run was not admitted"],
        "and it is refused by the narrowing, in the narrowing's own words"
    );
}

#[test]
fn a_delegate_run_as_a_named_agent_is_narrowed_to_what_its_author_declared() {
    // The property the whole named-agent feature turns on. An agent file says
    // `tools: [Read, Grep]`; the child must get those and not the parent's `write`. The failure
    // this prevents is the quiet one: an agent its author wrote as read-only silently handed
    // write and exec, with the record showing a child that had the parent's whole catalogue.
    let agents = Agents::new(vec![Agent {
        name: "reader".to_owned(),
        description: "Reads and reports.".to_owned(),
        tools: vec!["read".to_owned(), "absent".to_owned()],
        instructions: "You only read.".to_owned(),
    }]);
    let mut harness = Harness::new(
        ScriptedModel::new(vec![
            Ok(asks_for(&[(
                "call-1",
                "delegate",
                json!({"task": "look", "agent": "reader"}),
            )])),
            // The child's own turn: it names the tool its author did not give it.
            Ok(asks_for(&[("call-2", "write", json!({}))])),
            Ok(answer("child done")),
            Ok(answer("parent done")),
        ]),
        ScriptedTools::new(vec![
            spec("read", Approval::NotRequired),
            spec("write", Approval::NotRequired),
        ]),
    );
    harness.config = harness
        .config
        .clone()
        .with_delegation(Some(Delegation::default()))
        .with_agents(Some(agents));
    let (_, _sink) = harness.run();

    let child_request = harness
        .model
        .seen
        .iter()
        .find(|request| request.instructions.contains("You only read."))
        .expect("the agent's own body reaches the child, after the delegate preamble");
    assert_eq!(
        published_names(child_request),
        vec!["read"],
        "the child is offered what its author declared, intersected with what the parent had — \
         never `write`, which the agent did not ask for, and never `absent`, which the parent \
         does not have"
    );
    assert!(
        harness
            .tools
            .calls
            .iter()
            .all(|call| call.name.as_str() != "write"),
        "and naming it anyway does not reach the port"
    );
}

// --- a call outside a turn ------------------------------------------------------------------------

/// A `command` step of a workflow (`harness-cli` design 0003 § 6) is one call the document made and
/// no turn: the model is never asked, and everything a model's call meets — the approver, the
/// operator's `before-call` hook, the tool, `after-call` — is met in the same order and recorded
/// under the same events.
#[test]
fn a_call_outside_a_turn_meets_the_gate_a_model_call_meets_and_asks_no_model() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![]),
        ScriptedTools::new(vec![spec("run", Approval::Required)])
            .answering("run", ToolOutcome::ok(json!({"exit": 0}))),
    )
    .approving(Box::new(ApproveAll));
    let mut hooks = ScriptedHooks::default();
    let mut sink = VecLoopSink::new();
    let call = ToolCall {
        call_id: call_id("flow-command-1"),
        name: tool_name("run"),
        arguments: json!({"argv": ["cargo", "test"]}),
    };

    let outcome = AgentLoop::new(
        &mut harness.model,
        &mut harness.tools,
        harness.approvals.as_mut(),
        harness.config.clone(),
    )
    .with_hooks(&mut hooks)
    .call(&call, &mut sink);

    assert!(!outcome.failed, "{outcome:?}");
    assert_eq!(outcome.output, json!({"exit": 0}));
    assert_eq!(harness.tools.calls.len(), 1, "the tool ran once");
    assert!(
        harness.model.seen.is_empty(),
        "and no request was ever made: {:?}",
        harness.model.seen
    );
    assert_eq!(
        approvals(&sink),
        vec!["asked flow-command-1", "flow-command-1 approved=true"]
    );
    assert_eq!(hooks.asked_at(HookPoint::BeforeCall), vec![("run", "run")]);
    assert_eq!(hooks.asked_at(HookPoint::AfterCall), vec![("run", "run")]);
    let kinds: Vec<&str> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::ToolRequested(requested) => Some(requested.name.as_str()),
            LoopEvent::ToolCompleted { failed: false, .. } => Some("completed"),
            LoopEvent::ToolCompleted { failed: true, .. } => Some("completed-failed"),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        ["run", "completed"],
        "the record a model's call leaves"
    );
}

/// The same call under the default approver is refused before the hook and before the tool — a
/// person's no is not something a document gets around by not asking a model.
#[test]
fn a_call_outside_a_turn_is_refused_by_the_same_approver_and_reaches_nothing_past_it() {
    let mut harness = Harness::new(
        ScriptedModel::new(vec![]),
        ScriptedTools::new(vec![spec("run", Approval::Required)])
            .answering("run", ToolOutcome::ok(json!({"exit": 0}))),
    );
    let mut hooks = ScriptedHooks::default();
    let mut sink = VecLoopSink::new();
    let call = ToolCall {
        call_id: call_id("flow-command-1"),
        name: tool_name("run"),
        arguments: json!({"argv": ["cargo", "test"]}),
    };

    let outcome = AgentLoop::new(
        &mut harness.model,
        &mut harness.tools,
        harness.approvals.as_mut(),
        harness.config.clone(),
    )
    .with_hooks(&mut hooks)
    .call(&call, &mut sink);

    assert!(outcome.failed, "{outcome:?}");
    assert!(
        outcome
            .output
            .as_str()
            .is_some_and(|text| text.contains("was not approved")),
        "{outcome:?}"
    );
    assert!(harness.tools.calls.is_empty(), "the tool never ran");
    assert!(
        hooks.seen.is_empty(),
        "and no hook was asked about a call a person refused"
    );
    assert!(harness.model.seen.is_empty(), "and no request was made");
    assert_eq!(
        approvals(&sink),
        vec!["asked flow-command-1", "flow-command-1 approved=false"]
    );
}

// --- delegates side by side (design 0002 § 2, milestone M4) ---------------------------------------

/// What a group of children does to the model port they were forked from, watched from outside.
///
/// The interesting question a test has to answer about concurrency is *were two of them ever inside
/// a turn at the same time*, and it cannot be answered by timing a run: a fast serial run and a slow
/// parallel one look alike. So each child announces itself on the way in, waits for its siblings,
/// and the high-water mark is read afterwards.
///
/// The wait is **bounded**. A run that turns out to be serial has to fail this test rather than hang
/// it: the first child waits out `patience` for a sibling that is not coming, answers anyway, and
/// the peak the assertion reads is one.
struct Siblings {
    /// How many children are inside a turn right now.
    live: AtomicUsize,
    /// The most that were ever inside one at the same time. The whole point of the fixture.
    peak: AtomicUsize,
    /// How many have ever arrived, which **only goes up**.
    ///
    /// What a child waits on, and not `live`: a sibling that arrived and left again has still
    /// arrived, and waiting on the live count means the first child to arrive waits out its
    /// patience for a second that has already been and gone.
    arrived: AtomicUsize,
    /// How many children a turn waits for before answering.
    expect: usize,
    /// How long it waits for them before deciding they are not coming.
    patience: Duration,
}

impl Siblings {
    fn expecting(expect: usize) -> Arc<Self> {
        Arc::new(Self {
            live: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            arrived: AtomicUsize::new(0),
            expect,
            // Long enough that a loaded machine still sees the siblings arrive, short enough that
            // the serial case fails in a couple of seconds rather than sitting there.
            patience: Duration::from_secs(2),
        })
    }

    fn peak(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }

    /// One child's turn: arrive, wait for the others, leave.
    fn take_a_turn(&self) {
        let live = self.live.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(live, Ordering::SeqCst);
        self.arrived.fetch_add(1, Ordering::SeqCst);
        let until = Instant::now() + self.patience;
        while self.arrived.load(Ordering::SeqCst) < self.expect && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(1));
        }
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A model whose forks are what the children run on, and whose own turns are the parent's.
///
/// The split is the fixture's whole trick: the parent holds the [`SharedModel`] and every child
/// holds a [`ForkedModel`], so *the parent's script* and *what a child does* are two different
/// behaviours over one port — which is exactly the shape a real run has.
struct SharedModel {
    wire: WireId,
    parent: VecDeque<Result<TurnOutcome, WireError>>,
    children: Arc<Siblings>,
    /// Whether this port will hand out a second handle on itself at all.
    forkable: bool,
    /// A child that should panic rather than answer, by the task it was given.
    panics_on: Option<String>,
    /// Whether every child spends a turn on a tool call before reporting.
    children_read_first: bool,
}

impl SharedModel {
    fn new(parent: Vec<Result<TurnOutcome, WireError>>, children: &Arc<Siblings>) -> Self {
        Self {
            wire: wire(),
            parent: parent.into(),
            children: Arc::clone(children),
            forkable: true,
            panics_on: None,
            children_read_first: false,
        }
    }

    /// Every child calls a tool before it reports, which is what gives a run two turns to bound
    /// and one call to put to an approver.
    fn children_reading_first(mut self) -> Self {
        self.children_read_first = true;
        self
    }

    /// The same fixture over a port that refuses to fork, which is how a run falls back to order.
    fn unforkable(mut self) -> Self {
        self.forkable = false;
        self
    }

    fn panicking_on(mut self, task: &str) -> Self {
        self.panics_on = Some(task.to_owned());
        self
    }
}

impl ModelPort for SharedModel {
    fn wire(&self) -> &WireId {
        &self.wire
    }

    fn turn(
        &mut self,
        request: &TurnRequest,
        _sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        request.validate()?;
        self.parent
            .pop_front()
            .unwrap_or_else(|| Err(WireError::protocol("the parent's script ran out of turns")))
    }

    fn fork(&self) -> Option<Box<dyn ModelPort + Send + '_>> {
        self.forkable.then(|| {
            Box::new(ForkedModel {
                wire: self.wire.clone(),
                children: Arc::clone(&self.children),
                panics_on: self.panics_on.clone(),
                reads_first: self.children_read_first,
                taken: 0,
            }) as Box<dyn ModelPort + Send>
        })
    }
}

/// One child's handle on the shared model: it answers with the task it was given, once.
struct ForkedModel {
    wire: WireId,
    children: Arc<Siblings>,
    panics_on: Option<String>,
    /// Whether the child spends a turn on a tool call before it reports.
    reads_first: bool,
    /// Turns this one child has taken, so it can tell its first from its second.
    taken: usize,
}

/// The task a child was handed, which is the first user item of its conversation.
fn task_in(request: &TurnRequest) -> String {
    request
        .items
        .iter()
        .find_map(|item| match item {
            Item::UserText { text } => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

impl ModelPort for ForkedModel {
    fn wire(&self) -> &WireId {
        &self.wire
    }

    fn turn(
        &mut self,
        request: &TurnRequest,
        _sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        request.validate()?;
        let task = task_in(request);
        self.taken += 1;
        self.children.take_a_turn();
        assert!(
            self.panics_on.as_deref() != Some(task.as_str()),
            "the child given `{task}` was told to panic"
        );
        if self.reads_first && self.taken == 1 {
            return Ok(asks_for(&[("child-read", "read", json!({"path": task}))]));
        }
        Ok(answer(&format!("done: {task}")))
    }
}

/// A toolset a whole group of children can hold at once, recording what all of them called.
struct SharedTools {
    specs: Vec<ToolSpec>,
    calls: Arc<Mutex<Vec<ToolCall>>>,
    state: Arc<Mutex<Option<String>>>,
    envelope: Option<Envelope>,
    forkable: bool,
}

impl SharedTools {
    fn new(specs: Vec<ToolSpec>) -> Self {
        Self {
            specs,
            calls: Arc::new(Mutex::new(Vec::new())),
            state: Arc::new(Mutex::new(None)),
            envelope: None,
            forkable: true,
        }
    }

    fn enveloped(mut self, envelope: Envelope) -> Self {
        self.envelope = Some(envelope);
        self
    }

    fn handle(&self) -> Self {
        Self {
            specs: self.specs.clone(),
            calls: Arc::clone(&self.calls),
            state: Arc::clone(&self.state),
            envelope: self.envelope.clone(),
            forkable: self.forkable,
        }
    }

    fn state(&self) -> Option<String> {
        self.state.lock().expect("the shared tool state").clone()
    }
}

impl ToolPort for SharedTools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn invoked(&self, call: &ToolCall) -> Option<ToolSpec> {
        let published = self.specs.iter().find(|spec| spec.name == call.name)?;
        Some(match &self.envelope {
            Some(envelope) => ToolSpec {
                envelope: envelope.clone(),
                ..published.clone()
            },
            None => published.clone(),
        })
    }

    fn reachable_specs(&self) -> Vec<ToolSpec> {
        self.specs
            .iter()
            .cloned()
            .map(|published| match &self.envelope {
                Some(envelope) => ToolSpec {
                    envelope: envelope.clone(),
                    ..published
                },
                None => published,
            })
            .collect()
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        self.calls.lock().expect("the call log").push(call.clone());
        if let Some(value) = call.arguments.get("value").and_then(Value::as_str) {
            *self.state.lock().expect("the shared tool state") = Some(value.to_owned());
        }
        ToolOutcome::ok(json!({"read": call.name.as_str()}))
    }

    fn fork(&self) -> Option<Box<dyn ToolPort + Send + '_>> {
        self.forkable
            .then(|| Box::new(self.handle()) as Box<dyn ToolPort + Send>)
    }
}

/// One run over ports a test built itself, rather than over [`Harness`]'s scripted pair.
fn run_ports(
    model: &mut dyn ModelPort,
    tools: &mut dyn ToolPort,
    approvals: &mut dyn ApprovalPort,
    config: LoopConfig,
) -> (Result<LoopOutcome, LoopError>, VecLoopSink) {
    let mut sink = VecLoopSink::new();
    let outcome = AgentLoop::new(model, tools, approvals, config).run("do the thing", &mut sink);
    (outcome, sink)
}

/// A parent that asks for two delegates in one turn and then answers.
fn asks_for_two_delegates() -> Vec<Result<TurnOutcome, WireError>> {
    vec![
        Ok(asks_for(&[
            ("call-1", "delegate", json!({"task": "left"})),
            ("call-2", "delegate", json!({"task": "right"})),
        ])),
        Ok(answer("both reported")),
    ]
}

fn delegating_config(delegation: Delegation) -> LoopConfig {
    LoopConfig::new("scripted-model", "be useful").with_delegation(Some(delegation))
}

#[test]
fn two_delegates_of_one_turn_run_at_the_same_time() {
    let siblings = Siblings::expecting(2);
    let mut model = SharedModel::new(asks_for_two_delegates(), &siblings);
    let mut tools = SharedTools::new(vec![spec("read", Approval::NotRequired)]);
    let (outcome, sink) = run_ports(
        &mut model,
        &mut tools,
        &mut DenyAll,
        delegating_config(Delegation::default()),
    );
    let outcome = outcome.expect("a run whose delegates both report completes");

    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(
        siblings.peak(),
        2,
        "both children have to have been inside a turn at once; a peak of one is a run that \
         delegated in order"
    );
    assert_eq!(
        result_of(&outcome, "call-1").1["text"],
        json!("done: left"),
        "the first call's result is the first child's, whatever order they finished in"
    );
    assert_eq!(
        result_of(&outcome, "call-2").1["text"],
        json!("done: right")
    );
    assert_eq!(
        bracketing(&sink),
        vec![
            ("delegate-started", "call-1"),
            ("delegate-started", "call-2"),
            ("delegate-finished", "call-1"),
            ("delegate-finished", "call-2"),
        ],
        "both announced before either ends, which is what a reader of the record reads the \
         concurrency off — and each announced exactly once"
    );
}

#[test]
fn delegates_run_in_order_when_any_reachable_tool_can_mutate() {
    let siblings = Siblings::expecting(2);
    let mut model = SharedModel::new(asks_for_two_delegates_writing_in_order(), &siblings);
    let mut mutating = spec("write", Approval::NotRequired);
    mutating.envelope = Envelope {
        effects: vec![Effect::Write],
        risk: Risk::Low,
        idempotency: Idempotency::Idempotent,
        access: Vec::new(),
    };
    let mut tools = SharedTools::new(vec![mutating]);
    let (outcome, sink) = run_ports(
        &mut model,
        &mut tools,
        &mut DenyAll,
        delegating_config(Delegation::default()),
    );
    let outcome = outcome.expect("effectful reach takes the sequential path");
    assert_ran_in_order(&outcome, &siblings, &sink);
    assert_eq!(
        tools.state().as_deref(),
        Some("right"),
        "the second child observes the same state and its model-order write is final"
    );
    let values: Vec<String> = tools
        .calls
        .lock()
        .expect("the call log")
        .iter()
        .filter_map(|call| call.arguments["value"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(values, ["left", "right"], "effects keep model order");
}

/// The same turn, with the two children served by the parent's own port because nothing forked.
///
/// A run of delegates in order holds one port for the whole of each child, so the script reads as
/// the three loops interleaved in the order the turns actually happen.
fn asks_for_two_delegates_run_in_order() -> Vec<Result<TurnOutcome, WireError>> {
    vec![
        Ok(asks_for(&[
            ("call-1", "delegate", json!({"task": "left"})),
            ("call-2", "delegate", json!({"task": "right"})),
        ])),
        Ok(answer("done: left")),
        Ok(answer("done: right")),
        Ok(answer("both reported")),
    ]
}

/// Two sequential children that each exercise the same approval-gated reachable operation.
fn asks_for_two_delegates_reading_in_order() -> Vec<Result<TurnOutcome, WireError>> {
    vec![
        Ok(asks_for(&[
            ("call-1", "delegate", json!({"task": "left"})),
            ("call-2", "delegate", json!({"task": "right"})),
        ])),
        Ok(asks_for(&[(
            "child-left-read",
            "read",
            json!({"path": "left"}),
        )])),
        Ok(answer("done: left")),
        Ok(asks_for(&[(
            "child-right-read",
            "read",
            json!({"path": "right"}),
        )])),
        Ok(answer("done: right")),
        Ok(answer("both reported")),
    ]
}

/// Two children that make conflicting writes to the tool port's one shared state.
fn asks_for_two_delegates_writing_in_order() -> Vec<Result<TurnOutcome, WireError>> {
    vec![
        Ok(asks_for(&[
            ("call-1", "delegate", json!({"task": "left"})),
            ("call-2", "delegate", json!({"task": "right"})),
        ])),
        Ok(asks_for(&[(
            "child-left-write",
            "write",
            json!({"value": "left"}),
        )])),
        Ok(answer("done: left")),
        Ok(asks_for(&[(
            "child-right-write",
            "write",
            json!({"value": "right"}),
        )])),
        Ok(answer("done: right")),
        Ok(answer("both reported")),
    ]
}

/// What a run in order must produce, whichever reason sent it down that path.
///
/// The event bracketing is asserted as well as the results, because a group that gave up **after**
/// announcing its children wrote a second `DelegateStarted` for each of them when the caller then
/// ran them in order — two starts and one finish per child, which is a record no reader can
/// interpret. A shipped `ToolPort` wrapper that inherited `fork`'s default instead of delegating
/// it is all it took to produce that.
fn assert_ran_in_order(outcome: &LoopOutcome, siblings: &Siblings, sink: &VecLoopSink) {
    assert_eq!(outcome.stop, LoopStop::Completed);
    assert_eq!(
        siblings.peak(),
        0,
        "nothing was forked, so no child ran on a handle of its own"
    );
    assert_eq!(result_of(outcome, "call-1").1["text"], json!("done: left"));
    assert_eq!(result_of(outcome, "call-2").1["text"], json!("done: right"));
    assert_eq!(
        bracketing(sink),
        vec![
            ("delegate-started", "call-1"),
            ("delegate-finished", "call-1"),
            ("delegate-started", "call-2"),
            ("delegate-finished", "call-2"),
        ],
        "each child announced once, and bracketed before the next begins"
    );
}

/// Every `DelegateStarted` and `DelegateFinished` of a run, in order, with the call each names.
fn bracketing(sink: &VecLoopSink) -> Vec<(&'static str, &str)> {
    sink.events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::DelegateStarted { call_id, .. } => {
                Some(("delegate-started", call_id.as_str()))
            }
            LoopEvent::DelegateFinished { call_id, .. } => {
                Some(("delegate-finished", call_id.as_str()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_model_port_that_will_not_fork_runs_the_same_delegates_in_order_and_refuses_nothing() {
    let siblings = Siblings::expecting(2);
    let mut model = SharedModel::new(asks_for_two_delegates_run_in_order(), &siblings).unforkable();
    let mut tools = SharedTools::new(Vec::new());
    let (outcome, sink) = run_ports(
        &mut model,
        &mut tools,
        &mut DenyAll,
        delegating_config(Delegation::default()),
    );
    assert_ran_in_order(
        &outcome.expect("a run whose port will not fork still completes"),
        &siblings,
        &sink,
    );
}

#[test]
fn a_tool_port_that_will_not_fork_runs_the_same_delegates_in_order_and_refuses_nothing() {
    let siblings = Siblings::expecting(2);
    let mut model = SharedModel::new(asks_for_two_delegates_run_in_order(), &siblings);
    let mut tools = SharedTools::new(Vec::new());
    tools.forkable = false;
    let (outcome, sink) = run_ports(
        &mut model,
        &mut tools,
        &mut DenyAll,
        delegating_config(Delegation::default()),
    );
    assert_ran_in_order(
        &outcome.expect("a run whose tool port will not fork still completes"),
        &siblings,
        &sink,
    );
}

#[test]
fn a_run_that_may_not_delegate_side_by_side_runs_its_two_delegates_in_order() {
    let siblings = Siblings::expecting(2);
    let mut model = SharedModel::new(asks_for_two_delegates_run_in_order(), &siblings);
    let mut tools = SharedTools::new(Vec::new());
    let (outcome, sink) = run_ports(
        &mut model,
        &mut tools,
        &mut DenyAll,
        delegating_config(Delegation::default().with_max_parallel(1)),
    );
    assert_ran_in_order(&outcome.expect("the run completes"), &siblings, &sink);
}

#[test]
fn a_call_between_two_delegates_is_a_barrier_and_they_do_not_group() {
    let siblings = Siblings::expecting(2);
    let mut model = SharedModel::new(
        vec![
            Ok(asks_for(&[
                ("call-1", "delegate", json!({"task": "left"})),
                ("call-2", "write", json!({"path": "a"})),
                ("call-3", "delegate", json!({"task": "right"})),
            ])),
            // Each child in turn, on the parent's own port, because neither was forked.
            Ok(answer("done: left")),
            Ok(answer("done: right")),
            Ok(answer("done")),
        ],
        &siblings,
    );
    let mut tools =
        SharedTools::new(vec![spec("write", Approval::NotRequired)]).enveloped(Envelope {
            effects: vec![Effect::Write],
            risk: Risk::Low,
            idempotency: Idempotency::NonIdempotent,
            access: Vec::new(),
        });
    let (outcome, _) = run_ports(
        &mut model,
        &mut tools,
        &mut DenyAll,
        delegating_config(Delegation::default()),
    );
    let outcome = outcome.expect("the run completes");
    assert_eq!(
        siblings.peak(),
        0,
        "neither delegate was forked: the second may be there to look at what the write did, so          it waits for it and the two never form a group"
    );
    assert_eq!(result_of(&outcome, "call-1").1["text"], json!("done: left"));
    assert_eq!(
        result_of(&outcome, "call-3").1["text"],
        json!("done: right")
    );
}

#[test]
fn a_budget_that_will_not_divide_runs_delegates_in_order_and_binds_on_the_first_child() {
    let siblings = Siblings::expecting(2);
    let mut model = SharedModel::new(asks_for_two_delegates_run_in_order(), &siblings);
    let mut tools = SharedTools::new(Vec::new());
    let mut config = delegating_config(Delegation::default());
    // Eleven, of which the parent's first turn spends ten: one token of headroom, which is enough
    // for a child whole and nothing at all halved. A group that will not divide is run in order,
    // where each child is carved on what the one before it actually spent.
    config.budget = Budget {
        max_input_tokens: Some(11),
        ..Budget::default()
    };
    let (outcome, _) = run_ports(&mut model, &mut tools, &mut DenyAll, config);
    assert_eq!(
        siblings.peak(),
        0,
        "run in order on the parent's own port, not forked and not refused as a group"
    );
    let outcome = outcome.expect("the run ends on its budget rather than failing");
    assert!(
        result_of(&outcome, "call-1").0,
        "the first child spent beyond the one-token remainder and reports its bound"
    );
    assert_eq!(
        outcome.stop,
        LoopStop::MaxInputTokens {
            limit: 11,
            reported: 20,
        }
    );
}

#[test]
fn a_child_that_panics_is_a_failed_result_and_its_siblings_finish() {
    let siblings = Siblings::expecting(2);
    let mut model = SharedModel::new(asks_for_two_delegates(), &siblings).panicking_on("left");
    let mut tools = SharedTools::new(Vec::new());
    // The panic is caught and reported; the hook the test harness installs would otherwise print
    // the backtrace of a panic this test is deliberately causing.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let (outcome, sink) = run_ports(
        &mut model,
        &mut tools,
        &mut DenyAll,
        delegating_config(Delegation::default()),
    );
    std::panic::set_hook(previous);

    let outcome = outcome.expect("a run one of whose children panicked still ends");
    assert_eq!(outcome.stop, LoopStop::Completed);
    let (failed, output) = result_of(&outcome, "call-1");
    assert!(failed, "the model has to learn the sub-task did not happen");
    assert!(
        output.as_str().unwrap_or_default().contains("panicked"),
        "and what happened to it: {output}"
    );
    assert_eq!(
        result_of(&outcome, "call-2").1["text"],
        json!("done: right"),
        "the sibling of a child that panicked reports as normal"
    );
    assert!(
        sink.events().iter().any(|event| matches!(
            event,
            LoopEvent::DelegateFinished { call_id, .. } if call_id.as_str() == "call-1"
        )),
        "the record carries an ending for it, or a reader sees a delegate that never came back"
    );
}

#[test]
fn delegates_run_in_order_when_a_reachable_tool_needs_approval() {
    let siblings = Siblings::expecting(2);
    let mut model = SharedModel::new(asks_for_two_delegates_reading_in_order(), &siblings);
    let mut tools =
        SharedTools::new(vec![spec("read", Approval::NotRequired)]).enveloped(Envelope {
            effects: vec![Effect::Read],
            // Above the run's `Risk::Low` ceiling, so a person is asked about it.
            risk: Risk::Medium,
            idempotency: Idempotency::Idempotent,
            access: Vec::new(),
        });
    let mut approvals = Recording::default();
    let (outcome, _) = run_ports(
        &mut model,
        &mut tools,
        &mut approvals,
        delegating_config(Delegation::default()),
    );
    let outcome = outcome.expect("the run completes");

    assert_eq!(
        siblings.peak(),
        0,
        "approval is an ordered observation, so neither child gets a forked port"
    );
    assert_eq!(
        approvals.asked.len(),
        2,
        "one decision per child, in model order through the run's one approver"
    );
    assert_eq!(result_of(&outcome, "call-1").1["text"], json!("done: left"));
}

/// An approver that records what it was asked, from whichever thread asked it.
#[derive(Default)]
struct Recording {
    asked: Vec<ToolName>,
}

impl ApprovalPort for Recording {
    fn decide(&mut self, _: &ToolCall, spec: &ToolSpec) -> ApprovalDecision {
        self.asked.push(spec.name.clone());
        ApprovalDecision::Approved
    }
}

#[test]
fn the_delegate_tool_tells_the_model_it_can_ask_for_several_only_when_the_run_can_run_several() {
    let together = Delegation::default().spec();
    assert!(
        together.description.contains("at the same time"),
        "a model that is not told cannot use it: {}",
        together.description
    );
    let in_order = Delegation::default().with_max_parallel(1).spec();
    assert!(
        !in_order.description.contains("at the same time"),
        "and a run that cannot must not claim it: {}",
        in_order.description
    );
}

#[test]
fn a_group_of_delegates_divides_the_runs_remaining_tokens_between_them() {
    let siblings = Siblings::expecting(2);
    let mut model = SharedModel::new(asks_for_two_delegates(), &siblings).children_reading_first();
    let mut tools = SharedTools::new(vec![spec("read", Approval::NotRequired)]);
    let mut config = delegating_config(Delegation::default());
    // The parent's first turn reports ten input tokens, leaving fourteen. Halved, each child is
    // started on seven — and each child's own first turn reports ten, so each stops on a ceiling
    // that names the figure it was given.
    config.budget = Budget {
        max_input_tokens: Some(24),
        ..Budget::default()
    };
    let (outcome, _) = run_ports(&mut model, &mut tools, &mut DenyAll, config);
    let outcome = outcome.expect("a run that ends on its budget is an outcome, not a failure");

    assert_eq!(siblings.peak(), 2, "they ran side by side");
    for call in ["call-1", "call-2"] {
        assert_eq!(
            result_of(&outcome, call).1["stop"],
            json!({"kind": "max-input-tokens", "limit": 7, "reported": 10}),
            "half the remainder each: tokens add up, so a group cannot promise the whole of it \
             twice"
        );
    }
}
