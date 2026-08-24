use std::collections::{BTreeMap, VecDeque};

use harness_wire::{
    CallId, Envelope, Item, ModelPort, StopReason, StreamEvent, StreamSink, ToolCall, ToolName,
    ToolOutcome, ToolPort, ToolSpec, TurnOutcome, TurnRequest, Usage, WireError, WireErrorCode,
    WireId,
};
use serde_json::{Value, json};

use super::*;

const WIRE: &str = "scripted";

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
    }
}

fn answer(text: &str) -> TurnOutcome {
    TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::assistant(text)],
        usage: Some(usage(10, 5)),
    }
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
}

impl ScriptedTools {
    fn new(specs: Vec<ToolSpec>) -> Self {
        Self {
            specs,
            outcomes: BTreeMap::new(),
            calls: Vec::new(),
            cancel_after: None,
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
}

impl ToolPort for ScriptedTools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        self.calls.push(call.clone());
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
        max_input_tokens: Some(5),
        ..Budget::default()
    });
    let (outcome, _) = harness.run();
    assert_eq!(
        outcome.expect("bound binds").stop,
        LoopStop::MaxInputTokens {
            limit: 5,
            reported: 10
        }
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
        max_output_tokens: Some(2),
        ..Budget::default()
    });
    let (outcome, _) = harness.run();
    assert_eq!(
        outcome.expect("bound binds").stop,
        LoopStop::MaxOutputTokens {
            limit: 2,
            reported: 5
        }
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
            .any(|event| matches!(event, LoopEvent::Finished { stop } if matches!(stop, LoopStop::Cancelled { .. }))),
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
