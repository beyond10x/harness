//! Run the loop from a program without an async runtime.
//!
//! `AgentLoop` borrows its ports and blocks the calling thread until the run ends. It is the entry
//! for a program that has no async runtime; a program that has one awaits `LoopTask` instead (see
//! the `await_a_run` example). The model answers from a script so the example runs offline.
//!
//! ```console
//! cargo run -p b10x-harness-loop --example blocking_run
//! ```

use std::collections::VecDeque;
use std::error::Error;

use b10x_harness_loop::{AgentLoop, DenyAll, LoopConfig, VecLoopSink};
use harness_wire::{
    Item, ModelPort, StopReason, StreamEvent, StreamSink, ToolCall, ToolOutcome, ToolPort,
    ToolSpec, TurnOutcome, TurnRequest, WireError, WireId,
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut model = Scripted::answering(&["hello"]);
    let (mut tools, mut approvals) = (NoTools, DenyAll);
    let config = LoopConfig::new("fixture", "Answer briefly.");

    // Blocks this thread until the run ends.
    let mut sink = VecLoopSink::new();
    let outcome = AgentLoop::new(&mut model, &mut tools, &mut approvals, config)
        .run("Say hello.", &mut sink)?;
    println!("run: {:?}, streamed {:?}", outcome.stop, sink.text());
    Ok(())
}

/// A model that answers each turn with the next scripted text.
struct Scripted {
    wire: WireId,
    answers: VecDeque<String>,
}

impl Scripted {
    fn answering(answers: &[&str]) -> Self {
        Self {
            wire: WireId::new("fixture").expect("a fixed wire id is valid"),
            answers: answers.iter().map(|text| (*text).to_owned()).collect(),
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
        let text = self
            .answers
            .pop_front()
            .ok_or_else(|| WireError::protocol("the script is exhausted"))?;
        sink.emit(StreamEvent::TextDelta { text: text.clone() });
        Ok(TurnOutcome {
            stop_reason: StopReason::EndTurn,
            items: vec![Item::AssistantText { text }],
            usage: None,
        })
    }
}

/// A tool port that publishes nothing.
struct NoTools;

impl ToolPort for NoTools {
    fn specs(&self) -> &[ToolSpec] {
        &[]
    }

    fn call(&mut self, _call: &ToolCall) -> ToolOutcome {
        ToolOutcome::failed("this example publishes no tools")
    }
}
