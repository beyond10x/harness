//! `b10x-harness workflow` — one loop walks a whole workflow.
//!
//! # What is here and what is not
//!
//! The walk itself — a step is a turn, a section is a conversation, a `command` step is one gated
//! `run` call, a node's `scope` narrows the catalogue for one step, the cost and duration ceilings
//! bound the walk — is [`harness_workflow::Walk`], a library a caller can drive with no process.
//! This module is what is **process- and terminal-shaped** about it, and nothing else:
//!
//! - the two verbs and their flags, the document read from a file by its extension, and
//!   `--max-attempts` laid over every section;
//! - the run `prepare` builds, exactly as `run` builds it, lent to the walk port by port;
//! - the operator's `--hooks` file, which is the walk's [`harness_workflow::Governor`] at the
//!   `transition` point — **this file evaluates no gate**: [`hooks::Hooks::transition`] spawns the
//!   program the file named and the walk carries its words. With no hooks file, or one naming no
//!   `transition` hook, no governor is attached and every boundary proceeds;
//! - the session files, one per attempt of each section, which are the walk's
//!   [`harness_workflow::StepRecord`];
//! - the renderer both event streams are written to, and the exit status.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Subcommand};
use harness_flow::{Flow, FlowEvent, FlowSink, FlowStatus, Group, Node, Plan, Repeat, Report};
use harness_loop::{HookDecision, LoopEvent, LoopOutcome, LoopSink, RunLedger};
use harness_wire::Item;
use harness_workflow::{Governor, Section, StepRecord, Transition, Walk, WalkError};
use serde_json::{Map, Value};

use crate::{
    Prepared, Renderer, RunFailure, RunOptions, apply_profiles, hooks, persist, transcript,
};

/// The two verbs a workflow has, beside `run`, `chat`, `sessions`, `tools` and the rest.
#[derive(Debug, Clone, Subcommand)]
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

#[derive(Debug, Clone, Args)]
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
#[derive(Debug, Clone, Args)]
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
        WorkflowCommand::Run(command) => {
            let mut command = command.as_ref().clone();
            let outcome = match apply_profiles(&mut command.options) {
                Err(refusal) => Err(RunFailure::Refused(refusal)),
                Ok(profiles) => {
                    command.options.applied_profiles = profiles;
                    walk(&command)
                }
            };
            finished(outcome, command.options.json)
        }
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
    harness_workflow::check_payloads(&flow)?;
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
        Ok(report)
            if matches!(
                report.status(),
                FlowStatus::Completed | FlowStatus::AwaitingOperator
            ) =>
        {
            ExitCode::SUCCESS
        }
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
    let (flow, _) = document(&command.flow, command.max_attempts).map_err(RunFailure::Refused)?;
    let prepared = prepared_for(&flow, options).map_err(RunFailure::Refused)?;
    crate::install_interrupt(&prepared.cancel);

    // Held behind a `RefCell` because the walk hands its two sinks out separately and both write
    // the same two streams: the loop's events for a step have to land between that step's
    // `step-started` and `step-finished`, which is one stream and therefore one renderer.
    let renderer = RefCell::new(
        Renderer::new(io::stdout(), io::stderr(), options.json, options.quiet).within_a_flow(),
    );
    let Prepared {
        mut client,
        mut tools,
        mut approvals,
        config,
        cancel,
        session,
        session_dir,
        mut hooks,
    } = prepared;
    // The same hooks file at the fourth point. A copy and not a second reading: `Hooks` holds what
    // the file declared and nothing a call changes, so the loop's three points and the walk's
    // boundaries are governed by one document. Attached only when the file names a `transition`
    // hook, so a run with none records no consultation that did not happen.
    let mut governor = hooks
        .as_ref()
        .filter(|hooks| hooks.governs_transitions())
        .cloned();
    let mut sessions = Sessions {
        flow_run: session.id.clone(),
        template: &session,
        dir: session_dir.as_deref(),
        json: options.json,
        quiet: options.quiet,
        open: Vec::new(),
    };
    let mut walk = Walk::new(client.as_mut(), &mut tools, approvals.as_mut(), config)
        .with_cancel(cancel)
        .with_record(&mut sessions)
        .with_workspace(&options.workspace);
    if let Some(hooks) = hooks.as_mut() {
        walk = walk.with_hooks(hooks);
    }
    if let Some(governor) = governor.as_mut() {
        walk = walk.with_governor(governor);
    }
    walk.run(
        &flow,
        &command.input,
        &mut Rendered(&renderer),
        &mut Rendered(&renderer),
    )
    .map_err(|error| match error {
        WalkError::Refused(reason) => RunFailure::Refused(reason),
        WalkError::Aborted(reason) => RunFailure::Failed(reason),
    })
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
    let opening =
        harness_loop::OutputSchema::new(harness_workflow::answer_schema(&flow.root.gives))
            .expect("the derived answer schema is an object schema");
    crate::prepare(options, Some(opening))
}

