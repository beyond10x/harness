//! What a library caller's walk refuses, asked through `Walk` with no harness binary anywhere.
//!
//! `story:workflow-run-through-the-library` moved the walk out of the command line. These cases ask
//! the library the questions the command line used to answer for it: a command step meets the
//! caller's approver; a node's `scope` binds a write the approver allowed, and is taken off again;
//! a `context` file is read only from a workspace the caller handed over, and only inside it.

use std::path::Path;

use harness_catalogue::{Catalogue, Flat, LocalOperations};
use harness_flow::{Flow, FlowStatus, VecFlowSink};
use harness_loop::{
    ApprovalPort, ApproveAll, DEFAULT_ANSWER_NAME, DenyAll, LoopConfig, LoopEvent, VecLoopSink,
};
use harness_wire::{
    CallId, Item, ModelPort, StopReason, StreamSink, ToolCall, ToolName, TurnOutcome, TurnRequest,
    WireError, WireId,
};
use harness_workflow::{StepTools, Walk};
use serde_json::json;

const SHELL: &str = "/bin/sh";

/// A model that writes `wrote.txt` on its first turn of a step and answers `passed` on the next.
struct WritesThenAnswers {
    wire: WireId,
    asked: usize,
}

impl WritesThenAnswers {
    fn new() -> Self {
        Self {
            wire: WireId::new("fake").expect("a wire id"),
            asked: 0,
        }
    }
}

impl ModelPort for WritesThenAnswers {
    fn wire(&self) -> &WireId {
        &self.wire
    }

    fn turn(
        &mut self,
        request: &TurnRequest,
        _sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        self.asked += 1;
        let wrote_already = request
            .items
            .iter()
            .any(|item| matches!(item, Item::ToolCall(call) if call.name.as_str() == "file_write"));
        let call = if wrote_already {
            ToolCall {
                call_id: CallId::new(format!("answer-{}", self.asked)).expect("a call id"),
                name: ToolName::new(DEFAULT_ANSWER_NAME).expect("a tool name"),
                arguments: json!({"outcome": "passed"}),
            }
        } else {
            ToolCall {
                call_id: CallId::new(format!("write-{}", self.asked)).expect("a call id"),
                name: ToolName::new("file_write").expect("a tool name"),
                arguments: json!({"path": "wrote.txt", "text": "written\n"}),
            }
        };
        Ok(TurnOutcome {
            stop_reason: StopReason::ToolCalls,
            items: vec![Item::ToolCall(call)],
            usage: None,
        })
    }
}

/// The writing toolset over `dir`, with only the shell declared.
fn writing(dir: &Path) -> Flat {
    Flat::new(Catalogue::of(
        LocalOperations::unconfined(dir, vec![SHELL.to_owned()]).expect("the workspace opens"),
    ))
}

fn config() -> LoopConfig {
    LoopConfig::new("fake-model", "Finish every step by calling `answer`.")
}

fn warnings(events: &VecLoopSink) -> Vec<String> {
    events
        .events()
        .iter()
        .filter_map(|event| match event {
            LoopEvent::Warning { code, .. } => Some(code.clone()),
            _ => None,
        })
        .collect()
}

fn walk(
    document: &str,
    tools: &mut Flat,
    approvals: &mut dyn ApprovalPort,
    workspace: Option<&Path>,
) -> (FlowStatus, WritesThenAnswers, VecLoopSink) {
    let flow = Flow::from_yaml(document).expect("the document reads");
    let mut model = WritesThenAnswers::new();
    let (mut flow_events, mut loop_events) = (VecFlowSink::new(), VecLoopSink::new());
    let mut walk = Walk::new(&mut model, tools, approvals, config());
    if let Some(workspace) = workspace {
        walk = walk.with_workspace(workspace);
    }
    let report = walk
        .run(&flow, "the task", &mut flow_events, &mut loop_events)
        .expect("the walk runs");
    (report.status(), model, loop_events)
}

