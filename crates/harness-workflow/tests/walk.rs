//! A library caller walks the projected ADP document, with no harness process anywhere.
//!
//! The document is the fixture `harness-flow` pins, read unedited. The model is a fake that answers
//! every step through `answer` and decides by the **section the turn names** — the sentence
//! *"of section `…`"* the walk writes into every step's turn — so what it answers depends on where
//! the walk is standing and never on how many calls came before.

use harness_catalogue::{Catalogue, Flat, LocalOperations};
use harness_flow::{Flow, FlowEvent, FlowStatus, Moment, VecFlowSink};
use harness_loop::{
    DEFAULT_ANSWER_NAME, DenyAll, HookDecision, HookPoint, LoopConfig, LoopEvent, LoopStop,
    VecLoopSink,
};
use harness_wire::{
    CallId, Item, ModelPort, StopReason, StreamSink, ToolCall, ToolName, TurnOutcome, TurnRequest,
    WireError, WireId,
};
use harness_workflow::{Governor, Transition, Walk};
use serde_json::{Value, json};

const PROJECTED: &str = include_str!("../../harness-flow/fixtures/adp-default.projected.yaml");

const RETREAT: &str = "root.implement-to-review";
const REVIEW: &str = "root.implement-to-review.review";

/// What a section answers the `n`-th time it is asked, counting from 0.
type Decide = fn(section: &str, asked_before: usize) -> &'static str;

/// A model that answers every step by calling `answer`, keyed on the section its turn names.
struct BySection {
    wire: WireId,
    decide: Decide,
    /// Every section asked, in order.
    asked: Vec<String>,
}

impl BySection {
    fn new(decide: Decide) -> Self {
        Self {
            wire: WireId::new("fake").expect("a wire id"),
            decide,
            asked: Vec::new(),
        }
    }
}

/// The section a step's turn says it belongs to.
fn section_of(turn: &str) -> Option<&str> {
    let (_, after) = turn.split_once("of section `")?;
    let (section, _) = after.split_once('`')?;
    Some(section)
}

impl ModelPort for BySection {
    fn wire(&self) -> &WireId {
        &self.wire
    }

    fn turn(
        &mut self,
        request: &TurnRequest,
        _sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        let turn = request
            .items
            .iter()
            .rev()
            .find_map(|item| match item {
                Item::UserText { text } => Some(text.as_str()),
                _ => None,
            })
            .ok_or_else(|| WireError::protocol("the request carries no user turn"))?;
        let section = section_of(turn)
            .ok_or_else(|| WireError::protocol("the turn names no section"))?
            .to_owned();
        let before = self.asked.iter().filter(|asked| **asked == section).count();
        let outcome = (self.decide)(&section, before);
        self.asked.push(section);
        Ok(TurnOutcome {
            stop_reason: StopReason::ToolCalls,
            items: vec![Item::ToolCall(ToolCall {
                // Only so the call is matched to its result; nothing is decided by it.
                call_id: CallId::new(format!("answer-{}", self.asked.len())).expect("a call id"),
                name: ToolName::new(DEFAULT_ANSWER_NAME).expect("a tool name"),
                arguments: json!({"outcome": outcome}),
            })],
            usage: None,
        })
    }
}

/// The read-only toolset over a fresh directory: the steps here call nothing but `answer`.
fn toolset(dir: &tempfile::TempDir) -> Flat {
    Flat::new(Catalogue::of(
        LocalOperations::new(dir.path()).expect("the workspace opens"),
    ))
}

fn config() -> LoopConfig {
    LoopConfig::new("fake-model", "Finish every step by calling `answer`.")
}

fn projected() -> Flow {
    Flow::from_yaml(PROJECTED).expect("the projected document reads")
}

/// The outcome of every `answer` the loop accepted, in order.
fn answered(events: &VecLoopSink) -> Vec<String> {
    events
        .events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::Answered { value, .. } => value
                .get("outcome")
                .and_then(Value::as_str)
                .map(str::to_owned),
            _ => None,
        })
        .collect()
}

