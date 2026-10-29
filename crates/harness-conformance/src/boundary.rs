//! Calls existing production APIs; only upstream ports are scripted.
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use harness_loop::{
    AgentLoop, ApprovalPort, ApproveAll, Budget, DenyAll, LoopConfig, LoopError, LoopOutcome,
    RunLedger, VecLoopSink,
};
use harness_wire::{
    Approval, CallId, Envelope, Item, ModelPort, Risk, StreamSink, ToolCall, ToolName, ToolOutcome,
    ToolPort, ToolSpec, TurnOutcome, TurnRequest, WireError, WireId,
};
use serde_json::{Value, json};

use crate::Result;

/// How often a port was reached, readable after the port has moved to another thread.
type Count = Arc<AtomicU64>;

struct Model {
    wire: WireId,
    script: VecDeque<TurnOutcome>,
    calls: Count,
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
        request.check_opaque_items(&self.wire)?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.script
            .pop_front()
            .ok_or_else(|| WireError::protocol("fixture model exhausted"))
    }
}

struct Tools {
    specs: Vec<ToolSpec>,
    calls: Count,
}
impl Tools {
    fn new(calls: Count) -> Result<Self> {
        let mut specs = Vec::new();
        for (name, risk) in [("read", Risk::Low), ("write", Risk::Medium)] {
            specs.push(ToolSpec {
                name: ToolName::new(name)?,
                description: name.into(),
                input_schema: json!({"type":"object"}),
                approval: Approval::NotRequired,
                envelope: Envelope {
                    risk,
                    ..Envelope::default()
                },
            });
        }
        Ok(Self { specs, calls })
    }
}
impl ToolPort for Tools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }
    fn call(&mut self, _: &ToolCall) -> ToolOutcome {
        self.calls.fetch_add(1, Ordering::SeqCst);
        ToolOutcome::ok(json!("effect"))
    }
}

pub fn execute(command: &str, input: &Value) -> Result<Value> {
    match command {
        "harness.loop.ValidateBudget" => {
            let budget: Budget = serde_json::from_value(input["budget"].clone())?;
            let diagnostic = budget
                .validate(boolean(input, "priced")?)
                .err()
                .map(|e| e.to_string());
            Ok(json!({"accepted":diagnostic.is_none(),"diagnostic":diagnostic}))
        }
        "harness.loop.Run" => run_loop(input),
        "harness.loop.Call" => call(input),
        _ => Err(format!("no production adapter for {command}").into()),
    }
}
fn boolean(value: &Value, field: &str) -> Result<bool> {
    value[field]
        .as_bool()
        .ok_or_else(|| format!("{field} must be Boolean").into())
}
fn model(script: VecDeque<TurnOutcome>, calls: Count) -> Result<Model> {
    Ok(Model {
        wire: WireId::new("fixture")?,
        script,
        calls,
    })
}
fn approver(approve: bool) -> Box<dyn ApprovalPort> {
    if approve {
        Box::new(ApproveAll)
    } else {
        Box::new(DenyAll)
    }
}

/// One `harness.loop.Run` input, read once, so every way of driving it starts from the same ports.
struct RunFixture {
    script: VecDeque<TurnOutcome>,
    approve: bool,
    cancelled: bool,
    config: LoopConfig,
    model_calls: Count,
    tool_calls: Count,
}
impl RunFixture {
    fn read(input: &Value) -> Result<Self> {
        let mut config = LoopConfig::new("fixture", "Deterministic native conformance");
        config.budget = serde_json::from_value(input["budget"].clone())?;
        Ok(Self {
            script: serde_json::from_value(input["script"].clone())?,
            approve: boolean(input, "approve")?,
            cancelled: boolean(input, "cancelled")?,
            config,
            model_calls: Count::default(),
            tool_calls: Count::default(),
        })
    }

    /// The response for what the run returned; `items` and `ledger` are what it wrote back.
    fn response(
        &self,
        outcome: std::result::Result<LoopOutcome, LoopError>,
        items: &[Item],
        ledger: &RunLedger,
    ) -> Value {
        let model_calls = self.model_calls.load(Ordering::SeqCst);
        let tool_calls = self.tool_calls.load(Ordering::SeqCst);
        match outcome {
            Ok(outcome) => {
                let tokens = outcome.total_tokens();
                let failures = failed_results(&outcome.items);
                json!({"stop":outcome.stop,"text":outcome.text,"turns":outcome.turns,"model_calls":model_calls,"tool_calls":tool_calls,"failed_results":failures,"input_tokens":tokens.map(|v|v.0),"output_tokens":tokens.map(|v|v.1),"diagnostic":null})
            }
            Err(error) => {
                let tokens = ledger.total_tokens();
                let failures = failed_results(items);
                json!({"stop":null,"text":"","turns":ledger.turns,"model_calls":model_calls,"tool_calls":tool_calls,"failed_results":failures,"input_tokens":tokens.map(|v|v.0),"output_tokens":tokens.map(|v|v.1),"diagnostic":error.to_string()})
            }
        }
    }
}
fn failed_results(items: &[Item]) -> usize {
    items
        .iter()
        .filter(|item| matches!(item, Item::ToolResult { failed: true, .. }))
        .count()
}
fn run_loop(input: &Value) -> Result<Value> {
    let fixture = RunFixture::read(input)?;
    let mut model = model(fixture.script.clone(), Arc::clone(&fixture.model_calls))?;
    let mut tools = Tools::new(Arc::clone(&fixture.tool_calls))?;
    let mut approver = approver(fixture.approve);
    let mut agent = AgentLoop::new(
        &mut model,
        &mut tools,
        approver.as_mut(),
        fixture.config.clone(),
    );
    if fixture.cancelled {
        agent.cancel_handle().cancel();
    }
    let mut items = Vec::new();
    let mut ledger = RunLedger::default();
    let outcome = agent.run_in(
        &mut items,
        &mut ledger,
        "synthetic input",
        &mut VecLoopSink::new(),
    );
    Ok(fixture.response(outcome, &items, &ledger))
}