const COMMANDED: &str = "\
id: commanded
root:
  id: root
  nodes:
    - id: verify
      run:
        state: verify
        kind: command
        command: [/bin/sh, -c, \": > ran.txt\"]
";

#[test]
fn a_command_step_meets_the_callers_approver_and_deny_all_refuses_it() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut tools = writing(dir.path());
    let (status, model, _) = walk(COMMANDED, &mut tools, &mut DenyAll, None);
    assert_eq!(
        status,
        FlowStatus::Failed,
        "a refused command fails its step"
    );
    assert_eq!(model.asked, 0, "a command step asks no model");
    assert!(
        !dir.path().join("ran.txt").exists(),
        "the program did not run under DenyAll"
    );

    // The control: the same document under an approver that says yes does run it, so the refusal
    // above was the approver's and not a program that could not start.
    let control = tempfile::tempdir().expect("a temporary directory");
    let mut tools = writing(control.path());
    let (status, _, _) = walk(COMMANDED, &mut tools, &mut ApproveAll, None);
    assert_eq!(status, FlowStatus::Completed);
    assert!(control.path().join("ran.txt").exists(), "the program ran");
}

const DENIED_THEN_OPEN: &str = "\
id: scoped
root:
  id: root
  nodes:
    - id: denied
      run:
        state: denied
        prompt: \"Write wrote.txt.\"
        scope: [\"**=denied\"]
";

#[test]
fn a_nodes_scope_binds_a_write_the_approver_allowed_and_is_taken_off_after() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let mut tools = writing(dir.path());
    let (_, model, events) = walk(DENIED_THEN_OPEN, &mut tools, &mut ApproveAll, None);
    assert!(model.asked >= 1, "the step asked the model");
    assert!(
        !dir.path().join("wrote.txt").exists(),
        "the node's `**=denied` refused the write ApproveAll let through: {:?}",
        warnings(&events)
    );
    assert!(
        StepTools::catalogue(&tools).step_scope().is_empty(),
        "the node's scope was taken off when its step ended"
    );

    // The control: without the node's scope the same write lands, so the refusal above was the
    // scope's.
    let control = tempfile::tempdir().expect("a temporary directory");
    let mut tools = writing(control.path());
    let open = DENIED_THEN_OPEN.replace("        scope: [\"**=denied\"]\n", "");
    let (status, _, _) = walk(&open, &mut tools, &mut ApproveAll, None);
    assert_eq!(status, FlowStatus::Completed);
    assert!(
        control.path().join("wrote.txt").exists(),
        "the write landed"
    );
}

const CONTEXTED: &str = "\
id: contexted
root:
  id: root
  nodes:
    - id: read
      run:
        state: read
        prompt: \"Read it.\"
        context: [notes.md]
";

const CLIMBING: &str = "\
id: climbing
root:
  id: root
  nodes:
    - id: read
      run:
        state: read
        prompt: \"Read it.\"
        context: [link.md]
";

#[test]
fn a_context_file_with_no_workspace_handed_over_is_refused_before_any_model_is_asked() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(dir.path().join("notes.md"), "notes\n").expect("a seed");
    let mut tools = writing(dir.path());
    let (status, model, events) = walk(CONTEXTED, &mut tools, &mut DenyAll, None);
    assert_eq!(status, FlowStatus::Failed);
    assert_eq!(model.asked, 0, "no model was asked without its context");
    assert!(
        warnings(&events)
            .iter()
            .any(|code| code == "context-refused"),
        "{:?}",
        warnings(&events)
    );
}

#[test]
fn a_context_link_out_of_the_workspace_is_refused_before_any_model_is_asked() {
    let root = tempfile::tempdir().expect("a temporary directory");
    let work = root.path().join("work");
    std::fs::create_dir(&work).expect("the tree");
    std::fs::write(root.path().join("secret.md"), "outside\n").expect("a file outside");
    std::os::unix::fs::symlink(root.path().join("secret.md"), work.join("link.md"))
        .expect("a link out");
    let mut tools = writing(&work);
    let (status, model, events) = walk(CLIMBING, &mut tools, &mut DenyAll, Some(&work));
    assert_eq!(status, FlowStatus::Failed);
    assert_eq!(
        model.asked, 0,
        "no model was asked with a file from outside"
    );
    assert!(
        warnings(&events)
            .iter()
            .any(|code| code == "context-refused"),
        "{:?}",
        warnings(&events)
    );
}
