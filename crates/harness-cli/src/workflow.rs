//! `b10x-harness workflow` — one loop walks a whole workflow.
//!
//! # Why the runner is here
//!
//! [`harness_flow`] can plan and walk a document and deliberately knows nothing about a model, a
//! credential or a tool: what a step *is* lives behind [`StepRunner`], and until this file existed
//! every implementation of that trait was in a test. The only way a workflow ran this loop was one
//! process per state, which meant every step started cold and a retreat paid for its context
//! again. Here a section is one warm conversation and the walk is one process.
//!
//! # What this module is, in three sentences
//!
//! [`FlowRunner`] binds a step to one `AgentLoop::run_in` over the same [`Prepared`] the `run` verb
//! builds — one client, one catalogue, one approver, one gate. A **group is a conversation**: every
//! step with the same `scope` and `attempt` continues the same items, and a new scope starts from
//! what its siblings promised and nothing else. Every step runs under a schema this module derives,
//! so a step says *passed* or *failed* by calling `answer` rather than by being read out of prose.
//!
//! # A `command` step is a call, not a turn
//!
//! A step whose `run` says `kind: command` names a program the document runs — the projection's
//! verifiers, `producer: verifier` — and no model is asked anything (design 0003 § 6, M2). The
//! argv becomes one `run` call made through [`AgentLoop::call`], which is the gate a model's call
//! meets in the order it meets it: published, the approver, the operator's `before-call` hook, the
//! tool, `after-call`. The call and its result are filed into the section's conversation exactly
//! as a model's would be, so the next step in the scope reads the suite's output where it would
//! have read a tool's; exit `0` is a passed step and anything else — a non-zero exit, a timeout, a
//! refusal at any stage — is a failed one, with the reason in the record beside it. A runner that
//! reached the tool port directly would be a second answer to *what may this run do*.
//!
//! # Where the governor plugs in
//!
//! [`StepRunner::entering`] and [`StepRunner::leaving`] ask the operator's `transition` hook — the
//! fourth point, design 0003 § 3 — and turn what it says into a [`Gate`]. **This file evaluates no
//! gate and neither does the notation**: it spawns the program the `--hooks` file named, hands it
//! the boundary as one JSON document, and carries the answer's words. With no hooks file, or a file
//! naming no `transition` hook, nothing is spawned and every boundary proceeds — a walk nobody is
//! governing, which is what it was before the point existed.
//!
//! A hook that could not answer is read as a refusal at **both** moments ([`hooks::Hooks::transition`]):
//! a section that ran because nobody could be asked is the ungoverned walk the point exists to
//! prevent.
//!
//! # `--max-cost-microunits` and `--max-duration-ms` bound the walk, not the step
//!
//! Every other bound in [`RunOptions`] is per step, because that is what one `run_in` counts:
//! `--max-turns` is turns of *this* conversation, and a flow of eight steps under `--max-turns 20`
//! offers each of them twenty. Money and wall clock are not like that — a caller who says *spend at
//! most a dollar* means the walk, and eight steps each allowed a dollar is eight dollars (design
//! 0003 § 2).
//!
//! [`AgentLoop::run_in`] cannot be handed a running total: it **replaces** the ledger it is lent
//! with what this run spent, by design, so a caller reusing one reads the last step rather than the
//! walk. So the arithmetic is here. [`FlowRunner`] keeps its own cumulative ledger and the instant
//! the walk began, and before each step derives that step's [`harness_loop::Budget`] from what
//! **remains** — `max_cost_microunits` less what has been spent, `max_duration_ms` less what has
//! elapsed — leaving the loop's own per-step enforcement to do the work unchanged. A step that
//! would start with nothing left never starts: it is `Failed`, with a `flow-budget` warning naming
//! the ceiling and the spend, and no model is asked anything.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use clap::{Args, Subcommand};
use harness_flow::{
    Flow, FlowEvent, FlowSink, Gate, Group, Handoff, Moment, Node, NodeId, Plan, Repeat, Report,
    Step, StepContext, StepOutcome, StepRunner,
};
use harness_loop::{
    AgentLoop, Budget, HookDecision, HookPoint, LoopEvent, LoopOutcome, LoopSink, LoopStop,
    RunLedger,
};
use harness_wire::{CallId, Item, ToolCall, ToolName, ToolOutcome};
use serde_json::{Map, Value};

use crate::{Prepared, Renderer, RunFailure, RunOptions, hooks, persist, transcript};

/// The two verbs a workflow has, beside `run`, `chat`, `sessions`, `tools` and the rest.
#[derive(Debug, Subcommand)]
pub(crate) enum WorkflowCommand {
    /// Validate a workflow document and print what runs in what order. Contacts nothing.
    ///
    /// Like `tools`, this answers a question about a run without starting one: *does this document
    /// validate, and what would it do?* — for free, with no endpoint and no credential.
    Plan(PlanOptions),
    /// Walk a workflow document: one turn of the loop per step, one conversation per section.
    ///
    /// Every flag `run` takes, plus the document, the task and the retreat bound. There is no
    /// `--output-schema` here: the runner derives the one every step answers under, because the
    /// walk has to read *passed* or *failed* out of it (design 0003 § 2).
    Run(Box<WorkflowRunOptions>),
}

#[derive(Debug, Args)]
pub(crate) struct PlanOptions {
    /// The document: `.yaml`, `.yml` or `.json`, decided by extension and refused by name
    /// otherwise.
    #[arg(long, value_name = "FILE")]
    flow: PathBuf,
    /// Put this bound on every section, for a document that carries none.
    ///
    /// Absent leaves the document's own `repeat` bounds alone. A projection that says what order
    /// states run in but not how many times a retreat may be taken gets its bound here, on the
    /// command line, rather than by somebody editing a generated file.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    max_attempts: Option<u32>,
    /// Print the plan as JSON instead of as lines.
    #[arg(long)]
    json: bool,
}

/// `workflow run`: everything `run` takes, plus the document and the task.
///
/// [`RunOptions`] is flattened exactly as `chat` flattens it, so a workflow is bounded, confined,
/// approved and recorded by the same flags a single run is — there is no second vocabulary for
/// running the loop.
#[derive(Debug, Args)]
pub(crate) struct WorkflowRunOptions {
    #[command(flatten)]
    pub(crate) options: RunOptions,
    /// The document: `.yaml`, `.yml` or `.json`, decided by extension and refused by name
    /// otherwise.
    #[arg(long, value_name = "FILE")]
    flow: PathBuf,
    /// The task, given to every step beside the step's own prompt.
    ///
    /// The same word `run` uses, and the same thing: what this invocation is for. A step's own
    /// prompt says what to do *here*; this says what is being worked on at all.
    #[arg(long)]
    input: String,
    /// Put this bound on every section, for a document that carries none.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    max_attempts: Option<u32>,
}

/// Runs whichever verb was asked for.
pub(crate) fn dispatch(command: &WorkflowCommand) -> ExitCode {
    match command {
        WorkflowCommand::Plan(options) => planned(options),
        WorkflowCommand::Run(command) => finished(walk(command), command.options.json),
    }
}

// ---------------------------------------------------------------------------------------------
// The document
// ---------------------------------------------------------------------------------------------

/// Reads a document, bounds its sections, and validates it — everything that can be answered
/// without an endpoint.
///
/// Both verbs go through here, so `plan` is a true dry run of `run`: a document `plan` accepts is
/// one `run` will not refuse for a reason `plan` could have named first.
///
/// # Errors
///
/// Names the file and the refusal: an extension this build does not read, bytes that are not a
/// document ([`harness_flow::ParseError`]), a document that is not a workflow
/// ([`harness_flow::FlowError`], at the path it was found), or a step payload that is not an
/// object.
fn document(path: &Path, max_attempts: Option<u32>) -> Result<(Flow, Plan), String> {
    let flow = read(path, max_attempts)?;
    let plan = flow
        .plan()
        .map_err(|error| format!("`{}`: {error}", path.display()))?;
    payloads(&flow.root, &flow.root.id)?;
    Ok((flow, plan))
}

