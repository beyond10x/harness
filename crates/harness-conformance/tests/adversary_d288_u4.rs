//! Adversary cases for `story:harness-loop-modeled-in-ess` (wave d288, unit 4, pass 1).
//!
//! Each case names, in its doc comment, a mutant of `crates/harness-loop/src/lib.rs` that leaves
//! the checked-in ESS suite (`conformance/suite.json`, 165 scenarios) green and that the case
//! fails on. One case is red on the real code: it holds the declared `harness.loop.Loop` lifecycle
//! against the steps a real run takes.
//!
//! Two kinds of drive, both against the real `AgentLoop`:
//!
//! * through the production adapter (`src/boundary.rs`, compiled in unchanged), reading responses
//!   the authored scenarios never read: the deferred checkpoint's contents, the failed stop after
//!   a compaction, a second deferral after a resumed run;
//! * with ports scripted here, for what the adapter cannot express: a turn with two calls, a
//!   conversation of a measured size against the declared window, a priced run, a deadline.

#[path = "../src/boundary.rs"]
#[allow(dead_code)]
mod boundary;

use std::{cell::RefCell, collections::VecDeque, fs, path::PathBuf, rc::Rc, time::Duration};

use harness_loop::{
    AgentLoop, ApprovalCheckpoint, ApprovalDecision, ApprovalPort, LoopConfig, LoopError,
    LoopOutcome, LoopStop, RateCard, RunLedger, VecLoopSink,
};
use harness_wire::{
    Approval, CallId, Envelope, Item, ModelPort, Risk, StopReason, StreamSink, ToolCall, ToolName,
    ToolOutcome, ToolPort, ToolSpec, TurnOutcome, TurnRequest, Usage, WireError, WireId,
};
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// ---------------------------------------------------------------------------------------------
// Through the production adapter.
// ---------------------------------------------------------------------------------------------

/// One scenario's run under its own correlation, answered through `boundary::Loops`.
struct Scenario {
    loops: boundary::Loops,
    id: String,
}

impl Scenario {
    fn new(id: &str) -> Self {
        let loops = boundary::Loops::default();
        loops.begin(id);
        Self {
            loops,
            id: id.to_owned(),
        }
    }

    /// Answers `command` and returns the outcome the adapter named and the response it returned.
    fn step(&self, command: &str, input: &Value) -> (String, Value) {
        let answer = self
            .loops
            .execute(&self.id, command, input)
            .unwrap_or_else(|error| panic!("{command}: {error}"));
        assert!(
            answer.error.is_none(),
            "{command} was refused: {:?}",
            answer.error
        );
        (answer.outcome, answer.response.unwrap_or(Value::Null))
    }
}

impl Drop for Scenario {
    fn drop(&mut self) {
        self.loops.end(&self.id);
    }
}

fn run_a() -> Value {
    json!({"run_id": "run-a"})
}

fn call_tool(call_id: &str, tool: &str, text: &str) -> Value {
    json!({"run_id": "run-a", "call_id": call_id, "tool": tool, "text": text})
}

/// The failed stop a non-retriable `FailTurn` ends the run with, `after_compaction` as given.
fn refused_stop(after_compaction: bool) -> Value {
    json!({
        "kind": "failed",
        "value": {
            "cause": "provider-protocol",
            "message": "model wire refused: Protocol: the provider refused the request",
            "after_compaction": after_compaction,
        }
    })
}

/// A request that fails right after a summary turn folded the conversation is named as one.
///
/// Mutant (the ESS suite stays 165/165): `lib.rs:2747`
/// `state.failed_after_compaction = compacted;` → `state.failed_after_compaction = false;`.
/// Every authored `failed` stop has `after_compaction: false`, and no scenario fails a turn after
/// a compaction, so the field `FailedStop.after_compaction` is declared and never observed true.
#[test]
fn a_turn_that_fails_after_a_folded_summary_says_it_followed_a_compaction() {
    let s = Scenario::new("adv-d288-u4-fail-after-fold");
    assert_eq!(s.step("harness.loop.StartLoop", &run_a()).0, "requested");
    let (outcome, _) = s.step(
        "harness.loop.CallTool",
        &call_tool("call-1", "read", "long"),
    );
    assert_eq!(outcome, "called-long");
    let (outcome, _) = s.step("harness.loop.CompleteTool", &run_a());
    assert_eq!(outcome, "returned-then-summarise");
    assert_eq!(s.step("harness.loop.AnswerSummary", &run_a()).0, "folded");
    let (outcome, response) = s.step(
        "harness.loop.FailTurn",
        &json!({"run_id": "run-a", "retriable": false}),
    );
    assert_eq!(outcome, "failed");
    assert_eq!(response["stop"], refused_stop(true));
}

