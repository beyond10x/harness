//! The shipped toolset stays read-only, and `run` meets the approval gate.
//!
//! `tool_code_editor` writes and the adapter can start a process, but neither reaches the model
//! unasked: the writer is published only when `Operations::writes()` is true, and `run` is
//! `Risk::High` behind the approver. This exercises that last clause through the **real adapter**
//! rather than through a scripted port: a `run` entry built by `Catalogue` from
//! `LocalOperations::unconfined` is invoked through `AgentLoop::call` under the library default
//! approver `DenyAll`, and must come back a failed `ToolOutcome` with nothing started — the gate
//! `AGENTS.md` § Safety envelope describes, mirroring `harness-loop`'s own `asked == ["run"]`.

use harness_catalogue::{Catalogue, Flat, LocalOperations};
use harness_loop::{AgentLoop, DenyAll, LoopConfig, LoopEvent, VecLoopSink};
use harness_wire::{
    CallId, ModelPort, StreamSink, ToolCall, ToolName, TurnOutcome, TurnRequest, WireError, WireId,
};
use serde_json::json;

/// A model port the single-call path never reaches: `AgentLoop::call` sends nothing to the
/// provider, so `turn` is unreachable and only `wire` is answered.
struct NoModel {
    wire: WireId,
}

impl NoModel {
    fn new() -> Self {
        Self {
            wire: WireId::new("openai-responses").expect("a valid wire id"),
        }
    }
}

impl ModelPort for NoModel {
    fn wire(&self) -> &WireId {
        &self.wire
    }

    fn turn(
        &mut self,
        _request: &TurnRequest,
        _sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        unreachable!("`AgentLoop::call` runs one call outside any turn and asks the model nothing")
    }
}

#[test]
fn run_built_from_the_adapter_is_refused_under_deny_all() {
    let dir = tempfile::tempdir().expect("a temporary workspace");
    // The real adapter, writing and running a declared program — so the catalogue publishes a `run`
    // entry, built from `programs()`, at `Risk::High`.
    let operations =
        LocalOperations::unconfined(dir.path(), vec!["cargo".to_owned()]).expect("a workspace");
    let catalogue = Catalogue::of(operations);
    assert!(
        catalogue.entries().iter().any(|entry| entry.name == "run"),
        "the adapter publishes `run` when a program is declared"
    );

    let mut model = NoModel::new();
    let mut tools = Flat::new(catalogue);
    // The library default, and the one the whole gate turns on (AGENTS.md invariant 12).
    let mut approvals = DenyAll;
    let config = LoopConfig::new("b10x-emulated", "do the task");
    let mut agent = AgentLoop::new(&mut model, &mut tools, &mut approvals, config);

    let mut sink = VecLoopSink::new();
    let call = ToolCall {
        call_id: CallId::new("c-run").expect("a valid id"),
        name: ToolName::new("run").expect("a valid name"),
        arguments: json!({ "argv": ["cargo", "--version"] }),
    };
    let outcome = agent.call(&call, &mut sink);

    assert!(
        outcome.failed,
        "a `run` denied by `DenyAll` is a failed outcome, so the model sees it did not happen"
    );
    assert!(
        outcome
            .output
            .as_str()
            .unwrap_or_default()
            .contains("not approved"),
        "the refusal says the call was not approved: {:?}",
        outcome.output
    );
    // What was asked about is the `run` entry itself, mirroring `harness-loop`'s `asked == ["run"]`.
    let asked: Vec<&str> = sink
        .events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::ApprovalRequired { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        asked,
        vec!["run"],
        "the gate decided on the `run` entry by name"
    );
    // Nothing was started: the workspace is as empty as it was seeded.
    assert!(
        std::fs::read_dir(dir.path())
            .expect("the workspace is readable")
            .next()
            .is_none(),
        "a denied `run` starts no process and leaves nothing behind"
    );
}