fn completed_runs(events: &VecLoopSink) -> usize {
    events
        .events()
        .iter()
        .filter(|event| {
            matches!(
                event,
                LoopEvent::Finished {
                    stop: LoopStop::Completed,
                    ..
                }
            )
        })
        .count()
}

#[test]
fn the_projected_adp_workflow_walks_to_flow_finished_through_the_library() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut tools = toolset(&dir);
    // Review sends the work back once, then accepts it.
    let mut model = BySection::new(|section, before| {
        if section == REVIEW && before == 0 {
            "failed"
        } else {
            "passed"
        }
    });
    let mut approvals = DenyAll;
    let (mut flow_events, mut loop_events) = (VecFlowSink::new(), VecLoopSink::new());

    let report = Walk::new(&mut model, &mut tools, &mut approvals, config())
        .run(
            &projected(),
            "add a CSV export",
            &mut flow_events,
            &mut loop_events,
        )
        .expect("the walk runs");

    assert_eq!(report.status(), FlowStatus::Completed);
    // `flow-finished` names the root group, as `harness-flow` emits it; the governor is told the
    // document's id.
    assert_eq!(
        flow_events.events().last(),
        Some(&FlowEvent::FlowFinished {
            flow: "root".to_owned(),
            ran: 12,
            failed: 1,
            skipped: 0,
            retreats: 1,
            clean: true,
        }),
        "four states, then the retreat twice: {:?}",
        flow_events.events()
    );
    assert!(
        flow_events.events().contains(&FlowEvent::GroupRepeating {
            path: RETREAT.to_owned(),
            attempt: 1,
            of: 3,
        }),
        "the retreat is an event of its own"
    );
    let started = flow_events.steps_started();
    assert_eq!(
        started
            .iter()
            .filter(|path| path.ends_with("implement-1"))
            .count(),
        2,
        "implement ran again after the retreat: {started:?}"
    );
    assert_eq!(
        model
            .asked
            .iter()
            .filter(|section| section.as_str() == REVIEW)
            .count(),
        2
    );
    // The loop's half of the record: one completed run per step, each ended by an accepted
    // answer, the one refusal among them where the walk retreated, and nothing warned about.
    assert_eq!(completed_runs(&loop_events), 12);
    let outcomes = answered(&loop_events);
    assert_eq!(outcomes.len(), 12);
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| *outcome == "failed")
            .count(),
        1
    );
    assert_eq!(
        loop_events.warnings().collect::<Vec<_>>(),
        Vec::<(&str, &str)>::new()
    );
    assert!(
        !loop_events
            .events()
            .iter()
            .any(|event| matches!(event, LoopEvent::HookRan { .. })),
        "no governor was given, so no consultation is recorded"
    );
}

#[test]
fn a_retreat_that_keeps_failing_stops_at_its_bound_through_the_library() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut tools = toolset(&dir);
    let mut model = BySection::new(|section, _| {
        if section == REVIEW {
            "failed"
        } else {
            "passed"
        }
    });
    let mut approvals = DenyAll;
    let (mut flow_events, mut loop_events) = (VecFlowSink::new(), VecLoopSink::new());

    let report = Walk::new(&mut model, &mut tools, &mut approvals, config())
        .run(
            &projected(),
            "add a CSV export",
            &mut flow_events,
            &mut loop_events,
        )
        .expect("the walk runs");

    assert_eq!(report.status(), FlowStatus::Failed);
    assert!(
        flow_events.events().contains(&FlowEvent::GroupLeft {
            path: RETREAT.to_owned(),
            failed: true,
            gave: Vec::new(),
            attempts: 3,
            exhausted: true,
        }),
        "the bound is what stopped it, and the record says so: {:?}",
        flow_events.events()
    );
    let repeating: Vec<u32> = flow_events
        .events()
        .iter()
        .filter_map(|event| match event {
            FlowEvent::GroupRepeating { path, attempt, of } if path == RETREAT => {
                assert_eq!(*of, 3);
                Some(*attempt)
            }
            _ => None,
        })
        .collect();
    assert_eq!(repeating, vec![1, 2], "two retreats, and no third");
    assert_eq!(
        flow_events.events().last(),
        Some(&FlowEvent::FlowFinished {
            flow: "root".to_owned(),
            ran: 16,
            failed: 3,
            skipped: 0,
            retreats: 2,
            clean: false,
        })
    );
    assert_eq!(
        model
            .asked
            .iter()
            .filter(|section| section.as_str() == REVIEW)
            .count(),
        3,
        "review was asked once per attempt the bound allows"
    );
    // Every step still ended in an accepted answer: a step that answers *failed* is a failed
    // step, not a stopped run, so the loop warns about nothing.
    assert_eq!(completed_runs(&loop_events), 16);
    assert_eq!(
        answered(&loop_events)
            .iter()
            .filter(|outcome| *outcome == "failed")
            .count(),
        3
    );
    assert_eq!(loop_events.warnings().count(), 0);
}