/// A summary turn that failed on the wire still counts as a compaction for the next failure.
///
/// `compact_run` says so in its own doc comment ("a summary turn that failed with nothing elided
/// is reported too", `lib.rs:3101-3103`, `Compaction::Reported` at `lib.rs:3147,3151`).
///
/// Mutants (the ESS suite stays 165/165): `lib.rs:3147` `Summarised::Failed => summary_turn = true`
/// → `Summarised::Failed => {}`, and the `lib.rs:2747` mutant above. The authored
/// `loop-compaction-summary-failed` names `kept` from the `summary-failed` warning, which the first
/// mutant still emits.
#[test]
fn a_turn_that_fails_after_a_failed_summary_says_it_followed_a_compaction() {
    let s = Scenario::new("adv-d288-u4-fail-after-failed-summary");
    assert_eq!(s.step("harness.loop.StartLoop", &run_a()).0, "requested");
    let (outcome, _) = s.step(
        "harness.loop.CallTool",
        &call_tool("call-1", "read", "long"),
    );
    assert_eq!(outcome, "called-long");
    let (outcome, _) = s.step("harness.loop.CompleteTool", &run_a());
    assert_eq!(outcome, "returned-then-summarise");
    assert_eq!(s.step("harness.loop.FailSummary", &run_a()).0, "kept");
    let (outcome, response) = s.step(
        "harness.loop.FailTurn",
        &json!({"run_id": "run-a", "retriable": false}),
    );
    assert_eq!(outcome, "failed");
    assert_eq!(response["stop"], refused_stop(true));
}

/// The checkpoint a deferral hands back is bound to the id the stop names and to the exact call.
///
/// `spec/COVERAGE.md` says "the declared schema (one-variant `format`, every field typed) holds
/// the checkpoint": it holds the types and no value. The authored `Defer` steps pin `stop` and
/// `ledger` only.
///
/// Mutant (the ESS suite stays 165/165): `lib.rs:2456`
/// `checkpoint_id: deferred.checkpoint_id,` →
/// `checkpoint_id: format!("{}-taken", deferred.checkpoint_id),`. The stop still says `cp-1`
/// (`lib.rs:3463` reads the deferral, not the checkpoint) and `admit_resumption` only refuses an
/// empty id, so an embedder that files the checkpoint under the stop's id resumes the wrong one.
#[test]
fn the_deferred_checkpoint_holds_the_id_the_stop_names_and_the_exact_call() {
    let s = Scenario::new("adv-d288-u4-checkpoint-contents");
    assert_eq!(s.step("harness.loop.StartLoop", &run_a()).0, "requested");
    let (outcome, _) = s.step(
        "harness.loop.CallTool",
        &call_tool("call-7", "write", "short"),
    );
    assert_eq!(outcome, "asked");
    let (outcome, response) = s.step(
        "harness.loop.Defer",
        &json!({"run_id": "run-a", "checkpoint_id": "cp-1"}),
    );
    assert_eq!(outcome, "deferred");
    assert_eq!(
        response["stop"],
        json!({"kind": "awaiting-approval", "value": {"checkpoint_id": "cp-1"}})
    );
    let checkpoint = &response["checkpoint"];
    assert_eq!(checkpoint["format"], json!("harness.approval-checkpoint/1"));
    assert_eq!(
        checkpoint["checkpoint_id"],
        json!("cp-1"),
        "the checkpoint is filed under another id than the stop names"
    );
    assert_eq!(
        checkpoint["call"],
        json!({"call_id": "call-7", "name": "write", "arguments": {}})
    );
    assert_eq!(checkpoint["invoked"]["name"], json!("write"));
    assert_eq!(checkpoint["remaining_calls"], json!([]));
    assert_eq!(checkpoint["state"]["turns"], json!(1));
    assert_eq!(checkpoint["remaining_duration_ms"], Value::Null);
}