/// Reads the document with the reader its extension names.
///
/// The extension and not a sniff: a file whose bytes happen to parse as both is a file two readers
/// would disagree about the day one of them grew a feature, and a caller who typed the wrong name
/// deserves to be told which name this build reads.
fn read(path: &Path, max_attempts: Option<u32>) -> Result<Flow, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("reading the flow `{}`: {error}", path.display()))?;
    let extension = path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_ascii_lowercase);
    let read = match extension.as_deref() {
        Some("yaml" | "yml") => Flow::from_yaml(&text),
        Some("json") => Flow::from_json(&text),
        other => {
            return Err(format!(
                "`{}` is {}, and a workflow document is `.yaml`, `.yml` or `.json`: the reader is \
                 chosen by extension so that two readers can never disagree about one file",
                path.display(),
                other.map_or_else(
                    || "named without an extension".to_owned(),
                    |extension| format!("a `.{extension}` file")
                )
            ));
        }
    };
    let mut flow = read.map_err(|error| format!("`{}`: {error}", path.display()))?;
    if let Some(max) = max_attempts {
        bound(&mut flow.root, max);
    }
    Ok(flow)
}

/// Puts one bound on every section, **the root included**.
///
/// The flag's word is *every*, and the root is a group like any other. Leaving it out was a bound
/// that skipped exactly one section — and the one that holds the steps of a flat document, so
/// `--max-attempts 3` on a projection with no sub-sections bounded nothing at all while four
/// documents said it bounded everything.
///
/// Re-entering the root does run the whole document again. That is what a caller who bounded every
/// section asked for, and it is the same rule every other section is under: a group that did not
/// come out clean goes round again while the document still allows an attempt.
fn bound(group: &mut Group, max: u32) {
    group.repeat = Some(Repeat { max });
    for node in &mut group.nodes {
        if let Node::Group(inner) = node {
            bound(inner, max);
        }
    }
}

/// Refuses a step payload this runner could not read.
///
/// The notation keeps `run` opaque, so nothing in `harness-flow` can say this. Here it is a
/// refusal before the first request rather than a step that quietly arrives with no prompt: a
/// payload that is a string names none of the keys § 2 reads, and a walk that ran it would send
/// the flow's `--input` eight times and call the result a workflow.
///
/// An **absent** payload is fine — every key is optional, and a step with none is a step whose
/// prompt is the flow's own input.
fn payloads(group: &Group, path: &str) -> Result<(), String> {
    for node in &group.nodes {
        let here = format!("{path}.{}", node.id());
        match node {
            Node::Group(inner) => payloads(inner, &here)?,
            Node::Step(step) if step.run.is_object() || step.run.is_null() => {}
            Node::Step(step) => {
                return Err(format!(
                    "`{here}` carries a `run` payload that is {}: a step's payload is an object of \
                     the keys this runner reads — `state`, `summary`, `prompt`, `context` — or \
                     nothing at all",
                    shape(&step.run)
                ));
            }
        }
    }
    Ok(())
}

/// One phrase saying what a value is, for a refusal that names the problem.
fn shape(value: &Value) -> &'static str {
    match value {
        Value::Object(_) => "an object",
        Value::Array(_) => "an array",
        Value::String(_) => "a string",
        Value::Number(_) => "a number",
        Value::Bool(_) => "a boolean",
        Value::Null => "nothing",
    }
}

// ---------------------------------------------------------------------------------------------
// `workflow plan`
// ---------------------------------------------------------------------------------------------