/// Refuses one boundary, and records every boundary it was asked about.
struct Refusing {
    path: &'static str,
    moment: Moment,
    reason: &'static str,
    asked: Vec<(String, Moment, u32, u32)>,
}

impl Governor for Refusing {
    fn transition(&mut self, transition: &Transition<'_>) -> HookDecision {
        self.asked.push((
            transition.path.to_owned(),
            transition.moment,
            transition.attempt,
            transition.of,
        ));
        assert_eq!(transition.flow, "adp/default");
        assert_eq!(
            transition.leave.is_some(),
            transition.moment == Moment::Leave,
            "a leave carries its result and an enter does not"
        );
        if transition.path == self.path && transition.moment == self.moment {
            HookDecision::Block {
                reason: self.reason.to_owned(),
            }
        } else {
            HookDecision::Proceed
        }
    }
}

#[test]
fn a_transition_the_callers_governor_refuses_ends_the_walk_with_that_refusal() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut tools = toolset(&dir);
    let mut model = BySection::new(|_, _| "passed");
    let mut approvals = DenyAll;
    let mut governor = Refusing {
        path: RETREAT,
        moment: Moment::Enter,
        reason: "no implementation before the specification is signed",
        asked: Vec::new(),
    };
    let (mut flow_events, mut loop_events) = (VecFlowSink::new(), VecLoopSink::new());

    let report = Walk::new(&mut model, &mut tools, &mut approvals, config())
        .with_governor(&mut governor)
        .run(
            &projected(),
            "add a CSV export",
            &mut flow_events,
            &mut loop_events,
        )
        .expect("the walk runs");

    assert_eq!(report.status(), FlowStatus::Failed);
    assert!(
        flow_events
            .events()
            .contains(&FlowEvent::TransitionRefused {
                path: RETREAT.to_owned(),
                moment: Moment::Enter,
                attempt: 1,
                reason: "no implementation before the specification is signed".to_owned(),
            }),
        "the governor's own words reach the record: {:?}",
        flow_events.events()
    );
    assert_eq!(
        flow_events.events().last(),
        Some(&FlowEvent::FlowFinished {
            flow: "root".to_owned(),
            ran: 4,
            failed: 0,
            skipped: 4,
            retreats: 0,
            clean: false,
        })
    );
    assert!(
        model
            .asked
            .iter()
            .all(|section| !section.starts_with(RETREAT)),
        "no step of the refused section asked the model anything: {:?}",
        model.asked
    );
    assert!(
        governor
            .asked
            .contains(&(RETREAT.to_owned(), Moment::Enter, 1, 3)),
        "the governor is told the bound the plan reconciled: {:?}",
        governor.asked
    );
    // Every consultation is in the loop's record, the refusal with its words.
    let consulted: Vec<&HookDecision> = loop_events
        .events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::HookRan {
                point: HookPoint::Transition,
                call_id: None,
                decision,
            } => Some(decision),
            _ => None,
        })
        .collect();
    assert_eq!(consulted.len(), governor.asked.len());
    assert_eq!(
        consulted
            .iter()
            .filter(|decision| matches!(decision, HookDecision::Block { .. }))
            .collect::<Vec<_>>(),
        vec![&&HookDecision::Block {
            reason: "no implementation before the specification is signed".to_owned()
        }]
    );
}
