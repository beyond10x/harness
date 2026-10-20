//! Adversary cases for `story:an-undeclared-field-is-refused`, through the loop and not the port.
//!
//! The unit's own suite calls `Flat` and `Verbs` directly, which is how the served surface reaches
//! them. A model's call reaches the same port through `AgentLoop`, and there the catalogue's
//! check runs only once the port is called: after the approver was asked and after the operator's
//! `before-call` hook ran, and its refusal is handed to `after-call` as an outcome of a tool that
//! never ran. The loop's own tools are judged before their hook (`gate_owned`); a catalogue entry
//! is not.

#![cfg(unix)]

use harness_catalogue::{Catalogue, Flat, LocalOperations};
use harness_loop::{
    AfterCall, AgentLoop, ApprovalDecision, ApprovalPort, DenyAll, HookDecision, HookPort,
    LoopConfig, VecLoopSink,
};
use harness_wire::{
    CallId, ModelPort, StreamSink, ToolCall, ToolName, ToolOutcome, ToolSpec, TurnOutcome,
    TurnRequest, WireError, WireId,
};
use serde_json::{Value, json};

const BEFORE: &str = "before\n";
const STALE: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// `AgentLoop::call` asks the model nothing.
struct NoModel(WireId);

impl ModelPort for NoModel {
    fn wire(&self) -> &WireId {
        &self.0
    }

    fn turn(
        &mut self,
        _request: &TurnRequest,
        _sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        unreachable!("one call outside any turn")
    }
}

/// A person who says yes, and remembers being asked.
#[derive(Default)]
struct Yes {
    asked: Vec<String>,
}

impl ApprovalPort for Yes {
    fn decide(&mut self, _call: &ToolCall, spec: &ToolSpec) -> ApprovalDecision {
        self.asked.push(spec.name.as_str().to_owned());
        ApprovalDecision::Approved
    }
}

/// An operator's hooks that let everything through, and remember firing.
#[derive(Default)]
struct Watching {
    before: Vec<String>,
    after: Vec<String>,
}

impl HookPort for Watching {
    fn before_call(&mut self, _call: &ToolCall, invoked: &ToolSpec) -> HookDecision {
        self.before.push(invoked.name.as_str().to_owned());
        HookDecision::Proceed
    }

    fn after_call(
        &mut self,
        _call: &ToolCall,
        invoked: &ToolSpec,
        _outcome: &ToolOutcome,
    ) -> AfterCall {
        self.after.push(invoked.name.as_str().to_owned());
        AfterCall::default()
    }
}

fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary workspace");
    std::fs::write(dir.path().join("notes.txt"), BEFORE).expect("the guarded file");
    dir
}

fn flat(root: &std::path::Path) -> Flat {
    Flat::new(Catalogue::of(
        LocalOperations::unconfined(root, Vec::new()).expect("a workspace"),
    ))
}

fn guarded_write() -> ToolCall {
    ToolCall {
        call_id: CallId::new("c-1").expect("valid"),
        name: ToolName::new("file_write").expect("valid"),
        arguments: json!({"path": "notes.txt", "text": "after\n", "expected_sha256": STALE}),
    }
}

fn words(output: &Value) -> String {
    output
        .as_str()
        .map_or_else(|| output.to_string(), ToOwned::to_owned)
}

#[test]
fn a_file_write_carrying_an_undeclared_field_is_refused_before_a_person_or_a_hook_is_asked() {
    let dir = workspace();
    let mut model = NoModel(WireId::new("openai-responses").expect("a wire id"));
    let mut tools = flat(dir.path());
    let mut approvals = Yes::default();
    let mut hooks = Watching::default();
    let outcome = {
        let mut agent = AgentLoop::new(
            &mut model,
            &mut tools,
            &mut approvals,
            LoopConfig::new("b10x-emulated", "do the task"),
        )
        .with_hooks(&mut hooks);
        agent.call(&guarded_write(), &mut VecLoopSink::new())
    };
    let said = words(&outcome.output);
    assert!(outcome.failed, "{said}");
    assert!(said.contains("`expected_sha256`"), "{said}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("notes.txt")).expect("readable"),
        BEFORE
    );
    // Refused by the published schema alone: nothing about the call needed a person, and the
    // loop's own tools are judged before their `before-call` hook (`gate_owned`).
    // `HookPort::after_call`: "It does not fire for a call that never ran"; the writer never ran.
    // All three asked at once, so one red run shows every stage the refusal came after.
    let empty: Vec<String> = Vec::new();
    assert_eq!(
        (&approvals.asked, &hooks.before, &hooks.after),
        (&empty, &empty, &empty),
        "(approver asked, before-call fired, after-call fired) for a call the schema refuses"
    );
}

#[test]
fn under_the_default_approver_a_file_write_carrying_expected_sha256_is_refused_naming_it() {
    // The story's first acceptance line, through the loop and its library-default approver
    // (`AGENTS.md` invariant 12) rather than straight into the port.
    let dir = workspace();
    let mut model = NoModel(WireId::new("openai-responses").expect("a wire id"));
    let mut tools = flat(dir.path());
    let mut approvals = DenyAll;
    let outcome = AgentLoop::new(
        &mut model,
        &mut tools,
        &mut approvals,
        LoopConfig::new("b10x-emulated", "do the task"),
    )
    .call(&guarded_write(), &mut VecLoopSink::new());
    let said = words(&outcome.output);
    assert!(outcome.failed, "{said}");
    assert!(
        said.contains("`expected_sha256`"),
        "the refusal does not name the undeclared field: {said}"
    );
}