/// `b10x-harness workflow plan` — validate and answer, with nothing contacted.
fn planned(options: &PlanOptions) -> ExitCode {
    match plan_command(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // The same one-line record every unstartable run writes, so a driver reading the
            // stream needs no second parser for a plan that was refused.
            if options.json {
                println!("{}", crate::refused_line(&message));
            }
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

/// # Errors
///
/// Whatever [`document`] refused, unchanged.
fn plan_command(options: &PlanOptions) -> Result<(), String> {
    let (flow, plan) = document(&options.flow, options.max_attempts)?;
    if options.json {
        println!(
            "{}",
            serde_json::json!({"kind": "plan", "flow": flow.id, "plan": as_json(&plan)})
        );
        return Ok(());
    }
    let mut text = format!("{}\n", flow.id);
    lines(&plan, 1, &mut text);
    print!("{text}");
    Ok(())
}

/// One line per layer, indented per group, with the repeat bound beside the group it belongs to.
fn lines(plan: &Plan, depth: usize, into: &mut String) {
    let indent = "  ".repeat(depth);
    let bound = if plan.attempts > 1 {
        format!(" (repeat: max {})", plan.attempts)
    } else {
        String::new()
    };
    let _ = writeln!(into, "{indent}{}{bound}", plan.path);
    for (number, layer) in plan.layers.iter().enumerate() {
        let _ = writeln!(into, "{indent}  {}. {}", number + 1, layer.nodes.join(", "));
    }
    for inner in plan.groups.values() {
        lines(inner, depth + 1, into);
    }
}

/// The plan as a document, written here because [`Plan`] is the notation's type and serialising it
/// is this verb's need rather than the notation's.
fn as_json(plan: &Plan) -> Value {
    let layers: Vec<Value> = plan
        .layers
        .iter()
        .map(|layer| Value::from(layer.nodes.clone()))
        .collect();
    let groups: Map<String, Value> = plan
        .groups
        .iter()
        .map(|(id, inner)| (id.clone(), as_json(inner)))
        .collect();
    serde_json::json!({
        "path": plan.path,
        "attempts": plan.attempts,
        "layers": layers,
        "groups": groups,
    })
}

// ---------------------------------------------------------------------------------------------
// `workflow run`
// ---------------------------------------------------------------------------------------------

/// The exit status of a walk, in the three shapes a caller acts on differently.
///
/// `0` the flow came out clean; `2` it did not — a step failed, a section was skipped or exhausted,
/// or the run was cancelled; `1` it was refused before it started, or aborted on a broken wire.
/// `1` and not clap's `2` for a refusal, for the reason [`crate::stopped`] gives.
///
/// The tallies come from the [`Report`] the walk answered, which is the same object `flow-finished`
/// was built from: a line saying *three ran, one failed* and a record saying otherwise would be two
/// answers to one question.
fn finished(outcome: Result<Report, RunFailure>, json: bool) -> ExitCode {
    match outcome {
        Ok(report) if report.clean() => ExitCode::SUCCESS,
        Ok(report) => {
            eprintln!(
                "the flow did not come out clean — {} ran, {} failed, {} skipped, {} retreat(s); \
                 the record says which section did not",
                report.ran, report.failed, report.skipped, report.retreats
            );
            ExitCode::from(2)
        }
        Err(failure) => {
            if json && failure.never_started() {
                println!("{}", crate::refused_line(failure.message()));
            }
            eprintln!("error: {}", failure.message());
            ExitCode::FAILURE
        }
    }
}

/// Walks the document, and answers whether it came out clean.
///
/// # Errors
///
/// [`RunFailure::Refused`] for everything decided before the first request — a flag this verb does
/// not take, a document that does not read or does not validate, and whatever `prepare` refuses.
/// [`RunFailure::Failed`] when a step broke on the wire: a broken wire is nobody's failed step, and
/// a walk that recorded a network blip as `Failed` would misreport the plan.
fn walk(command: &WorkflowRunOptions) -> Result<Report, RunFailure> {
    let options = &command.options;
    if options.resume.is_some() {
        return Err(RunFailure::Refused(
            "`--resume` continues one conversation and a flow has one per section, each named \
             after the section it belongs to. Resuming a whole flow is its own cursor and does not \
             exist yet; start it again, or resume one of its sections with `run --resume`."
                .to_owned(),
        ));
    }
    let (flow, plan) =
        document(&command.flow, command.max_attempts).map_err(RunFailure::Refused)?;
    let mut prepared = prepared_for(&flow, options).map_err(RunFailure::Refused)?;
    crate::install_interrupt(&prepared.cancel);

    // Held behind a `RefCell` because the walk hands the runner and the sink out separately and
    // both write the same two streams: the loop's events for a step have to land between that
    // step's `step-started` and `step-finished`, which is one stream and therefore one renderer.
    let renderer = RefCell::new(
        Renderer::new(io::stdout(), io::stderr(), options.json, options.quiet).within_a_flow(),
    );
    let halted = Cell::new(false);
    let flow_run = prepared.session.id.clone();
    let hooks = prepared.hooks.take();
    let mut runner = FlowRunner {
        promises: promises(&flow.root, &flow.root.id),
        bounds: bounds(&plan),
        prepared: &mut prepared,
        options,
        flow: &flow.id,
        input: &command.input,
        flow_run,
        open: Vec::new(),
        hooks,
        spend: RunLedger::default(),
        started: Instant::now(),
        renderer: &renderer,
        halted: &halted,
        aborted: None,
        commands: 0,
    };
    let mut record = FlowRecord {
        renderer: &renderer,
        halted: &halted,
    };
    // The document already planned once, so this cannot refuse; it is mapped rather than unwrapped
    // because a panic here would be this file asserting something the notation owns.
    let report = flow
        .run(&mut runner, &mut record)
        .map_err(|error| RunFailure::Refused(error.to_string()))?;
    if let Some(error) = runner.aborted {
        return Err(RunFailure::Failed(error));
    }
    Ok(report)
}

/// The one run every step of this walk is a turn of — built the way `run` builds it, and told a
/// schema.
///
/// **The schema is not [`None`] here**, and that is the whole reason this is a function. `prepare`
/// writes the standing instruction, and the sentence *"Finish by calling `answer`…"* is written
/// only when it has been handed a schema to name. Every step of a flow runs under one this module
/// derives — so a `None` here would publish the `answer` tool and never tell the model it exists,
/// leaving it to meet the loop's nudge after a turn of prose instead of the instruction before it.
///
/// The root's schema is the one built here, because `prepare` writes one instruction for the run;
/// each step then replaces the *schema* with its own section's promises, which is the part that
/// differs section by section. The instruction naming `answer` does not.
///
/// # Errors
///
/// Whatever `prepare` refused: a credential, a workspace, a confinement, a session, a hooks file.
fn prepared_for(flow: &Flow, options: &RunOptions) -> Result<Prepared, String> {
    let opening = harness_loop::OutputSchema::new(answer_schema(&flow.root.gives))
        .expect("the derived answer schema is an object schema");
    crate::prepare(options, Some(opening))
}

/// How many attempts each section may take, by path.
///
/// Read from the [`Plan`] — which is where `repeat.max`, `--max-attempts` and the default of one
/// have already been reconciled — because a governor asked *whether* to allow a retreat must be told
/// how many are left, and this file working it out a second time from the document would be a second
/// answer to a question the plan already answered.
fn bounds(plan: &Plan) -> BTreeMap<String, u32> {
    let mut found = BTreeMap::new();
    collect_bounds(plan, &mut found);
    found
}

fn collect_bounds(plan: &Plan, into: &mut BTreeMap<String, u32>) {
    into.insert(plan.path.clone(), plan.attempts);
    for inner in plan.groups.values() {
        collect_bounds(inner, into);
    }
}

/// What each section promises its siblings, by path.
///
/// Read once, before anything runs, because it is what the derived schema offers a step: the names
/// the enclosing group declared and no others.
fn promises(group: &Group, path: &str) -> BTreeMap<String, Vec<NodeId>> {
    let mut found = BTreeMap::new();
    found.insert(path.to_owned(), group.gives.clone());
    for node in &group.nodes {
        if let Node::Group(inner) = node {
            found.extend(promises(inner, &format!("{path}.{}", inner.id)));
        }
    }
    found
}

/// The shape every step answers in, derived from the enclosing group's own `gives`.
///
/// **The model never sees a schema file.** `outcome` is the [`StepOutcome`] the walk acts on;
/// `note` is what the step wants the record to say; `gives` carries the names the group promised,
/// and nothing else — a step cannot hand over a name its own section never declared, because the
/// document is where that contract is written.
///
/// `gives` is present even for a section that promises nothing, with no properties. A key that
/// appeared only sometimes would read as a schema that forgot, and the step still has to answer
/// `outcome` either way.
fn answer_schema(gives: &[NodeId]) -> Value {
    let promised: Map<String, Value> = gives
        .iter()
        .map(|name| (name.clone(), serde_json::json!({})))
        .collect();
    serde_json::json!({
        "type": "object",
        "required": ["outcome"],
        "properties": {
            "outcome": {"enum": ["passed", "failed"]},
            "note": {"type": "string"},
            "gives": {"type": "object", "properties": promised},
        },
    })
}

// ---------------------------------------------------------------------------------------------
// The record
// ---------------------------------------------------------------------------------------------

/// Where the walk reports: the same renderer the loop's own events go to.
///
/// # Why it can stop reporting
///
/// A cancelled step and a broken wire both end the flow where it stands, and the walk has no word
/// for that — it goes on skipping and leaving sections it will never run. Emitting those would
/// claim things happened that did not, so once the runner has halted **nothing more reaches the
/// record**, `flow-finished` included (design 0003 § 2). What already ran is still filed; the exit
/// status and the loop's own last event say why there is no ending.
struct FlowRecord<'a> {
    renderer: &'a RefCell<Renderer<io::Stdout, io::Stderr>>,
    halted: &'a Cell<bool>,
}

impl FlowSink for FlowRecord<'_> {
    fn emit(&mut self, event: FlowEvent) {
        if self.halted.get() {
            return;
        }
        FlowSink::emit(&mut *self.renderer.borrow_mut(), event);
    }
}

/// A step that could not be given what its document declared, and the code the record files it
/// under.
///
/// Two codes because they are two different facts about a run. `context-refused` is a boundary
/// holding: the document named something this run may not see, or something that is not there, and
/// nobody's disk went wrong. `step-context` is a file inside the workspace that would not read —
/// a permission, a directory where a file was expected — which is a machine to go and look at.
/// One code for both would make the first look like an accident.
struct Refused {
    code: &'static str,
    reason: String,
}

impl Refused {
    /// A name the workspace does not admit.
    fn context(reason: String) -> Self {
        Self {
            code: "context-refused",
            reason,
        }
    }
}

/// The one path a `context` name may resolve to: a file inside the workspace, proved to be there.
///
/// # Errors
///
/// The whole sentence, without the step's path, which the caller puts in front: an absolute name,
/// one that does not resolve, or one that resolves outside. Each names the path **and** the
/// workspace, because *outside the workspace* is unreadable without the boundary it is outside of.
fn inside(workspace: &Path, name: &str) -> Result<PathBuf, String> {
    if Path::new(name).is_absolute() {
        return Err(format!(
            "names the context file `{name}`, which is an absolute path. A `context` entry is \
             relative to the workspace `{}` and is read from inside it; a document that could name \
             any path on the machine would be a workspace that bounds nothing",
            workspace.display()
        ));
    }
    let at = workspace.join(name);
    let resolved = at.canonicalize().map_err(|error| {
        format!(
            "names the context file `{name}`, which does not resolve inside the workspace `{}` \
             ({}): {error}",
            workspace.display(),
            at.display()
        )
    })?;
    if !resolved.starts_with(workspace) {
        return Err(format!(
            "names the context file `{name}`, which resolves to `{}` — outside the workspace `{}`. \
             A step reads what the run may see, and nothing above it",
            resolved.display(),
            workspace.display()
        ));
    }
    Ok(resolved)
}

/// One section's conversation, for one attempt of it.
///
/// A new attempt is a new one of these: [`harness_flow::Repeat`] re-runs the whole scope, so a
/// retreat starts from what crossed the boundary and not from the draft it is retreating from.
struct Scope {
    path: String,
    attempt: u32,
    session: transcript::Session,
    /// The conversation, lent to the loop one step at a time.
    items: Vec<Item>,
    /// What the steps of this section answered under `gives`, last write wins.
    gives: Handoff,
}