/// A resumed run that defers again hands back a second checkpoint, and that one resumes too.
///
/// Every step is a declared transition (`resume`, `return`, `ask`, `defer`), and `Defer` declares
/// a resumed run's answer (`ledger` absent for a resumed run); no scenario takes it.
///
/// Mutant (the ESS suite stays 165/165): `lib.rs:2398`
/// `let checkpoint = self.take_checkpoint(&state);` → `let checkpoint = None;`. A resumed run
/// that defers then returns `awaiting-approval` with nothing to resume from.
#[test]
fn a_resumed_run_that_defers_again_hands_back_a_checkpoint_that_resumes() {
    let s = Scenario::new("adv-d288-u4-defer-twice");
    assert_eq!(s.step("harness.loop.StartLoop", &run_a()).0, "requested");
    let (outcome, _) = s.step(
        "harness.loop.CallTool",
        &call_tool("call-7", "write", "short"),
    );
    assert_eq!(outcome, "asked");
    let (outcome, _) = s.step(
        "harness.loop.Defer",
        &json!({"run_id": "run-a", "checkpoint_id": "cp-1"}),
    );
    assert_eq!(outcome, "deferred");
    let (outcome, response) = s.step("harness.loop.ResumeApproved", &run_a());
    assert_eq!(
        (outcome.as_str(), &response),
        ("resumed", &json!({"turn": 1}))
    );
    let (outcome, response) = s.step("harness.loop.CompleteTool", &run_a());
    assert_eq!(
        (outcome.as_str(), &response),
        ("returned", &json!({"turn": 2}))
    );
    let (outcome, _) = s.step(
        "harness.loop.CallTool",
        &call_tool("call-8", "write", "short"),
    );
    assert_eq!(outcome, "asked");
    let (outcome, response) = s.step(
        "harness.loop.Defer",
        &json!({"run_id": "run-a", "checkpoint_id": "cp-2"}),
    );
    assert_eq!(outcome, "deferred");
    assert_eq!(
        response["stop"],
        json!({"kind": "awaiting-approval", "value": {"checkpoint_id": "cp-2"}})
    );
    assert_eq!(response["ledger"], Value::Null);
    assert_eq!(response["checkpoint"]["checkpoint_id"], json!("cp-2"));
    assert_eq!(response["checkpoint"]["call"]["call_id"], json!("call-8"));
    assert_eq!(response["checkpoint"]["state"]["turns"], json!(2));
    let (outcome, response) = s.step("harness.loop.ResumeApproved", &run_a());
    assert_eq!(
        (outcome.as_str(), &response),
        ("resumed", &json!({"turn": 2}))
    );
    let (outcome, response) = s.step("harness.loop.CompleteTool", &run_a());
    assert_eq!(
        (outcome.as_str(), &response),
        ("returned", &json!({"turn": 3}))
    );
    let (outcome, response) = s.step(
        "harness.loop.AnswerTurn",
        &json!({"run_id": "run-a", "text": "Done."}),
    );
    assert_eq!(outcome, "completed");
    assert_eq!(
        response,
        json!({"stop": {"kind": "completed"}, "ledger": null})
    );
}

// ---------------------------------------------------------------------------------------------
// The real loop with ports scripted here.
// ---------------------------------------------------------------------------------------------

/// The port a run asked, as the Loop entity's states name the wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Port {
    Model,
    Summary,
    Approval,
    Tool,
}

impl Port {
    fn state(self) -> &'static str {
        match self {
            Self::Model => "AwaitingModel",
            Self::Summary => "AwaitingSummary",
            Self::Approval => "AwaitingApproval",
            Self::Tool => "AwaitingTool",
        }
    }
}

