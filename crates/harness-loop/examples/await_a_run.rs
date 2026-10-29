//! Await a run, and a run that continues its conversation, from a tokio program.
//!
//! `LoopTask` is the recommended way to embed the loop in an async program: the builder runs on
//! the task's own thread, which holds the ports from construction to drop, and the program only
//! ever awaits futures. The model here answers from a script so the example runs offline; a
//! program opens its model client and its tools in the builder instead.
//!
//! ```console
//! cargo run -p b10x-harness-loop --example await_a_run
//! ```

use std::collections::VecDeque;
use std::error::Error;

use b10x_harness_loop::{
    DenyAll, LoopCancel, LoopConfig, LoopPorts, LoopTask, Ran, RanIn, VecLoopSink,
};
use harness_wire::{
    Item, ModelPort, StopReason, StreamEvent, StreamSink, ToolCall, ToolOutcome, ToolPort,
    ToolSpec, TurnOutcome, TurnRequest, WireError, WireId,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = LoopConfig::new("fixture", "Answer briefly.");
    let task = LoopTask::spawn(config, |_cancel: &LoopCancel| {
        // On the task's own thread: open the model client and the tools here.
        let model = Scripted::answering(&["hello", "hello again"]);
        Ok::<_, String>(LoopPorts::new(model, NoTools, DenyAll))
    });

    // The sink comes back with the result, holding the run's events.
    let Ran { result, sink } = task.run("Say hello.", VecLoopSink::new()).await?;
    let first = result?;
    println!("run: {:?}, streamed {:?}", first.stop, sink.text());

    // `run_in` continues a conversation; the items and the spend come back however it ended.
    let next = task.run_in(first.items, "Once more.", VecLoopSink::new());
    let RanIn {
        result,
        items,
        spend,
        ..
    } = next.await?;
    let second = result?;
    println!(
        "run_in: {:?}, {} items, {} turn",
        second.stop,
        items.len(),
        spend.turns
    );

    task.shutdown().await?;
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