/// A step is a turn: [`StepRunner`] over one [`Prepared`].
///
/// One client, one catalogue, one approver and one gate for the whole walk — built once by
/// `prepare`, exactly as `run` builds them, because a section that published its own toolset would
/// be a second answer to *what may this run do* (design 0003 § 6 keeps that for M2).
struct FlowRunner<'a> {
    prepared: &'a mut Prepared,
    options: &'a RunOptions,
    /// The document's own name, as a `transition` hook is told it.
    flow: &'a str,
    /// The flow's own task, given to every step beside the step's own prompt.
    input: &'a str,
    /// What each section promises, by path.
    promises: BTreeMap<String, Vec<NodeId>>,
    /// How many attempts each section may take, by path — the `of` a governor is told.
    bounds: BTreeMap<String, u32>,
    /// The identifier every session of this walk is named under.
    flow_run: String,
    /// The sections open right now, innermost last.
    open: Vec<Scope>,
    /// The operator's hooks, or [`None`] when nobody named a file.
    ///
    /// The three points the loop asks at reach it through `with_hooks` below. The fourth —
    /// `transition`, design 0003 § 3 — is asked in [`StepRunner::entering`] and
    /// [`StepRunner::leaving`], which is why this is held here rather than left in `prepared`.
    hooks: Option<hooks::Hooks>,
    /// What the **whole walk** has spent, folded step by step.
    ///
    /// Not the ledger lent to [`AgentLoop::run_in`]: that one is replaced with one step's figures
    /// on every exit path, by design, so a running total cannot be kept in it. This is what the
    /// flow-wide ceilings are measured against (see this module's header).
    spend: RunLedger,
    /// When the walk began, for the same reason: `--max-duration-ms` bounds the walk.
    started: Instant,
    renderer: &'a RefCell<Renderer<io::Stdout, io::Stderr>>,
    /// Set when a step was cancelled or broke on the wire: the walk stops running anything.
    halted: &'a Cell<bool>,
    /// The wire failure that ended the walk, when one did. `1` and not `2`.
    aborted: Option<String>,
    /// How many `command` steps have run, which names the next one's call.
    commands: u32,
}