/// What the scripted ports were asked, in order.
#[derive(Default)]
struct Record {
    /// Each port call, with the `harness.loop` command its answer is in the specification.
    steps: Vec<(Port, &'static str)>,
    /// Each call the tool port ran, as the call serializes.
    ran: Vec<Value>,
    /// The conversation each conversation request (not a summary request) carried.
    requests: Vec<Vec<Item>>,
}

type Shared = Rc<RefCell<Record>>;

struct Model {
    wire: WireId,
    script: VecDeque<TurnOutcome>,
    record: Shared,
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
        let mut record = self.record.borrow_mut();
        // A summary turn is the one request that publishes no tools (lib.rs:3224).
        if request.tools.is_empty() {
            record
                .steps
                .push((Port::Summary, "harness.loop.AnswerSummary"));
            return Ok(answer(
                "Summary of the earlier conversation.",
                Vec::new(),
                1,
            ));
        }
        record.requests.push(request.items.clone());
        let outcome = self
            .script
            .pop_front()
            .ok_or_else(|| WireError::protocol("the script is exhausted"))?;
        let command = if outcome
            .items
            .iter()
            .any(|item| matches!(item, Item::ToolCall(_)))
        {
            "harness.loop.CallTool"
        } else {
            "harness.loop.AnswerTurn"
        };
        record.steps.push((Port::Model, command));
        Ok(outcome)
    }
}

struct Approver {
    script: VecDeque<ApprovalDecision>,
    record: Shared,
}

impl ApprovalPort for Approver {
    fn decide(&mut self, _: &ToolCall, _: &ToolSpec) -> ApprovalDecision {
        let decision = self
            .script
            .pop_front()
            .unwrap_or_else(|| ApprovalDecision::denied("the script is exhausted"));
        let command = match &decision {
            ApprovalDecision::Approved => "harness.loop.Approve",
            ApprovalDecision::Denied { .. } => "harness.loop.Deny",
            ApprovalDecision::Deferred { .. } => "harness.loop.Defer",
        };
        self.record
            .borrow_mut()
            .steps
            .push((Port::Approval, command));
        decision
    }
}

struct Tools {
    specs: Vec<ToolSpec>,
    record: Shared,
}

impl ToolPort for Tools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        let mut record = self.record.borrow_mut();
        record.steps.push((Port::Tool, "harness.loop.CompleteTool"));
        record
            .ran
            .push(serde_json::to_value(call).expect("a call serializes"));
        ToolOutcome::ok(json!("effect"))
    }
}

/// The adapter's two tools: `read` runs unattended, `write` is over the default ceiling.
fn specs() -> Vec<ToolSpec> {
    [("read", Risk::Low), ("write", Risk::Medium)]
        .into_iter()
        .map(|(name, risk)| ToolSpec {
            name: ToolName::new(name).expect("a tool name"),
            description: name.into(),
            input_schema: json!({"type": "object"}),
            approval: Approval::NotRequired,
            envelope: Envelope {
                risk,
                ..Envelope::default()
            },
        })
        .collect()
}

/// The configuration the adapter's stepwise runs use (`boundary.rs`, `config`).
fn config() -> LoopConfig {
    let mut config = LoopConfig::new("fixture", "Deterministic native conformance")
        .with_retry_backoff(Duration::from_millis(1));
    config.context_window = Some(4096);
    config
}

fn call(call_id: &str, tool: &str) -> ToolCall {
    ToolCall {
        call_id: CallId::new(call_id).expect("a call id"),
        name: ToolName::new(tool).expect("a tool name"),
        arguments: json!({}),
    }
}

/// A turn answering `text` and asking `calls`, reporting `input` input tokens and ten output.
fn answer(text: &str, calls: Vec<ToolCall>, input: u64) -> TurnOutcome {
    let stop_reason = if calls.is_empty() {
        StopReason::EndTurn
    } else {
        StopReason::ToolCalls
    };
    let mut items = vec![Item::AssistantText {
        text: text.to_owned(),
    }];
    items.extend(calls.into_iter().map(Item::ToolCall));
    TurnOutcome {
        stop_reason,
        items,
        usage: Some(Usage {
            model: "fixture".to_owned(),
            input_tokens: input,
            output_tokens: 10,
            cached_input_tokens: 0,
            cache_creation_input_tokens: None,
        }),
    }
}

struct Fresh {
    result: std::result::Result<LoopOutcome, LoopError>,
    ledger: RunLedger,
}