// ---------------------------------------------------------------------------------------------
// What the walk is lent
// ---------------------------------------------------------------------------------------------

/// The published toolset as the walk narrows it: whatever the surface, the scope lives on the one
/// catalogue behind it, so a walk narrows the same boundary under `--surface flat` and
/// `--surface verbs`.
impl harness_workflow::StepTools for crate::Published {
    fn catalogue(&self) -> &harness_catalogue::Catalogue {
        crate::Published::catalogue(self)
    }

    fn catalogue_mut(&mut self) -> &mut harness_catalogue::Catalogue {
        crate::Published::catalogue_mut(self)
    }
}

/// The operator's `transition` hook as the walk's governor (design 0003 § 3).
///
/// A hook that could not answer is read as a refusal at **both** moments
/// ([`hooks::Hooks::transition`]): a section that ran because nobody could be asked is the
/// ungoverned walk the point exists to prevent.
impl Governor for hooks::Hooks {
    fn transition(&mut self, transition: &Transition<'_>) -> HookDecision {
        hooks::Hooks::transition(
            self,
            transition.flow,
            transition.path,
            transition.moment,
            transition.attempt,
            transition.of,
            transition.leave,
        )
    }
}

/// Both of the walk's event streams, written to the one renderer.
///
/// # Why the flow's stream can stop
///
/// A cancelled step and a broken wire both end the flow where it stands; the walk then lets
/// nothing more through to its flow sink, `flow-finished` included (design 0003 § 2), so this
/// renders exactly what reaches it. What already ran is still filed; the exit status and the
/// loop's own last event say why there is no ending.
struct Rendered<'a>(&'a RefCell<Renderer<io::Stdout, io::Stderr>>);

impl FlowSink for Rendered<'_> {
    fn emit(&mut self, event: FlowEvent) {
        FlowSink::emit(&mut *self.0.borrow_mut(), event);
    }
}

impl LoopSink for Rendered<'_> {
    fn emit(&mut self, event: LoopEvent) {
        LoopSink::emit(&mut *self.0.borrow_mut(), event);
    }
}

/// One session file per attempt of each section: the walk's [`StepRecord`].
///
/// Each is named by [`session_id`] after **every** attempt above it, `<flow-run>.root.1.specify.2`.
/// The flow-run half is the identifier `prepare` already minted for a run that, here, files
/// nothing of its own, so the sessions of one walk sort together and name the section they hold.
struct Sessions<'a> {
    /// The run's own session, which every section's is a fresh copy of: wire, model, endpoint and
    /// workspace.
    template: &'a transcript::Session,
    /// The identifier every session of this walk is named under.
    flow_run: String,
    /// Where they are written, or [`None`] under `--no-session`.
    dir: Option<&'a Path>,
    json: bool,
    quiet: bool,
    /// The sections open right now, innermost last, beside the attempt each is on.
    open: Vec<(String, u32, transcript::Session)>,
}

impl Sessions<'_> {
    fn of(&mut self, section: Section<'_>) -> Option<&mut transcript::Session> {
        self.open
            .iter_mut()
            .rev()
            .find(|(path, attempt, _)| path == section.path && *attempt == section.attempt)
            .map(|(_, _, session)| session)
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
        match persist(session, self.dir) {
            Err(error) => eprintln!("warning [session] {error}"),
            Ok(None) => {}
            Ok(Some(path)) => {
                if self.json {
                    let _ = writeln!(
                        io::stdout(),
                        "{}",
                        serde_json::json!({
                            "kind": "session",
                            "id": session.id,
                            "path": path.display().to_string(),
                        })
                    );
                } else if !self.quiet {
                    eprintln!("session {} saved to {}", session.id, path.display());
                }
            }
        }
    }
}