impl FlowRunner<'_> {
    /// The one user turn a step is given.
    ///
    /// The flow's task, where the step sits, what earlier sections established, and then the
    /// step's own prompt. **Nothing else crosses**: a sibling's transcript never reaches a step in
    /// another scope, which is the context rule the notation already states.
    ///
    /// # Errors
    ///
    /// Names a declared context file that could not be given, and the code the record files it
    /// under. The step then fails by name and the walk skips whatever needed it — a step given less
    /// context than its document declared is a step nobody can reproduce from the document.
    fn turn(&self, path: &str, step: &Step, context: &StepContext) -> Result<String, Refused> {
        let payload = step.run.as_object();
        let text = |key: &str| payload.and_then(|map| map.get(key)).and_then(Value::as_str);
        let mut turn = self.input.to_owned();
        let named = text("state").map_or_else(String::new, |state| format!(" (`{state}`)"));
        let _ = write!(
            turn,
            "\n\nYou are in step `{path}`{named}, attempt {} of section `{}`.",
            context.attempt, context.scope
        );
        if !context.available.is_empty() {
            turn.push_str("\n\nEarlier sections established:");
            for (name, value) in &context.available {
                let _ = write!(turn, "\n{name}: {}", plain(value));
            }
        }
        if let Some(prompt) = text("prompt").or_else(|| text("summary")) {
            let _ = write!(turn, "\n\n{prompt}");
        }
        let given = self.given(path, payload)?;
        if !given.is_empty() {
            turn.push_str(
                "\n\nFiles this step was given. They are already read; do not read them again:",
            );
            turn.push_str(&given);
        }
        Ok(turn)
    }

    /// The step's declared context files, read **from inside the workspace** and labelled as
    /// `--context` labels them.
    ///
    /// # A `context` name is not a path this run may follow anywhere
    ///
    /// The document is not the operator: a projection is generated by another component, and
    /// `--workspace` is the sentence saying what this run may see. `workspace.join(name)` drops the
    /// base the moment `name` is absolute, and `..` walks out of it — so a document could name
    /// `/home/…/.ssh/id_ed25519` or `../../secrets.env` and the runner would read it into a model
    /// turn, past the confinement the tools are under and past the write scope, without anything
    /// saying so. Every name is therefore resolved and **proved to be inside** the canonicalised
    /// workspace before it is read; canonicalising is also what makes a symlink out of the tree
    /// refuse rather than succeed.
    ///
    /// # Errors
    ///
    /// [`Refused`] under `context-refused` for a name outside the workspace or one that is not
    /// there, naming the path and the workspace; under `step-context` for a file that is inside and
    /// still could not be read. Absent is an error and not an empty string, for the reason
    /// `--context` gives: a run told it has a file and given nothing reports on what it did not
    /// read.
    fn given(&self, path: &str, payload: Option<&Map<String, Value>>) -> Result<String, Refused> {
        let Some(files) = payload
            .and_then(|map| map.get("context"))
            .and_then(Value::as_array)
        else {
            return Ok(String::new());
        };
        let workspace = self.options.workspace.canonicalize().map_err(|error| {
            Refused::context(format!(
                "`{path}` names context files and the workspace `{}` could not be resolved: \
                 {error}",
                self.options.workspace.display()
            ))
        })?;
        let mut given = String::new();
        for entry in files {
            let Some(name) = entry.as_str() else {
                return Err(Refused::context(format!(
                    "`{path}` names a context file that is not a path: {entry}"
                )));
            };
            let at = inside(&workspace, name)
                .map_err(|reason| Refused::context(format!("`{path}` {reason}")))?;
            let body = fs::read_to_string(&at).map_err(|error| Refused {
                code: "step-context",
                reason: format!(
                    "`{path}` names the context file `{}`, which could not be read: {error}",
                    at.display()
                ),
            })?;
            let _ = write!(given, "\n\n--- {name} ---\n{body}");
        }
        Ok(given)
    }

    /// A fresh session for one attempt of one section, named by [`session_id`] after **every**
    /// attempt above it: `<flow-run>.root.1.specify.2`.
    ///
    /// The flow-run half is [`transcript::Session::new_id`], taken once for the whole walk — the
    /// identifier `prepare` already minted for a run that, here, files nothing of its own. So the
    /// sessions of one walk sort together and name the section they hold.
    ///
    /// [`FlowRunner::open`] is read here rather than remembered anywhere, because at the moment a
    /// section is entered it *is* the ancestor chain: `entering` mints this before it pushes.
    fn session_for(&self, path: &str, attempt: u32) -> transcript::Session {
        let template = &self.prepared.session;
        let mut session = transcript::Session::new(
            template.wire.clone(),
            &template.model,
            &template.base_url,
            template.workspace.clone(),
        );
        let above: Vec<(&str, u32)> = self
            .open
            .iter()
            .map(|open| (open.path.as_str(), open.attempt))
            .collect();
        session.id = session_id(&self.flow_run, &above, path, attempt);
        session
    }

    /// Files a section's conversation, and says so on whichever stream the run is writing.
    ///
    /// A section that ran nothing has no conversation: no file is written and no line claims one
    /// was. The root of a document whose steps all live in sub-sections is exactly that, and an
    /// empty transcript beside the real ones would be a session nobody could resume into anything.
    fn file(&self, session: &transcript::Session) {
        if session.items.is_empty() {
            return;
        }
        match persist(session, self.prepared.session_dir.as_deref()) {
            Err(error) => eprintln!("warning [session] {error}"),
            Ok(None) => {}
            Ok(Some(path)) => {
                if self.options.json {
                    let _ = writeln!(
                        io::stdout(),
                        "{}",
                        serde_json::json!({
                            "kind": "session",
                            "id": session.id,
                            "path": path.display().to_string(),
                        })
                    );
                } else if !self.options.quiet {
                    eprintln!("session {} saved to {}", session.id, path.display());
                }
            }
        }
    }

    /// This step's ceilings: what the **walk** has left, not what it started with.
    ///
    /// Only the two totals the flow owns are rewritten. `max_turns`, the token bounds and the
    /// per-turn output offer are passed through untouched, because each of them bounds one
    /// conversation and a flow of eight steps is eight conversations — carving those would be a
    /// different, unasked-for feature.
    ///
    /// # Errors
    ///
    /// Names the ceiling and the spend when nothing is left. A remainder of zero is not handed to
    /// the loop: [`Budget::validate`] refuses a zero bound by name, so the step would come back as
    /// a `LoopError` — a broken run rather than a bound that bound (`AGENTS.md` invariant 11) — and
    /// it would have cost a request to learn something knowable here.
    fn ceilings(&self, path: &str) -> Result<Budget, String> {
        let mut budget = self.prepared.config.budget.clone();
        if let Some(ceiling) = budget.max_cost_microunits {
            let spent = self.spend.cost_micro_usd.unwrap_or(0);
            budget.max_cost_microunits = Some(left(ceiling, spent).ok_or_else(|| {
                format!(
                    "`--max-cost-microunits {ceiling}` is a ceiling on the whole walk and the walk \
                     has spent {spent}; `{path}` did not start"
                )
            })?);
        }
        if let Some(ceiling) = budget.max_duration_ms {
            let gone = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
            budget.max_duration_ms = Some(left(ceiling, gone).ok_or_else(|| {
                format!(
                    "`--max-duration-ms {ceiling}` is a ceiling on the whole walk and {gone} ms of \
                     it have gone; `{path}` did not start"
                )
            })?);
        }
        Ok(budget)
    }

    /// Folds one step's figures into the walk's.
    ///
    /// The same arithmetic the loop does for a delegate: absent cost stays absent, so a step nobody
    /// could price does not turn an unpriced walk into one that cost zero (`AGENTS.md` invariant 7).
    fn absorb(&mut self, step: &RunLedger) {
        self.spend.usage.extend(step.usage.iter().cloned());
        if let Some(cost) = step.cost_micro_usd {
            self.spend.cost_micro_usd =
                Some(self.spend.cost_micro_usd.unwrap_or(0).saturating_add(cost));
        }
        self.spend.turns = self.spend.turns.saturating_add(step.turns);
    }

    /// Asks the operator's `transition` hook whether this boundary may be crossed.
    ///
    /// Nothing is spawned when no hook named the point, and no [`LoopEvent::HookRan`] is recorded
    /// for a consultation that did not happen: a record showing a hook at every boundary of a run
    /// with no governor would be a gate a reader believes was there. **This is deliberately
    /// stricter than the loop**, which emits `HookRan { decision: proceed }` at each of its three
    /// points whenever *any* hooks file is attached, declared for that point or not. Widening the
    /// loop's convention is not this story's to do; narrowing this one is, and the narrower record
    /// is the one whose events all describe something that happened.
    ///
    /// Both a `Block` and a `Failed` become [`Gate::Refused`], carrying the words they came with. A
    /// hook that could not answer did not say yes, and that reading is the same at both moments.
    fn ask(
        &mut self,
        path: &str,
        moment: Moment,
        attempt: u32,
        leave: Option<(bool, &Handoff)>,
    ) -> Gate {
        if !self
            .hooks
            .as_ref()
            .is_some_and(hooks::Hooks::governs_transitions)
        {
            return Gate::Proceed;
        }
        let of = self.bounds.get(path).copied().unwrap_or(1);
        let (flow, renderer) = (self.flow, self.renderer);
        let decision = self.hooks.as_mut().map_or(HookDecision::Proceed, |hooks| {
            hooks.transition(flow, path, moment, attempt, of, leave)
        });
        LoopSink::emit(
            &mut *renderer.borrow_mut(),
            LoopEvent::HookRan {
                point: HookPoint::Transition,
                call_id: None,
                decision: decision.clone(),
            },
        );
        match decision {
            HookDecision::Proceed => Gate::Proceed,
            HookDecision::Block { reason } | HookDecision::Failed { reason } => {
                Gate::Refused { reason }
            }
        }
    }

    /// One warning on the same stream as everything else the run reports.
    fn warn(&self, code: &str, message: &str) {
        LoopSink::emit(
            &mut *self.renderer.borrow_mut(),
            LoopEvent::Warning {
                code: code.to_owned(),
                message: message.to_owned(),
            },
        );
    }

    /// Which of the open sections this step belongs to.
    ///
    /// The innermost match, which is the section the walk is standing in: a step of a group runs
    /// while no child of that group is open.
    /// Runs a `command` step: its argv as one `run` call through the run's gate, filed into the
    /// section's conversation, and read as the step's outcome.
    ///
    /// The call is named `flow-command-<n>` for the walk, because a call id is what the result is
    /// matched to in the conversation and a model never minted one for this call. The budget is
    /// the walk's remainder, exactly as a model step's is: the clock the call runs under is what
    /// `--max-duration-ms` has left.
    fn command(
        &mut self,
        path: &str,
        index: usize,
        argv: &[String],
        budget: Budget,
    ) -> StepOutcome {
        self.commands = self.commands.saturating_add(1);
        let call = ToolCall {
            call_id: CallId::new(format!("flow-command-{}", self.commands))
                .expect("a counter is printable ASCII"),
            name: ToolName::new("run").expect("`run` is a tool name"),
            arguments: serde_json::json!({ "argv": argv }),
        };
        let config = self.prepared.config.clone().with_budget(budget);
        let outcome = {
            let mut record = self.renderer.borrow_mut();
            let prepared = &mut *self.prepared;
            let mut agent = AgentLoop::new(
                prepared.client.as_mut(),
                &mut prepared.tools,
                prepared.approvals.as_mut(),
                config,
            )
            .with_cancel(prepared.cancel.clone());
            if let Some(hooks) = self.hooks.as_mut() {
                agent = agent.with_hooks(hooks);
            }
            agent.call(&call, &mut *record)
        };
        {
            // Filed as a model's call would be — the call, then its result — so the section's
            // next step reads what the program said where it would have read a tool's answer,
            // and the session shows the command that ran and what it printed.
            let scope = &mut self.open[index];
            scope.items.push(Item::ToolCall(call.clone()));
            scope
                .items
                .push(Item::result(call.call_id.clone(), outcome.clone()));
            scope.session.items.clone_from(&scope.items);
        }
        match command_outcome(&outcome) {
            Ok(()) => StepOutcome::Passed,
            Err(reason) => {
                self.warn(
                    "step-command",
                    &format!("`{path}` ran `{}`: {reason}", argv.join(" ")),
                );
                StepOutcome::Failed
            }
        }
    }

    fn scope_of(&self, context: &StepContext) -> Option<usize> {
        self.open
            .iter()
            .rposition(|open| open.path == context.scope && open.attempt == context.attempt)
    }

    /// What one finished run of the loop says the step did.
    ///
    /// The whole of design 0003 § 2's table, minus the wire failure, which never produces one of
    /// these.
    fn read(&mut self, path: &str, finished: &LoopOutcome) -> StepOutcome {
        match &finished.stop {
            LoopStop::Completed => {
                let passed = finished
                    .structured
                    .as_ref()
                    .and_then(|answer| answer.get("outcome"))
                    .and_then(Value::as_str)
                    == Some("passed");
                if passed {
                    StepOutcome::Passed
                } else {
                    StepOutcome::Failed
                }
            }
            // Not a failed step and not a failed flow: somebody stopped it. What ran is filed and
            // the walk emits no ending, because there was none.
            LoopStop::Cancelled { reason } => {
                self.warn(
                    "flow-cancelled",
                    &format!("`{path}` was cancelled ({reason}); the walk stops where it stands"),
                );
                self.halted.set(true);
                StepOutcome::Failed
            }
            // A budget that bound, or a model that ended in prose after the nudge. Both are the
            // step failing, and the reason is in the record beside this line.
            stop => {
                self.warn(
                    "step-stopped",
                    &format!("`{path}` gave no answer: {stop:?}"),
                );
                StepOutcome::Failed
            }
        }
    }
}