/// One fresh `run_in` over scripted ports.
fn fresh(
    config: LoopConfig,
    script: Vec<TurnOutcome>,
    decisions: Vec<ApprovalDecision>,
    record: &Shared,
) -> Fresh {
    let mut model = Model {
        wire: WireId::new("fixture").expect("a wire id"),
        script: script.into(),
        record: Rc::clone(record),
    };
    let mut tools = Tools {
        specs: specs(),
        record: Rc::clone(record),
    };
    let mut approver = Approver {
        script: decisions.into(),
        record: Rc::clone(record),
    };
    let mut agent = AgentLoop::new(&mut model, &mut tools, &mut approver, config);
    let mut items = Vec::new();
    let mut ledger = RunLedger::default();
    let result = agent.run_in(
        &mut items,
        &mut ledger,
        "synthetic input",
        &mut VecLoopSink::new(),
    );
    Fresh { result, ledger }
}

/// A turn that asks two calls and is deferred at the first keeps the second for the resumption,
/// and the resumed run runs both, in order.
///
/// The checkpoint field `remaining_calls` is declared and typed (`spec/domains/loop.yaml`,
/// "filled at 3461") and is `[]` in every run the suite makes, because the adapter's `CallTool`
/// asks one call per turn.
///
/// Mutant (the ESS suite stays 165/165): `lib.rs:2460`
/// `remaining_calls: deferred.remaining_calls,` → `remaining_calls: Vec::new(),`.
#[test]
fn a_deferred_call_keeps_the_calls_its_turn_asked_after_it() {
    let record = Shared::default();
    let write = call("call-w", "write");
    let read = call("call-r", "read");
    let first = fresh(
        config(),
        vec![answer("Two calls.", vec![write.clone(), read.clone()], 100)],
        vec![ApprovalDecision::Deferred {
            checkpoint_id: "cp-1".to_owned(),
        }],
        &record,
    );
    let outcome = first.result.expect("a deferral is not an error");
    assert_eq!(
        outcome.stop,
        LoopStop::AwaitingApproval {
            checkpoint_id: "cp-1".to_owned()
        }
    );
    assert!(
        record.borrow().ran.is_empty(),
        "a call ran before its turn was decided"
    );
    let persisted = serde_json::to_string(
        outcome
            .checkpoint
            .as_ref()
            .expect("a deferred run hands back a checkpoint"),
    )
    .expect("a checkpoint serializes");
    let value: Value = serde_json::from_str(&persisted).expect("a checkpoint is JSON");
    let write_value = serde_json::to_value(&write).expect("a call serializes");
    let read_value = serde_json::to_value(&read).expect("a call serializes");
    assert_eq!(value["call"], write_value);
    assert_eq!(
        value["remaining_calls"],
        json!([read_value.clone()]),
        "the checkpoint dropped the call its turn asked after the deferred one"
    );

    let resumed = Shared::default();
    let checkpoint: ApprovalCheckpoint =
        serde_json::from_str(&persisted).expect("the persisted checkpoint loads");
    let mut model = Model {
        wire: WireId::new("fixture").expect("a wire id"),
        script: VecDeque::from([answer("Done.", Vec::new(), 100)]),
        record: Rc::clone(&resumed),
    };
    let mut tools = Tools {
        specs: specs(),
        record: Rc::clone(&resumed),
    };
    let mut approver = Approver {
        script: VecDeque::new(),
        record: Rc::clone(&resumed),
    };
    let result = AgentLoop::new(&mut model, &mut tools, &mut approver, config()).resume_approval(
        checkpoint,
        ApprovalDecision::Approved,
        &mut VecLoopSink::new(),
    );
    let outcome = result.expect("the resumed run completes");
    assert_eq!(outcome.stop, LoopStop::Completed);
    let resumed = resumed.borrow();
    assert_eq!(
        resumed.ran,
        vec![write_value, read_value],
        "the resumed run did not run the deferred call and then the call after it"
    );
    let answered: Vec<&CallId> = resumed
        .requests
        .last()
        .expect("the resumed run asked the model again")
        .iter()
        .filter_map(|item| match item {
            Item::ToolResult {
                call_id,
                failed: false,
                ..
            } => Some(call_id),
            _ => None,
        })
        .collect();
    assert_eq!(answered, vec![&write.call_id, &read.call_id]);
}

/// The results `items` holds for `call_id`, failed ones included.
fn results_for<'a>(items: &'a [Item], call_id: &CallId) -> Vec<&'a Item> {
    items
        .iter()
        .filter(|item| matches!(item, Item::ToolResult { call_id: id, .. } if id == call_id))
        .collect()
}

