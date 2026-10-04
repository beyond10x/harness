//! Calls existing production APIs; only upstream ports are scripted.
use std::collections::VecDeque;

use harness_flow::{Flow, Gate, Step, StepContext, StepOutcome, StepRunner, VecFlowSink};
use harness_loop::{
    AgentLoop, ApprovalPort, ApproveAll, Budget, DenyAll, LoopConfig, RunLedger, VecLoopSink,
};
use harness_wire::{
    Approval, CallId, Envelope, Item, ModelPort, Risk, StreamSink, ToolCall, ToolName, ToolOutcome,
    ToolPort, ToolSpec, TurnOutcome, TurnRequest, WireError, WireId,
};
use serde_json::{Value, json};

use crate::Result;

struct Model {
    wire: WireId,
    script: VecDeque<TurnOutcome>,
    calls: u64,
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
        self.calls += 1;
        self.script
            .pop_front()
            .ok_or_else(|| WireError::protocol("fixture model exhausted"))
    }
}

struct Tools {
    specs: Vec<ToolSpec>,
    calls: u64,
}
impl Tools {
    fn new() -> Result<Self> {
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
        Ok(Self { specs, calls: 0 })
    }
}
impl ToolPort for Tools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }
    fn call(&mut self, _: &ToolCall) -> ToolOutcome {
        self.calls += 1;
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
        "harness.flow.Run" => run_flow(input),
        _ => Err(format!("no production adapter for {command}").into()),
    }
}
fn boolean(value: &Value, field: &str) -> Result<bool> {
    value[field]
        .as_bool()
        .ok_or_else(|| format!("{field} must be Boolean").into())
}
fn model(script: VecDeque<TurnOutcome>) -> Result<Model> {
    Ok(Model {
        wire: WireId::new("fixture")?,
        script,
        calls: 0,
    })
}
fn run_loop(input: &Value) -> Result<Value> {
    let mut model = model(serde_json::from_value(input["script"].clone())?)?;
    let mut tools = Tools::new()?;
    let mut yes = ApproveAll;
    let mut no = DenyAll;
    let approver: &mut dyn ApprovalPort = if boolean(input, "approve")? {
        &mut yes
    } else {
        &mut no
    };
    let mut config = LoopConfig::new("fixture", "Deterministic native conformance");
    config.budget = serde_json::from_value(input["budget"].clone())?;
    let mut agent = AgentLoop::new(&mut model, &mut tools, approver, config);
    if boolean(input, "cancelled")? {
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
    match outcome {
        Ok(outcome) => {
            let tokens = outcome.total_tokens();
            let failures = outcome
                .items
                .iter()
                .filter(|item| matches!(item, Item::ToolResult { failed: true, .. }))
                .count();
            Ok(
                json!({"stop":outcome.stop,"text":outcome.text,"turns":outcome.turns,"model_calls":model.calls,"tool_calls":tools.calls,"failed_results":failures,"input_tokens":tokens.map(|v|v.0),"output_tokens":tokens.map(|v|v.1),"diagnostic":null}),
            )
        }
        Err(error) => {
            let tokens = ledger.total_tokens();
            let failures = items
                .iter()
                .filter(|item| matches!(item, Item::ToolResult { failed: true, .. }))
                .count();
            Ok(
                json!({"stop":null,"text":"","turns":ledger.turns,"model_calls":model.calls,"tool_calls":tools.calls,"failed_results":failures,"input_tokens":tokens.map(|v|v.0),"output_tokens":tokens.map(|v|v.1),"diagnostic":error.to_string()}),
            )
        }
    }
}
fn call(input: &Value) -> Result<Value> {
    let mut model = model(VecDeque::new())?;
    let mut tools = Tools::new()?;
    let mut yes = ApproveAll;
    let mut no = DenyAll;
    let approver: &mut dyn ApprovalPort = if boolean(input, "approve")? {
        &mut yes
    } else {
        &mut no
    };
    let config = LoopConfig::new("fixture", "Deterministic native conformance");
    let call = ToolCall {
        call_id: CallId::new("call")?,
        name: ToolName::new(input["name"].as_str().ok_or("name must be String")?)?,
        arguments: json!({}),
    };
    let outcome = AgentLoop::new(&mut model, &mut tools, approver, config)
        .call(&call, &mut VecLoopSink::new());
    Ok(json!({"failed":outcome.failed,"tool_calls":tools.calls,"refusal":outcome.refusal}))
}

struct Steps {
    script: VecDeque<String>,
    paths: Vec<String>,
    refuse_entry: bool,
}
impl StepRunner for Steps {
    fn run(&mut self, path: &str, _: &Step, _: &StepContext) -> StepOutcome {
        self.paths.push(path.into());
        match self.script.pop_front().as_deref() {
            Some("paused") => StepOutcome::Paused {
                reason: "operator fixture".into(),
            },
            Some("passed") => StepOutcome::Passed,
            _ => StepOutcome::Failed,
        }
    }
    fn entering(&mut self, _: &str, _: u32) -> Gate {
        if self.refuse_entry {
            Gate::Refused {
                reason: "entry fixture".into(),
            }
        } else {
            Gate::Proceed
        }
    }
}
fn run_flow(input: &Value) -> Result<Value> {
    let mut steps = Steps {
        script: serde_json::from_value(input["script"].clone())?,
        paths: Vec::new(),
        refuse_entry: boolean(input, "refuse_entry")?,
    };
    let outcome = Flow::from_json(&input["document"].to_string())
        .map_err(|e| e.to_string())
        .and_then(|flow| {
            flow.run(&mut steps, &mut VecFlowSink::new())
                .map_err(|e| e.to_string())
        });
    match outcome {
        Ok(report) => Ok(
            json!({"status":format!("{:?}",report.status()),"reached":report.reached,"ran":report.ran,"failed":report.failed,"skipped":report.skipped,"retreats":report.retreats,"paths":steps.paths,"diagnostic":null}),
        ),
        Err(error) => Ok(
            json!({"status":null,"reached":0,"ran":0,"failed":0,"skipped":0,"retreats":0,"paths":steps.paths,"diagnostic":error}),
        ),
    }
}