/// One `harness.loop.Call` input.
struct CallFixture {
    approve: bool,
    call: ToolCall,
    tool_calls: Count,
}
impl CallFixture {
    fn read(input: &Value) -> Result<Self> {
        Ok(Self {
            approve: boolean(input, "approve")?,
            call: ToolCall {
                call_id: CallId::new("call")?,
                name: ToolName::new(input["name"].as_str().ok_or("name must be String")?)?,
                arguments: json!({}),
            },
            tool_calls: Count::default(),
        })
    }

    fn config() -> LoopConfig {
        LoopConfig::new("fixture", "Deterministic native conformance")
    }

    fn response(&self, outcome: &ToolOutcome) -> Value {
        json!({"failed":outcome.failed,"tool_calls":self.tool_calls.load(Ordering::SeqCst),"refusal":outcome.refusal})
    }
}
fn call(input: &Value) -> Result<Value> {
    let fixture = CallFixture::read(input)?;
    let mut model = model(VecDeque::new(), Count::default())?;
    let mut tools = Tools::new(Arc::clone(&fixture.tool_calls))?;
    let mut approver = approver(fixture.approve);
    let outcome = AgentLoop::new(
        &mut model,
        &mut tools,
        approver.as_mut(),
        CallFixture::config(),
    )
    .call(&fixture.call, &mut VecLoopSink::new());
    Ok(fixture.response(&outcome))
}

#[cfg(test)]
mod tests {
    //! `LoopTask` and `AgentLoop` agree on every scenario the boundary drives through the loop
    //! (story `loop-task-awaits-a-run`).

    use std::future::Future;
    use std::path::Path;
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    use std::thread;
    use std::time::{Duration, Instant};

    use harness_loop::{LoopCancel, LoopPorts, LoopTask, VecLoopSink};
    use serde_json::Value;

    use super::{CallFixture, RunFixture, Tools, approver, execute, model};
    use crate::Result;

    /// Wakes the thread waiting in [`block_on`].
    struct Unpark(thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }

    /// Polls `future` on this thread, parking between polls, for at most five seconds.
    fn block_on<F: Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        let waker = Waker::from(Arc::new(Unpark(thread::current())));
        let mut context = Context::from_waker(&waker);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
            let now = Instant::now();
            assert!(now < deadline, "the loop task did not answer within 5 s");
            thread::park_timeout(deadline - now);
        }
    }

    /// `harness.loop.Run` through `LoopTask::run_in`; a cancelled scenario cancels through the
    /// task's own handle before the run is sent.
    fn run_through_task(input: &Value) -> Result<Value> {
        let fixture = RunFixture::read(input)?;
        let (script, approve) = (fixture.script.clone(), fixture.approve);
        let (model_calls, tool_calls) = (
            Arc::clone(&fixture.model_calls),
            Arc::clone(&fixture.tool_calls),
        );
        let task = LoopTask::spawn(fixture.config.clone(), move |_: &LoopCancel| -> Result<_> {
            Ok(LoopPorts::boxed(
                Box::new(model(script, model_calls)?),
                Box::new(Tools::new(tool_calls)?),
                approver(approve),
            ))
        });
        if fixture.cancelled {
            task.cancel_handle().cancel();
        }
        let ran = block_on(task.run_in(Vec::new(), "synthetic input", VecLoopSink::new()))?;
        block_on(task.shutdown())?;
        Ok(fixture.response(ran.result, &ran.items, &ran.spend))
    }

    /// `harness.loop.Call` through `LoopTask::call`.
    fn call_through_task(input: &Value) -> Result<Value> {
        let fixture = CallFixture::read(input)?;
        let (approve, tool_calls) = (fixture.approve, Arc::clone(&fixture.tool_calls));
        let task = LoopTask::spawn(CallFixture::config(), move |_: &LoopCancel| -> Result<_> {
            Ok(LoopPorts::boxed(
                Box::new(model(
                    std::collections::VecDeque::new(),
                    super::Count::default(),
                )?),
                Box::new(Tools::new(tool_calls)?),
                approver(approve),
            ))
        });
        let (outcome, _) = block_on(task.call(fixture.call.clone(), VecLoopSink::new()))?;
        block_on(task.shutdown())?;
        Ok(fixture.response(&outcome))
    }

    #[test]
    fn loop_task_and_agent_loop_agree_on_every_run_and_call_scenario() -> Result<()> {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/scenarios");
        let mut paths: Vec<_> = std::fs::read_dir(&directory)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<_>>()?;
        paths.sort();
        let mut compared = 0;
        for path in paths {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("scenario file name")?
                .to_owned();
            if !(name.starts_with("run-") || name.starts_with("call-")) {
                continue;
            }
            let scenario: Value = serde_yaml_ng::from_str(&std::fs::read_to_string(&path)?)?;
            for step in scenario["timeline"].as_array().ok_or("timeline")? {
                let command = step["command"].as_str().ok_or("command")?;
                let input = &step["input"];
                let through_task = match command {
                    "harness.loop.Run" => run_through_task(input)?,
                    "harness.loop.Call" => call_through_task(input)?,
                    other => return Err(format!("{name}: unexpected command {other}").into()),
                };
                assert_eq!(
                    through_task,
                    execute(command, input)?,
                    "{name}: LoopTask and AgentLoop disagree"
                );
            }
            compared += 1;
        }
        assert_eq!(compared, 17, "the run-* and call-* scenarios compared");
        Ok(())
    }
}