/// A deferred call has no result in the checkpoint's conversation, and the resumed run answers it
/// exactly once.
///
/// A deferral is not an outcome: `run_calls` returns before `complete` (`lib.rs:3460-3466`), so
/// the checkpoint's `state.items` hold the call and no result. The suite never reads `state.items`
/// (declared `List<Json>`), and the adapter's model port accepts a conversation that answers a
/// call twice.
///
/// Mutant (the ESS suite stays 165/165): `lib.rs:3466` `complete(call, result, state, sink);` moved
/// above `if let Some(deferred) = self.deferred.as_mut() {` at `lib.rs:3460`. The "suspended"
/// failure enters the checkpoint, and the resumed run replays two results for one call.
#[test]
fn a_deferred_call_is_answered_once_in_the_resumed_conversation() {
    let record = Shared::default();
    let write = call("call-w", "write");
    let first = fresh(
        config(),
        vec![answer("One write.", vec![write.clone()], 100)],
        vec![ApprovalDecision::Deferred {
            checkpoint_id: "cp-1".to_owned(),
        }],
        &record,
    );
    let checkpoint = first
        .result
        .expect("a deferral is not an error")
        .checkpoint
        .expect("a deferred run hands back a checkpoint");
    let persisted = serde_json::to_string(&checkpoint).expect("a checkpoint serializes");
    let value: Value = serde_json::from_str(&persisted).expect("a checkpoint is JSON");
    let held: Vec<Item> =
        serde_json::from_value(value["state"]["items"].clone()).expect("the conversation loads");
    assert_eq!(
        results_for(&held, &write.call_id),
        Vec::<&Item>::new(),
        "the checkpoint's conversation already answers the call that awaits a decision"
    );

    let resumed = Shared::default();
    let mut model = Model {
        wire: WireId::new("fixture").expect("a wire id"),
        script: VecDeque::from([answer("Done.", Vec::new(), 100)]),
        record: Rc::clone(&resumed),
    };
    let mut tools = Tools {
        specs: specs(),
        record: Rc::clone(&resumed),
    };
    let mut approver = Approver {
        script: VecDeque::new(),
        record: Rc::clone(&resumed),
    };
    let checkpoint: ApprovalCheckpoint =
        serde_json::from_str(&persisted).expect("the persisted checkpoint loads");
    let outcome = AgentLoop::new(&mut model, &mut tools, &mut approver, config())
        .resume_approval(
            checkpoint,
            ApprovalDecision::Approved,
            &mut VecLoopSink::new(),
        )
        .expect("the resumed run completes");
    assert_eq!(outcome.stop, LoopStop::Completed);
    let resumed = resumed.borrow();
    let next = resumed
        .requests
        .last()
        .expect("the resumed run asked the model again");
    let failed: Vec<bool> = results_for(next, &write.call_id)
        .into_iter()
        .filter_map(|item| match item {
            Item::ToolResult { failed, .. } => Some(*failed),
            _ => None,
        })
        .collect();
    assert_eq!(
        failed,
        vec![false],
        "the resumed conversation does not answer the approved call exactly once, successfully"
    );
}

