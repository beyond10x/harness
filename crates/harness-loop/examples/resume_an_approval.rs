//! Await a run that stops for a person's decision, then await its resumption.
//!
//! The approver defers instead of deciding, so the run ends `AwaitingApproval` before the effect
//! and hands back an `ApprovalCheckpoint`. The program stores the checkpoint, and later, on a
//! fresh task with fresh ports, resumes it with the decision. The model answers from a script so
//! the example runs offline.
//!
//! ```console
//! cargo run -p b10x-harness-loop --example resume_an_approval
//! ```

use std::collections::VecDeque;
use std::error::Error;

use b10x_harness_loop::{
    ApprovalCheckpoint, ApprovalDecision, ApprovalPort, LoopCancel, LoopConfig, LoopPorts,
    LoopTask, Ran, VecLoopSink,
};
use harness_wire::{
    Approval, CallId, Envelope, Item, ModelPort, Risk, StopReason, StreamEvent, StreamSink,
    ToolCall, ToolName, ToolOutcome, ToolPort, ToolSpec, TurnOutcome, TurnRequest, WireError,
    WireId,
};
use serde_json::json;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = LoopConfig::new("fixture", "Answer briefly.");

    let task = LoopTask::spawn(config.clone(), |_cancel: &LoopCancel| {
        let model = Scripted::new(vec![asks_to_write()]);
        Ok::<_, String>(LoopPorts::new(model, Notes::new(), Defer))
    });
    let Ran { result, .. } = task.run("Write the note.", VecLoopSink::new()).await?;
    let suspended = result?;
    println!("run: {:?}", suspended.stop);
    let stored = serde_json::to_string(&suspended.checkpoint)?;
    task.shutdown().await?;

    // Later, perhaps in another process: fresh ports, the stored checkpoint, a person's decision.
    let checkpoint: Option<ApprovalCheckpoint> = serde_json::from_str(&stored)?;
    let checkpoint = checkpoint.ok_or("the run did not stop for approval")?;
    let task = LoopTask::spawn(config, |_cancel: &LoopCancel| {
        let model = Scripted::new(vec![answers("The note is written.")]);
        Ok::<_, String>(LoopPorts::new(model, Notes::new(), Defer))
    });
    let resume = task.resume_approval(checkpoint, ApprovalDecision::Approved, VecLoopSink::new());
    let Ran { result, sink } = resume.await?;
    let resumed = result?;
    println!(
        "resume_approval: {:?}, streamed {:?}",
        resumed.stop,
        sink.text()
    );
    task.shutdown().await?;
    Ok(())
}

/// Defers every decision under one checkpoint id, as a hosted approver that asks a person
/// elsewhere does.
struct Defer;

impl ApprovalPort for Defer {
    fn decide(&mut self, _call: &ToolCall, _spec: &ToolSpec) -> ApprovalDecision {
        ApprovalDecision::deferred("note-approval-1")
    }
}

/// One `write_note` tool at `Risk::Medium`, above the default unattended ceiling, so the loop
/// asks the approver before every call.
struct Notes {
    specs: Vec<ToolSpec>,
}

impl Notes {
    fn new() -> Self {
        Self {
            specs: vec![ToolSpec {
                name: ToolName::new("write_note").expect("a fixed tool name is valid"),
                description: "Writes a note.".to_owned(),
                input_schema: json!({"type": "object"}),
                approval: Approval::NotRequired,
                envelope: Envelope {
                    risk: Risk::Medium,
                    ..Envelope::default()
                },
            }],
        }
    }
}

impl ToolPort for Notes {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        println!("effect: {} ran", call.name.as_str());
        ToolOutcome::ok(json!("written"))
    }
}

fn asks_to_write() -> TurnOutcome {
    TurnOutcome {
        stop_reason: StopReason::ToolCalls,
        items: vec![Item::ToolCall(ToolCall {
            call_id: CallId::new("call-1").expect("a fixed call id is valid"),
            name: ToolName::new("write_note").expect("a fixed tool name is valid"),
            arguments: json!({}),
        })],
        usage: None,
    }
}

fn answers(text: &str) -> TurnOutcome {
    TurnOutcome {
        stop_reason: StopReason::EndTurn,
        items: vec![Item::AssistantText {
            text: text.to_owned(),
        }],
        usage: None,
    }
}

/// A model that answers each turn with the next scripted outcome.
struct Scripted {
    wire: WireId,
    turns: VecDeque<TurnOutcome>,
}

impl Scripted {
    fn new(turns: Vec<TurnOutcome>) -> Self {
        Self {
            wire: WireId::new("fixture").expect("a fixed wire id is valid"),
            turns: turns.into(),
        }
    }
}

impl ModelPort for Scripted {
    fn wire(&self) -> &WireId {
        &self.wire
    }

    fn turn(
        &mut self,
        _request: &TurnRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError> {
        let turn = self
            .turns
            .pop_front()
            .ok_or_else(|| WireError::protocol("the script is exhausted"))?;
        for item in &turn.items {
            if let Item::AssistantText { text } = item {
                sink.emit(StreamEvent::TextDelta { text: text.clone() });
            }
        }
        Ok(turn)
    }
}