impl StepRunner for FlowRunner<'_> {
    fn run(&mut self, path: &str, step: &Step, context: &StepContext) -> StepOutcome {
        if self.halted.get() {
            return StepOutcome::Failed;
        }
        let Some(index) = self.scope_of(context) else {
            self.warn(
                "step-scope",
                &format!(
                    "`{path}` belongs to section `{}` and no conversation was opened for it",
                    context.scope
                ),
            );
            return StepOutcome::Failed;
        };
        // Before anything is read and before anything is sent: a walk with nothing left buys no
        // more turns, and a step that says so without having asked a model is the whole point of a
        // ceiling on the flow rather than on the step.
        let budget = match self.ceilings(path) {
            Ok(budget) => budget,
            Err(reason) => {
                self.warn("flow-budget", &reason);
                return StepOutcome::Failed;
            }
        };
        // A `command` step is the document's own program and not a question for the model: one
        // call through the run's gate, no turn bought (module header, design 0003 § 6).
        match command_of(step) {
            Ok(None) => {}
            Ok(Some(argv)) => return self.command(path, index, &argv, budget),
            Err(reason) => {
                self.warn("step-command", &format!("`{path}` {reason}"));
                return StepOutcome::Failed;
            }
        }
        // Also before anything is sent: a step whose declared context this run may not read is a
        // step nobody could reproduce from the document, and asking a model without it would be
        // answering a different question at full price.
        let turn = match self.turn(path, step, context) {
            Ok(turn) => turn,
            Err(refused) => {
                self.warn(refused.code, &refused.reason);
                return StepOutcome::Failed;
            }
        };
        let names: &[NodeId] = self.promises.get(&context.scope).map_or(&[], Vec::as_slice);
        let schema = harness_loop::OutputSchema::new(answer_schema(names))
            .expect("the derived answer schema is an object schema");
        // Cloned per step, exactly as `run` clones it: the config is the run's, and what differs
        // section by section is the shape of the answer and what the walk has left to spend.
        let config = self
            .prepared
            .config
            .clone()
            .with_output_schema(Some(schema))
            .with_budget(budget);

        let mut items = std::mem::take(&mut self.open[index].items);
        // The loop replaces the ledger it is lent with this step's own figures, so the walk's
        // running total is folded from it below rather than kept in it.
        let mut step_spend = RunLedger::default();
        let outcome = {
            let mut record = self.renderer.borrow_mut();
            let prepared = &mut *self.prepared;
            let mut agent = AgentLoop::new(
                prepared.client.as_mut(),
                &mut prepared.tools,
                prepared.approvals.as_mut(),
                config,
            )
            .with_cancel(prepared.cancel.clone());
            if let Some(hooks) = self.hooks.as_mut() {
                agent = agent.with_hooks(hooks);
            }
            agent.run_in(&mut items, &mut step_spend, turn, &mut *record)
        };
        self.absorb(&step_spend);
        let spent = step_spend;

        match outcome {
            Ok(finished) => {
                {
                    let scope = &mut self.open[index];
                    scope.session.extend(&finished);
                    scope.items = items;
                    // Last write wins: a section that answered the same name twice hands over what
                    // it ended up believing, not its first draft of it.
                    if let Some(Value::Object(given)) = finished
                        .structured
                        .as_ref()
                        .and_then(|answer| answer.get("gives"))
                    {
                        for (name, value) in given {
                            scope.gives.insert(name.clone(), value.clone());
                        }
                    }
                }
                self.read(path, &finished)
            }
            // The conversation the loop handed back is the conversation, and it is filed with what
            // it cost — the same rule `run` follows for a run that broke on turn twenty.
            Err(error) => {
                {
                    let scope = &mut self.open[index];
                    scope.session.items = items;
                    scope.session.spent(&spent);
                }
                let reason = error.to_string();
                self.warn(
                    "flow-aborted",
                    &format!("`{path}` broke on the wire: {reason}"),
                );
                self.halted.set(true);
                self.aborted = Some(reason);
                StepOutcome::Failed
            }
        }
    }

    fn handoff(&mut self, scope: &str, gives: &[NodeId]) -> Handoff {
        let Some(open) = self.open.iter().rev().find(|open| open.path == scope) else {
            return Handoff::new();
        };
        // Only what the document promised. A step that answered something its section never
        // declared has said it to its own conversation and no further: `gives` is the boundary.
        gives
            .iter()
            .filter_map(|name| {
                open.gives
                    .get(name)
                    .map(|value| (name.clone(), value.clone()))
            })
            .collect()
    }

    /// Asks the governor, and opens this attempt's conversation if it said yes.
    ///
    /// **Design 0003 § 3's `enter` moment.** The refusal returns before the conversation is opened:
    /// a section nobody allowed to run has nothing to file, and a session named after an attempt
    /// that never happened would be one nobody could resume into anything.
    ///
    /// A halted walk asks nobody. It is going nowhere, and spawning the operator's program to be
    /// told about a section that will not run would be a consultation about nothing.
    fn entering(&mut self, path: &str, attempt: u32) -> Gate {
        if self.halted.get() {
            return Gate::Proceed;
        }
        if let Gate::Refused { reason } = self.ask(path, Moment::Enter, attempt, None) {
            return Gate::Refused { reason };
        }
        let session = self.session_for(path, attempt);
        self.open.push(Scope {
            path: path.to_owned(),
            attempt,
            session,
            items: Vec::new(),
            gives: Handoff::new(),
        });
        Gate::Proceed
    }

    /// Files this attempt's conversation, then asks the governor whether its result is accepted.
    ///
    /// **Design 0003 § 3's `leave` moment**, asked after the handoff so that whoever answers has
    /// seen what the section is handing over. The section is filed **first and either way**: a
    /// refused attempt still happened and still cost what it cost, and a governor that declines a
    /// result must not be able to delete the transcript of it.
    ///
    /// Refusing an attempt that came out clean marks it failed, and the document's own `repeat`
    /// bound decides whether that becomes a retreat or the end of one — which is how a governor
    /// forces a retreat without this file knowing the word.
    fn leaving(&mut self, path: &str, attempt: u32, failed: bool, handoff: &Handoff) -> Gate {
        // Nothing to file for a section the walk halted inside, or one `entering` never opened.
        if let Some(mut scope) = self
            .open
            .pop_if(|open| open.path == path && open.attempt == attempt)
        {
            scope.session.updated_unix = crate::unix_now();
            self.file(&scope.session);
        }
        // A walk the wire broke or a person stopped asks nobody: see `entering`.
        if self.halted.get() {
            return Gate::Proceed;
        }
        self.ask(path, Moment::Leave, attempt, Some((failed, handoff)))
    }
}

/// The name of one attempt of one section: the walk's identifier, then every open scope on the way
/// down as its own name and the attempt that scope is on.
///
/// `<flow-run>.root.1.specify.2` is the second attempt of `root.specify`, run under the **first**
/// attempt of the root. Naming only the section's own attempt — `<flow-run>.root.specify.2`, which
/// is what this did until 2026-08-30 — files a re-entered ancestor's second pass over the first
/// one's: the seventh paid native walk retreated the root, ran `root.specify` attempt 1 twice, and
/// the transcript of the one whose validator exited `1` was overwritten by the one that came after
/// it. Only the event record still said it had happened.
///
/// `above` is the ancestor chain, outermost first. Each scope after the first contributes its own
/// name and not its whole path, so an id grows by two parts per level rather than by the whole path
/// again; the outermost keeps its path, which is the document's root id and one name.
///
/// # Why two chains cannot come out as one string
///
/// Every scope contributes exactly two `.`-separated parts — a name, then a decimal attempt — so a
/// chain of `d` scopes is `2d` parts and reading them back in pairs recovers the chain and the
/// attempts. That rests on a name carrying no `.` of its own, which is not an assumption:
/// `harness_flow::FlowError::DottedName` refuses one, because a name with a dot in it would already
/// make two different sections read as the same path.
fn session_id(flow_run: &str, above: &[(&str, u32)], path: &str, attempt: u32) -> String {
    let here = (path, attempt);
    let mut id = flow_run.to_owned();
    let mut enclosing: Option<&str> = None;
    for (scope, taken) in above.iter().chain(std::iter::once(&here)) {
        let name = enclosing
            .and_then(|outer| scope.strip_prefix(outer)?.strip_prefix('.'))
            .unwrap_or(scope);
        let _ = write!(id, ".{name}.{taken}");
        enclosing = Some(scope);
    }
    id
}

/// What is left of a flow-wide ceiling, or [`None`] when nothing is.
///
/// Zero is `None` and not `Some(0)` on purpose: a bound of zero admits nothing, and handing one to
/// the loop would be refused by name after the step had already been started.
fn left(ceiling: u64, spent: u64) -> Option<u64> {
    ceiling
        .checked_sub(spent)
        .filter(|remaining| *remaining > 0)
}