/// The lifecycle `spec/domains/loop.yaml` declares, read back: whether answering `command` may
/// move a loop waiting in `from` to `to`.
fn declared(spec: &Value, command: &str, from: &str, to: &str) -> bool {
    let Some(entity) = spec["entities"]
        .as_array()
        .and_then(|all| all.iter().find(|e| e["name"] == "harness.loop.Loop"))
    else {
        return false;
    };
    let transitions = entity["lifecycle"]["transitions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let Some(declared) = spec["commands"]
        .as_array()
        .and_then(|all| all.iter().find(|c| c["name"] == command))
    else {
        return false;
    };
    declared["outcomes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|outcome| outcome["moves"].as_str())
        .filter_map(|moves| moves.strip_prefix("harness.loop.Loop."))
        .any(|name| {
            transitions.iter().any(|t| {
                t["name"] == name
                    && t["to"] == to
                    && t["from"]
                        .as_array()
                        .is_some_and(|states| states.iter().any(|state| state == from))
            })
        })
}

/// Every step a real run takes through turns that ask two calls is a transition the
/// specification declares for the command that answered it.
///
/// RED on the real code. `spec/COVERAGE.md` calls the lifecycle "seven states, 17 transitions"
/// and the story's acceptance asks for the loop states "with their legal transitions"; the
/// declared ones all assume one call per turn, because the adapter's `CallTool` asks one. A turn
/// that asks two goes from a returned tool to the approver or to the tool port again, and a denial
/// with a call after it goes to the tool port (`run_calls`, `lib.rs:3378-3468`, loops over the
/// turn's calls). Neither is declared, and neither is listed as UNMAPPED.
#[test]
fn every_step_of_turns_with_two_calls_is_a_declared_transition() {
    let spec: Value = serde_yaml_ng::from_str(
        &fs::read_to_string(root().join("spec/domains/loop.yaml")).expect("the domain reads"),
    )
    .expect("the domain parses");
    // The reader is not vacuous: it admits a declared step and refuses an undeclared one.
    assert!(declared(
        &spec,
        "harness.loop.CallTool",
        "AwaitingModel",
        "AwaitingApproval"
    ));
    assert!(!declared(
        &spec,
        "harness.loop.CallTool",
        "AwaitingModel",
        "Suspended"
    ));

    let record = Shared::default();
    let ran = fresh(
        config(),
        vec![
            answer(
                "Two writes.",
                vec![call("call-1", "write"), call("call-2", "write")],
                100,
            ),
            answer(
                "A write, then a read.",
                vec![call("call-3", "write"), call("call-4", "read")],
                100,
            ),
            answer(
                "A write, then a read.",
                vec![call("call-5", "write"), call("call-6", "read")],
                100,
            ),
            answer("Done.", Vec::new(), 100),
        ],
        vec![
            ApprovalDecision::Approved,
            ApprovalDecision::Approved,
            ApprovalDecision::denied("not now"),
            ApprovalDecision::Approved,
        ],
        &record,
    );
    let outcome = ran.result.expect("the run completes");
    assert_eq!(outcome.stop, LoopStop::Completed);

    let steps = record.borrow().steps.clone();
    let mut undeclared = Vec::new();
    for (at, (port, command)) in steps.iter().enumerate() {
        let from = port.state();
        let to = steps
            .get(at + 1)
            .map_or("Stopped", |(next, _)| next.state());
        if !declared(&spec, command, from, to) {
            undeclared.push(format!("{command}: {from} -> {to}"));
        }
    }
    assert_eq!(
        undeclared,
        Vec::<String>::new(),
        "steps the real loop took that spec/domains/loop.yaml does not declare"
    );
}

/// The conversation's size the way compaction measures it (`measure`, `lib.rs:1349-1355`).
fn measured(items: &[Item]) -> usize {
    items
        .iter()
        .map(|item| serde_json::to_string(item).map_or(0, |json| json.len()))
        .sum()
}

/// A turn whose assistant text is `bytes` long and asks one read, then a closing turn. Returns
/// the summary turns the run asked for and the conversation the closing request carried.
///
/// Every turn reports one input token, so the trigger reads the byte estimate, not a provider's
/// count (the latter is UNMAPPED in `spec/COVERAGE.md`).
fn summaries_after(bytes: usize) -> (usize, Vec<Item>) {
    let record = Shared::default();
    let ran = fresh(
        config(),
        vec![
            answer(&"x".repeat(bytes), vec![call("call-1", "read")], 1),
            answer("Done.", Vec::new(), 1),
        ],
        Vec::new(),
        &record,
    );
    assert_eq!(
        ran.result.expect("the run completes").stop,
        LoopStop::Completed
    );
    let record = record.borrow();
    let summaries = record
        .steps
        .iter()
        .filter(|(port, _)| *port == Port::Summary)
        .count();
    let closing = record
        .requests
        .last()
        .cloned()
        .expect("the run asked the model");
    (summaries, closing)
}

/// The summary turn starts at 80 % of the declared window and not a token before.
///
/// `spec/COVERAGE.md` says `unfolded` is "the conversation passed the compaction trigger (80% of
/// a 4096-token window, `lib.rs:1105,3122`)". The suite's long text is 16 KiB, which is 100 % of
/// that window, and its short text is under 1 %, so any trigger from 1 % to 100 % passes it.
///
/// Mutants (the ESS suite stays 165/165): `lib.rs:1105` `COMPACTION_TRIGGER_PERCENT: u64 = 80` →
/// `= 100` (and → `= 79`); `lib.rs:3122` `if occupied < window...` → `if occupied <= window...`.
#[test]
fn the_compaction_trigger_is_eighty_percent_of_the_declared_window() {
    // Under the trigger, so the closing request carries the conversation exactly as compaction
    // measured it before the second turn.
    let probe = 1000;
    let (summaries, closing) = summaries_after(probe);
    assert_eq!(summaries, 0, "a 1000-byte conversation was summarised");
    let overhead = measured(&closing) - probe;
    // 4096 * 80 / 100 = 3276 tokens, at four bytes a token: 13104 bytes is the first that triggers.
    let first = 13_104 - overhead;
    assert_eq!(
        summaries_after(first - 1).0,
        0,
        "3275 estimated tokens, under 80 % of 4096, started a summary turn"
    );
    assert_eq!(
        summaries_after(first).0,
        1,
        "3276 estimated tokens, 80 % of 4096, did not start a summary turn"
    );
}

/// A priced run's ledger carries what it cost.
///
/// `RunLedger.cost_micro_usd` is declared, and every ledger the suite compares has
/// `cost_micro_usd: null`: no Loop or Run scenario prices its model. (`spec/COVERAGE.md` lists cost
/// accumulation as UNMAPPED; this is the one figure the ledger exists to carry to ADR 0101's
/// attribution.)
///
/// Mutant (the ESS suite stays 165/165): `lib.rs:1030` `cost_micro_usd: self.cost_total,` →
/// `cost_micro_usd: None,`.
#[test]
fn the_ledger_of_a_priced_run_carries_its_cost() {
    let mut config = config();
    config.prices = Some(
        RateCard::parse(
            r#"{"source": "adversary fixture", "as_of": "2026-10-30", "models": {"fixture": {"input_usd_per_mtok": 1.0, "cached_input_usd_per_mtok": 0.0, "output_usd_per_mtok": 2.0}}}"#,
        )
        .expect("the card parses"),
    );
    let record = Shared::default();
    let ran = fresh(
        config,
        vec![
            answer("A read.", vec![call("call-1", "read")], 100),
            answer("Done.", Vec::new(), 100),
        ],
        Vec::new(),
        &record,
    );
    // Per turn: 100 input tokens at 1 USD and 10 output at 2 USD per million, 120 micro-USD.
    assert_eq!(
        ran.result.expect("the run completes").cost_micro_usd,
        Some(240)
    );
    assert_eq!(ran.ledger.cost_micro_usd, Some(240));
    assert_eq!(ran.ledger.turns, 2);
}

/// A checkpoint taken under a deadline carries the time left, so the resumed run keeps its bound.
///
/// `ApprovalCheckpoint.remaining_duration_ms` is declared, and the suite's runs have no deadline,
/// so it is `null` in every checkpoint the suite takes. (`spec/COVERAGE.md` lists elapsed deadlines
/// as UNMAPPED.)
///
/// Mutant (the ESS suite stays 165/165): `lib.rs:2462`
/// `remaining_duration_ms: deferred.remaining_duration_ms,` → `remaining_duration_ms: None,`.
#[test]
fn a_checkpoint_taken_under_a_deadline_carries_the_time_left() {
    let mut config = config();
    config.budget.max_duration_ms = Some(600_000);
    let record = Shared::default();
    let ran = fresh(
        config,
        vec![answer("A write.", vec![call("call-1", "write")], 100)],
        vec![ApprovalDecision::Deferred {
            checkpoint_id: "cp-1".to_owned(),
        }],
        &record,
    );
    let outcome = ran.result.expect("a deferral is not an error");
    let checkpoint = serde_json::to_value(
        outcome
            .checkpoint
            .expect("a deferred run hands back a checkpoint"),
    )
    .expect("a checkpoint serializes");
    let left = checkpoint["remaining_duration_ms"]
        .as_u64()
        .expect("a checkpoint taken under a deadline says how much of it was left");
    assert!((1..=600_000).contains(&left), "{left} ms left of 600000");
}