impl StepRecord for Sessions<'_> {
    fn opened(&mut self, section: Section<'_>, above: &[Section<'_>]) {
        let template = self.template;
        let mut session = transcript::Session::new(
            template.wire.clone(),
            &template.model,
            &template.base_url,
            template.workspace.clone(),
        );
        let above: Vec<(&str, u32)> = above.iter().map(|open| (open.path, open.attempt)).collect();
        session.id = session_id(&self.flow_run, &above, section.path, section.attempt);
        self.open
            .push((section.path.to_owned(), section.attempt, session));
    }

    fn answered(&mut self, section: Section<'_>, outcome: &LoopOutcome) {
        if let Some(session) = self.of(section) {
            session.extend(outcome);
        }
    }

    fn called(&mut self, section: Section<'_>, items: &[Item]) {
        if let Some(session) = self.of(section) {
            session.items = items.to_vec();
        }
    }

    // The conversation the loop handed back is the conversation, and it is filed with what it
    // cost — the same rule `run` follows for a run that broke on turn twenty.
    fn broke(&mut self, section: Section<'_>, items: &[Item], spent: &RunLedger) {
        if let Some(session) = self.of(section) {
            session.items = items.to_vec();
            session.spent(spent);
        }
    }

    fn closed(&mut self, section: Section<'_>) {
        if let Some((_, _, mut session)) = self
            .open
            .pop_if(|(path, attempt, _)| path == section.path && *attempt == section.attempt)
        {
            session.updated_unix = crate::unix_now();
            self.file(&session);
        }
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

#[cfg(test)]
mod tests {
    use harness_flow::Step;
    use harness_wire::{CallId, ToolCall, ToolName, ToolOutcome};
    use harness_workflow::declared_scope;

    use super::*;

    /// A tool's answer as one line: a string as itself, anything else as the JSON it is.
    fn plain(value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        }
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

    // --- a step runs under the scope its own node declares ---------------------------------

    /// One node, with whatever `run:` payload the case is about.
    fn a_node(run: Value) -> Step {
        Step {
            id: "implement-1".to_owned(),
            needs: Vec::new(),
            run,
        }
    }

    /// The toolset a walk publishes, through the same `published()` path `prepare` uses.
    ///
    /// `unconfined` and not `new` only so that the two writing entries exist to be refused: the
    /// shipped toolset is read-only until a confinement provides one, and a test that stood up a
    /// substrate daemon to watch a rule refuse would be measuring the daemon.
    fn a_toolset(dir: &Path, run: &[&str]) -> crate::Published {
        let toolchain = harness_substrate::Toolchain::default();
        let declared: Vec<String> = run.iter().map(|rule| (*rule).to_owned()).collect();
        let process_workspace_access = harness_substrate::process_workspace_access(&[])
            .expect("an empty process write scope is read-only");
        crate::published(
            harness_catalogue::LocalOperations::unconfined(dir, Vec::new())
                .expect("the workspace opens"),
            None,
            crate::Surface::Flat,
            &crate::Confinement {
                substrate: None,
                embedded: false,
                cgroup_root: None,
                workspace_id: "ws_test",
                programs: &[],
                toolchain: &toolchain,
                process_workspace_access,
                scope: crate::write_scope(&declared).expect("the run's own rules read"),
            },
        )
        .expect("nothing is confined here, so nothing can refuse")
        .tools
    }

    /// The catalogue narrowed to what this node declares, exactly as the walk does it.
    fn under(tools: &mut crate::Published, node: &Step) {
        tools
            .catalogue_mut()
            .narrow(declared_scope(node).expect("the node's rules read"));
    }

    /// One whole-file write through the published surface, as a model's call reaches it.
    fn writing(tools: &mut crate::Published, path: &str) -> ToolOutcome {
        use harness_wire::ToolPort as _;
        tools.call(&ToolCall {
            call_id: CallId::new("flow-write-1").expect("a call id"),
            name: ToolName::new("file_write").expect("`file_write` is a tool name"),
            arguments: serde_json::json!({"path": path, "text": "revision: 99\n"}),
        })
    }

    /// A workspace with the store directory the eval's map denies writes to.
    fn a_workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temporary directory");
        fs::create_dir_all(dir.path().join(".engineering/planning/specification"))
            .expect("the store directory");
        dir
    }

    const SPECIFICATION: &str = ".engineering/planning/specification/passkey-login.md";

    #[test]
    fn a_step_whose_node_denies_a_path_cannot_write_it_and_nothing_reaches_disk() {
        // The eighth paid native walk, in one test. The map's deliberate-denial step denies
        // `.engineering/**` precisely so a refusal can be observed; the run's own scope allows
        // everything, and until the node was read `revision: 99` reached the store on disk and
        // only the store's after-the-fact validator caught it.
        let dir = a_workspace();
        let mut tools = a_toolset(dir.path(), &["**=allowed"]);
        under(
            &mut tools,
            &a_node(serde_json::json!({
                "state": "implement",
                "scope": [".engineering/**=denied", "**=allowed"],
            })),
        );

        let refused = writing(&mut tools, SPECIFICATION);

        assert!(
            refused.failed,
            "the call is refused, and as an outcome the model is told about (invariant 9)"
        );
        let said = plain(&refused.output);
        assert!(said.contains(SPECIFICATION), "{said}");
        assert!(
            !dir.path().join(SPECIFICATION).exists(),
            "refused in the tool layer, before the write: the provider was never reached"
        );
    }

    #[test]
    fn a_node_that_declares_no_scope_runs_under_the_runs_own_and_narrows_nothing() {
        // A document that says nothing does not silently narrow a run. Both halves are the point:
        // what the run denied is still denied, and what it never mentioned is still writable.
        let dir = a_workspace();
        let mut tools = a_toolset(dir.path(), &["target/**=denied"]);
        let silent = a_node(serde_json::json!({"state": "implement", "prompt": "carry on"}));
        assert!(
            declared_scope(&silent)
                .expect("a node with no scope reads")
                .is_empty(),
            "no key is no scope, not an empty one that restricts nothing new"
        );
        under(&mut tools, &silent);

        assert!(
            !writing(&mut tools, SPECIFICATION).failed,
            "the run allowed it"
        );
        assert!(
            writing(&mut tools, "target/debug/x").failed,
            "and the run's own rule still binds"
        );
    }

    #[test]
    fn the_first_rule_a_node_wrote_decides_so_re_ordering_the_list_changes_what_it_means() {
        // First match wins, in the order the map wrote it. The projection emits the list unsorted
        // for exactly this reason, and nothing on this side may sort it either.
        let dir = a_workspace();
        let deny_first = [".engineering/**=denied", "**=allowed"];
        let allow_first = ["**=allowed", ".engineering/**=denied"];

        let mut tools = a_toolset(dir.path(), &["**=allowed"]);
        under(
            &mut tools,
            &a_node(serde_json::json!({"scope": deny_first})),
        );
        assert!(
            writing(&mut tools, SPECIFICATION).failed,
            "the denial is first, so it decides"
        );

        under(
            &mut tools,
            &a_node(serde_json::json!({"scope": allow_first})),
        );
        assert!(
            !writing(&mut tools, SPECIFICATION).failed,
            "the same two rules, the other way round, mean the other thing"
        );
    }

    #[test]
    fn a_node_cannot_give_back_what_the_runs_own_scope_took_away() {
        // A projection is generated by another component and the command line is the operator's
        // own sentence. A node that could re-permit what `--write-scope` denied would make the
        // flag a suggestion, so narrowing is structural: both layers are asked and the first
        // refusal wins.
        let dir = a_workspace();
        let mut tools = a_toolset(dir.path(), &[".engineering/**=denied"]);
        under(
            &mut tools,
            &a_node(serde_json::json!({"scope": [".engineering/**=allowed", "**=allowed"]})),
        );

        let refused = writing(&mut tools, SPECIFICATION);

        assert!(refused.failed, "the run's denial stands");
        assert!(
            !dir.path().join(SPECIFICATION).exists(),
            "and nothing reached disk"
        );
    }

    #[test]
    fn a_narrowing_lasts_one_step_and_the_next_starts_under_the_runs_scope_again() {
        // What `StepRunner::run` puts back however the step ended. A narrowing that outlived its
        // step would be one node's rule silently governing its siblings, which no document says.
        let dir = a_workspace();
        let mut tools = a_toolset(dir.path(), &["**=allowed"]);
        under(
            &mut tools,
            &a_node(serde_json::json!({"scope": [".engineering/**=denied"]})),
        );
        assert!(writing(&mut tools, SPECIFICATION).failed);

        tools
            .catalogue_mut()
            .narrow(harness_catalogue::Scope::default());

        assert!(
            !writing(&mut tools, SPECIFICATION).failed,
            "the next step is under the run's scope alone"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(SPECIFICATION)).expect("written"),
            "revision: 99\n"
        );
    }
}