/// A handed-over value as one line: a string as itself, anything else as the JSON it is.
///
/// A quoted `"SPEC-1"` in a prompt reads as a literal with quotes in it, which is not what the
/// section handed over.
/// The argv of a `command` step, or [`None`] for a step that is a turn.
///
/// # Errors
///
/// A step that says `kind: command` and carries no usable `command` — absent, not a list, empty,
/// or holding something that is not a string — is an error and not a turn: a document that meant
/// to run a verifier and would instead have asked a model about it is the misreport this exists
/// to prevent.
fn command_of(step: &Step) -> Result<Option<Vec<String>>, String> {
    let payload = step.run.as_object();
    let kind = payload
        .and_then(|map| map.get("kind"))
        .and_then(Value::as_str);
    if kind != Some("command") {
        return Ok(None);
    }
    let Some(command) = payload
        .and_then(|map| map.get("command"))
        .and_then(Value::as_array)
    else {
        return Err("is a `command` step and carries no `command` list to run".to_owned());
    };
    let argv: Option<Vec<String>> = command
        .iter()
        .map(|word| word.as_str().map(str::to_owned))
        .collect();
    match argv {
        Some(argv) if !argv.is_empty() => Ok(Some(argv)),
        Some(_) => Err("is a `command` step whose `command` is empty".to_owned()),
        None => Err(
            "is a `command` step whose `command` holds something that is not a string".to_owned(),
        ),
    }
}

/// What one `run` call says the command step did: exit `0` passed, everything else did not.
///
/// A refusal at any stage of the gate, a tool that failed the call, a killed process and a non-zero
/// exit are all the step failing, each with its own words. The words are what a reader gets, so a
/// suite that went red and a program nobody approved are not the same line in the record.
///
/// # Errors
///
/// The reason the step is not passed, in one sentence.
fn command_outcome(outcome: &ToolOutcome) -> Result<(), String> {
    if let Some(refusal) = &outcome.refusal {
        return Err(format!("it was refused by rule: {}", refusal.message()));
    }
    if outcome.failed {
        return Err(format!(
            "it did not run: {}",
            outcome.output.as_str().map_or_else(
                || plain(&outcome.output),
                |text| text.chars().take(240).collect()
            )
        ));
    }
    if outcome
        .output
        .get("timed_out")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("it timed out".to_owned());
    }
    match exit_of(&outcome.output) {
        Exit::Code(0) => Ok(()),
        Exit::Code(code) => Err(format!("exit {code}")),
        Exit::Signal(signal) => Err(format!("killed by signal {signal}")),
        Exit::Unfinished(state) => Err(format!("it did not exit (state: {state})")),
        Exit::Absent => Err("it ended without an exit code".to_owned()),
    }
}

/// How a program the `run` tool ran came to an end.
#[derive(Debug, PartialEq, Eq)]
enum Exit {
    Code(i64),
    /// Substrate names a signal (`KILL`, `TERM`, `INT`); the local port has no field for one.
    Signal(String),
    /// The confined record says the execution is in a state that is not `exited` — `cancelled`,
    /// `expired` — and carries no status.
    Unfinished(String),
    Absent,
}

/// The exit, read from **either shape the `run` tool answers in**.
///
/// The local port writes `exit: <code>` (`harness-tools/local.rs`, `null` for a killed process);
/// the confined port writes `exit: <the execution record>` with `exit.exit: {code, signal}`
/// (`harness-substrate/embedded.rs` and `client.rs`, substrate's own resource, serialised). The
/// sixth paid native walk ran its validator inside the sandbox, saw it exit `0`, and read the
/// record as *no exit code* because only the first shape was looked at — a passed verifier
/// reported as a failed step, which is the misreport a command step exists to prevent.
fn exit_of(output: &Value) -> Exit {
    let read = |exit: &Value| match exit {
        Value::Number(code) => code.as_i64().map_or(Exit::Absent, Exit::Code),
        Value::Object(status) => match (
            status.get("code").and_then(Value::as_i64),
            status.get("signal").filter(|signal| !signal.is_null()),
        ) {
            (Some(code), _) => Exit::Code(code),
            (None, Some(signal)) => Exit::Signal(plain(signal)),
            (None, None) => Exit::Absent,
        },
        _ => Exit::Absent,
    };
    match output.get("exit") {
        // The confined record: the status is one level down, beside the execution's id and state.
        Some(Value::Object(record))
            if record.contains_key("state") || record.contains_key("exit") =>
        {
            match record.get("exit") {
                Some(status) if !status.is_null() => read(status),
                _ => match record.get("state").and_then(Value::as_str) {
                    Some(state) if state != "exited" => Exit::Unfinished(state.to_owned()),
                    _ => Exit::Absent,
                },
            }
        }
        Some(exit) => read(exit),
        None => Exit::Absent,
    }
}

fn plain(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_derived_schema_offers_exactly_what_the_section_promised() {
        // Design 0003 § 2, byte for byte. The model never sees a schema file, so this is the only
        // place the shape of a step's answer is written down.
        assert_eq!(
            answer_schema(&["a".to_owned(), "b".to_owned()]),
            serde_json::json!({
                "type": "object",
                "required": ["outcome"],
                "properties": {
                    "outcome": {"enum": ["passed", "failed"]},
                    "note": {"type": "string"},
                    "gives": {"type": "object", "properties": {"a": {}, "b": {}}},
                },
            })
        );
        // A section that promises nothing still answers `outcome`, and `gives` is present with no
        // properties rather than absent.
        assert_eq!(
            answer_schema(&[])["properties"]["gives"],
            serde_json::json!({"type": "object", "properties": {}})
        );
    }

    #[test]
    fn a_step_payload_that_is_not_an_object_is_refused_by_path() {
        let flow = Flow::from_yaml(
            "id: f\nroot:\n  id: root\n  nodes:\n    - id: one\n      run: \"just a string\"\n",
        )
        .expect("the document reads");
        let error = payloads(&flow.root, &flow.root.id).expect_err("refused");
        assert!(error.contains("`root.one`"), "{error}");
        assert!(error.contains("a string"), "{error}");
    }

    #[test]
    fn a_step_with_no_payload_at_all_is_accepted() {
        let flow = Flow::from_yaml("id: f\nroot:\n  id: root\n  nodes:\n    - id: one\n")
            .expect("the document reads");
        payloads(&flow.root, &flow.root.id).expect("every key is optional");
    }

    #[test]
    fn a_document_this_build_does_not_read_is_refused_by_its_extension() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join("flow.toml");
        fs::write(&path, "id = 'f'").expect("write");
        let error = read(&path, None).expect_err("refused");
        assert!(error.contains("`.toml`"), "{error}");
        assert!(error.contains(".yaml"), "and what it does read: {error}");
    }

    /// Design 0003 § 6's outcome rule for a command step, case by case: only a clean exit `0` is a
    /// passed step, and each of the other ways a call ends says which one it was.
    #[test]
    fn a_command_step_passes_on_exit_zero_and_names_every_other_ending() {
        let ran = |exit: Value, timed_out: bool| {
            ToolOutcome::ok(serde_json::json!({
                "argv": ["cargo", "test"],
                "exit": exit,
                "stdout": "",
                "stderr": "",
                "timed_out": timed_out,
            }))
        };
        assert_eq!(command_outcome(&ran(serde_json::json!(0), false)), Ok(()));
        assert_eq!(
            command_outcome(&ran(serde_json::json!(101), false)),
            Err("exit 101".to_owned())
        );
        assert_eq!(
            command_outcome(&ran(Value::Null, false)),
            Err("it ended without an exit code".to_owned())
        );
        assert_eq!(
            command_outcome(&ran(Value::Null, true)),
            Err("it timed out".to_owned())
        );
        assert_eq!(
            command_outcome(&ToolOutcome::failed("`run` was not approved: the ceiling")),
            Err("it did not run: `run` was not approved: the ceiling".to_owned())
        );
    }

    /// The confined port answers with substrate's execution record under `exit`, and the status
    /// is one level down. The sixth paid native walk read a validator that exited `0` in the
    /// sandbox as *no exit code*; this is the shape it saw, byte for byte where it matters.
    #[test]
    fn a_command_step_reads_its_exit_from_the_confined_record_as_well_as_the_local_one() {
        let confined = |code: Value, signal: Value| {
            ToolOutcome::ok(serde_json::json!({
                "stdout": "1 file(s) in .engineering/planning: 1 artifact(s)\nvalid\n",
                "stderr": "",
                "stdout_truncated": false,
                "stderr_truncated": false,
                "output_complete": true,
                "exit": {
                    "id": "ex_85092_16",
                    "kind": "exec",
                    "state": "exited",
                    "workspace": "ws_project",
                    "exit": {"code": code, "signal": signal},
                },
                "slice": {"stream": "stdout", "eof": true},
            }))
        };
        assert_eq!(
            command_outcome(&confined(serde_json::json!(0), Value::Null)),
            Ok(())
        );
        assert_eq!(
            command_outcome(&confined(serde_json::json!(2), Value::Null)),
            Err("exit 2".to_owned())
        );
        assert_eq!(
            command_outcome(&confined(Value::Null, serde_json::json!("KILL"))),
            Err("killed by signal KILL".to_owned())
        );
        assert_eq!(
            command_outcome(&confined(Value::Null, Value::Null)),
            Err("it ended without an exit code".to_owned())
        );
        // A record with no status is not read as a success, and one whose execution never exited
        // says which state it is in.
        assert_eq!(
            command_outcome(&ToolOutcome::ok(
                serde_json::json!({"exit": {"id": "ex_1"}})
            )),
            Err("it ended without an exit code".to_owned())
        );
        assert_eq!(
            command_outcome(&ToolOutcome::ok(serde_json::json!({
                "exit": {"id": "ex_1", "state": "expired", "exit": null}
            }))),
            Err("it did not exit (state: expired)".to_owned())
        );
    }

    /// Which steps are commands, and what a document that meant one and wrote it wrong is told.
    #[test]
    fn a_command_step_is_read_from_its_kind_and_a_malformed_one_is_an_error_not_a_turn() {
        let step = |run: Value| Step {
            id: "check".to_owned(),
            needs: Vec::new(),
            run,
        };
        assert_eq!(
            command_of(&step(
                serde_json::json!({"state": "verify", "prompt": "Run it."})
            )),
            Ok(None)
        );
        assert_eq!(
            command_of(&step(serde_json::json!({"state": "verify"}))),
            Ok(None),
            "a step with no kind is a turn, as every step was"
        );
        assert_eq!(
            command_of(&step(serde_json::json!({
                "state": "verify", "kind": "command", "command": ["cargo", "test", "--workspace"]
            }))),
            Ok(Some(vec![
                "cargo".to_owned(),
                "test".to_owned(),
                "--workspace".to_owned()
            ]))
        );
        assert!(
            command_of(&step(serde_json::json!({"kind": "command"})))
                .is_err_and(|reason| reason.contains("no `command` list")),
        );
        assert!(
            command_of(&step(serde_json::json!({"kind": "command", "command": []})))
                .is_err_and(|reason| reason.contains("is empty")),
        );
        assert!(
            command_of(&step(
                serde_json::json!({"kind": "command", "command": ["sh", 1]})
            ))
            .is_err_and(|reason| reason.contains("not a string")),
        );
    }

    #[test]
    fn a_bound_from_the_command_line_reaches_every_section_including_the_root() {
        let mut flow = Flow::from_yaml(
            "id: f\nroot:\n  id: root\n  repeat: {max: 5}\n  nodes:\n    - id: inner\n      \
             repeat: {max: 7}\n      nodes:\n        - id: one\n",
        )
        .expect("the document reads");
        bound(&mut flow.root, 2);
        assert_eq!(
            flow.root.repeat,
            Some(Repeat { max: 2 }),
            "the root is a section too, and the flag says every one of them"
        );
        let Node::Group(inner) = &flow.root.nodes[0] else {
            panic!("the first node is a group");
        };
        assert_eq!(
            inner.repeat,
            Some(Repeat { max: 2 }),
            "the document's 7 is overridden"
        );
    }

    #[test]
    fn a_flow_step_is_told_to_finish_by_calling_the_answer_tool() {
        // Every step runs under a derived schema, so every step has to be told how to answer under
        // it. The instruction is written once, by `prepare`, out of the schema it is handed — and a
        // walk that handed it none published `answer` and never named it.
        let dir = tempfile::tempdir().expect("a temporary directory");
        let workspace = dir.path().canonicalize().expect("canonical");
        // No credential is named, which is its own declaration: nothing here is contacted, and a
        // test that wrote a key into its own environment would be a test that leaks the habit.
        let Ok(crate::Cli {
            command: crate::Command::Workflow(WorkflowCommand::Run(command)),
        }) = <crate::Cli as clap::Parser>::try_parse_from([
            "b10x-harness",
            "workflow",
            "run",
            "--base-url",
            "https://gw.example/v1",
            "--model",
            "b10x-emulated",
            "--workspace",
            workspace.to_str().expect("utf-8 path"),
            "--no-session",
            "--flow",
            "flow.yaml",
            "--input",
            "add a CSV export",
        ])
        else {
            panic!("the workflow run subcommand parses to its own options");
        };
        let flow = Flow::from_yaml("id: f\nroot:\n  id: root\n  nodes:\n    - id: one\n")
            .expect("the document reads");

        let prepared = prepared_for(&flow, &command.options).expect("nothing here is contacted");
        assert!(
            prepared.config.instructions.contains("Finish by calling `"),
            "the standing instruction says how a step ends: {}",
            prepared.config.instructions
        );
        assert!(
            prepared
                .config
                .instructions
                .contains(harness_loop::DEFAULT_ANSWER_NAME),
            "and names the tool the derived schema publishes: {}",
            prepared.config.instructions
        );
    }

    /// The naming rule of design 0003 § 4, as it is after the seventh paid native walk lost a
    /// transcript to it: the section's own attempt is the last part, and every attempt above it is
    /// in front of that.
    #[test]
    fn a_sections_session_names_every_attempt_above_it_and_not_only_its_own() {
        assert_eq!(
            session_id("walk", &[], "root", 1),
            "walk.root.1",
            "the root under nothing is its path and its attempt"
        );
        assert_eq!(
            session_id("walk", &[("root", 1)], "root.specify", 2),
            "walk.root.1.specify.2",
            "a section carries the attempt of the scope it is running under"
        );
        assert_eq!(
            session_id(
                "walk",
                &[("root", 2), ("root.implement-to-review", 3)],
                "root.implement-to-review.verify",
                1
            ),
            "walk.root.2.implement-to-review.3.verify.1",
            "and of every scope above that, outermost first, each by its own name"
        );
    }

    /// The bug the story was filed for, as arithmetic on the names alone: a root that retreats runs
    /// the sections under it a second time, from attempt 1 again, and the two passes must not be
    /// one file.
    #[test]
    fn no_two_sections_of_one_walk_can_share_a_session_name() {
        // Every (scope, attempt) a walk of two root attempts opens, in the order it opens them,
        // named the way this file names them and the way it used to.
        let mut named: Vec<String> = Vec::new();
        let mut by_own_attempt: Vec<String> = Vec::new();
        for root in 1..=2 {
            named.push(session_id("walk", &[], "root", root));
            by_own_attempt.push(format!("walk.root.{root}"));
            named.push(session_id("walk", &[("root", root)], "root.shape", 1));
            by_own_attempt.push("walk.root.shape.1".to_owned());
            named.push(session_id(
                "walk",
                &[("root", root), ("root.shape", 1)],
                "root.shape.specify",
                1,
            ));
            by_own_attempt.push("walk.root.shape.specify.1".to_owned());
        }
        let unique: std::collections::BTreeSet<&String> = named.iter().collect();
        assert_eq!(
            unique.len(),
            6,
            "one file per (scope, attempt) at every level: {named:?}"
        );
        assert_eq!(unique.len(), named.len(), "and no two of them alike");
        // What this replaced, on the same walk: the section's own attempt and nothing above it,
        // which is one name for both passes of everything under the re-entered root.
        let survived: std::collections::BTreeSet<&String> = by_own_attempt.iter().collect();
        assert_eq!(
            survived.len(),
            4,
            "six sections ran and four files were left: {survived:?}"
        );
    }

    #[test]
    fn a_handed_over_string_reaches_a_prompt_without_its_quotes() {
        assert_eq!(plain(&serde_json::json!("SPEC-1")), "SPEC-1");
        assert_eq!(plain(&serde_json::json!({"id": 1})), "{\"id\":1}");
    }
}
