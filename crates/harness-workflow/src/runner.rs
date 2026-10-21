//! [`StepRunner`] over the ports a [`Walk`] was lent: a step is a turn, a section is a
//! conversation.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use harness_flow::{
    FlowEvent, FlowSink, Gate, Handoff, Moment, NodeId, Step, StepContext, StepOutcome, StepRunner,
};
use harness_loop::{
    AgentLoop, Budget, HookDecision, HookPoint, LoopError, LoopEvent, LoopOutcome, LoopSink,
    LoopStop, OutputSchema, RunLedger,
};
use harness_wire::{CallId, Item, ToolCall, ToolName};
use serde_json::{Map, Value};

use crate::command::command_outcome;
use crate::document::{StepAction, action_of, answer_schema, declared_scope, plain, scope_lines};
use crate::{Section, Transition, Walk};

/// Where the walk reports, until it halts.
///
/// A cancelled step and a broken wire both end the flow where it stands, and the walk has no word
/// for that — it goes on skipping and leaving sections it will never run. Emitting those would
/// claim things happened that did not, so once the runner has halted **nothing more reaches the
/// sink**, `flow-finished` included (design 0003 § 2). What already ran is still filed; the
/// caller's error and the loop's own last event say why there is no ending.
pub(crate) struct Halting<'w> {
    pub(crate) events: &'w mut dyn FlowSink,
    pub(crate) halted: &'w Cell<bool>,
}

impl FlowSink for Halting<'_> {
    fn emit(&mut self, event: FlowEvent) {
        if self.halted.get() {
            return;
        }
        self.events.emit(event);
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
pub(crate) struct Scope {
    path: String,
    attempt: u32,
    /// The conversation, lent to the loop one step at a time.
    items: Vec<Item>,
    /// What the steps of this section answered under `gives`, last write wins.
    gives: Handoff,
}

impl Scope {
    fn section(&self) -> Section<'_> {
        Section {
            path: &self.path,
            attempt: self.attempt,
        }
    }
}

/// A step is a turn: [`StepRunner`] over one [`Walk`]'s ports.
pub(crate) struct Runner<'w, 'a> {
    pub(crate) walk: &'w mut Walk<'a>,
    /// The document's own name, as a governor is told it.
    pub(crate) flow: &'w str,
    /// The flow's own task, given to every step beside the step's own prompt.
    pub(crate) input: &'w str,
    /// What each section promises, by path.
    pub(crate) promises: BTreeMap<String, Vec<NodeId>>,
    /// How many attempts each section may take, by path — the `of` a governor is told.
    pub(crate) bounds: BTreeMap<String, u32>,
    /// The sections open right now, innermost last.
    pub(crate) open: Vec<Scope>,
    /// What the **whole walk** has spent, folded step by step.
    ///
    /// Not the ledger lent to [`AgentLoop::run_in`]: that one is replaced with one step's figures
    /// on every exit path, by design, so a running total cannot be kept in it. This is what the
    /// flow-wide ceilings are measured against (see the crate header).
    pub(crate) spend: RunLedger,
    /// When the walk began, for the same reason: the duration ceiling bounds the walk.
    pub(crate) started: Instant,
    /// The loop's events, and the walk's own warnings and governor consultations beside them.
    pub(crate) events: &'w mut dyn LoopSink,
    /// Set when a step was cancelled or broke on the wire: the walk stops running anything.
    pub(crate) halted: &'w Cell<bool>,
    /// The wire failure that ended the walk, when one did.
    pub(crate) aborted: Option<String>,
    /// How many `command` steps have run, which names the next one's call.
    pub(crate) commands: u32,
}

impl Runner<'_, '_> {
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
        // Read from the catalogue and not from the payload a second time: what the tool will
        // enforce is what the step is told, so the two can never come to say different things. The
        // tool is what makes it true; this is what stops the step spending a turn — a paid one —
        // discovering its own document's rule by being refused.
        let narrowed = self.walk.tools.catalogue().step_scope();
        if !narrowed.is_empty() {
            turn.push_str(
                "\n\nWhere this step may write, on top of the run's own scope. A path no rule \
                 names is unrestricted:\n",
            );
            turn.push_str(&scope_lines(narrowed));
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
    /// # A `context` name is not a path this walk may follow anywhere
    ///
    /// The document is not the operator: a projection is generated by another component, and the
    /// workspace the caller named is the sentence saying what this walk may see.
    /// `workspace.join(name)` drops the base the moment `name` is absolute, and `..` walks out of
    /// it — so a document could name a key file or `../../secrets.env` and the runner would read
    /// it into a model turn, past the confinement the tools are under and past the write scope,
    /// without anything saying so. Every name is therefore resolved and **proved to be inside**
    /// the canonicalised workspace before it is read; canonicalising is also what makes a symlink
    /// out of the tree refuse rather than succeed. A walk its caller gave no workspace reads no
    /// context file at all.
    ///
    /// # Errors
    ///
    /// [`Refused`] under `context-refused` for a name outside the workspace or one that is not
    /// there, naming the path and the workspace, or for any name when there is no workspace;
    /// under `step-context` for a file that is inside and still could not be read. Absent is an
    /// error and not an empty string, for the reason `--context` gives: a run told it has a file
    /// and given nothing reports on what it did not read.
    fn given(&self, path: &str, payload: Option<&Map<String, Value>>) -> Result<String, Refused> {
        let Some(files) = payload
            .and_then(|map| map.get("context"))
            .and_then(Value::as_array)
        else {
            return Ok(String::new());
        };
        let Some(root) = self.walk.workspace else {
            return Err(Refused::context(format!(
                "`{path}` names context files and this walk was given no workspace to read them \
                 from"
            )));
        };
        let workspace = root.canonicalize().map_err(|error| {
            Refused::context(format!(
                "`{path}` names context files and the workspace `{}` could not be resolved: \
                 {error}",
                root.display()
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
        let mut budget = self.walk.config.budget.clone();
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

    /// Asks the caller's governor whether this boundary may be crossed.
    ///
    /// With no governor every boundary proceeds, and no [`LoopEvent::HookRan`] is recorded for a
    /// consultation that did not happen: a record showing a hook at every boundary of a run with
    /// no governor would be a gate a reader believes was there. **This is deliberately stricter
    /// than the loop**, which emits `HookRan { decision: proceed }` at each of its three points
    /// whenever *any* hooks are attached, declared for that point or not.
    ///
    /// Both a `Block` and a `Failed` become [`Gate::Refused`], carrying the words they came with. A
    /// governor that could not answer did not say yes, and that reading is the same at both
    /// moments.
    fn ask(
        &mut self,
        path: &str,
        moment: Moment,
        attempt: u32,
        leave: Option<(bool, &Handoff)>,
    ) -> Gate {
        let Some(governor) = self.walk.governor.as_deref_mut() else {
            return Gate::Proceed;
        };
        let of = self.bounds.get(path).copied().unwrap_or(1);
        let decision = governor.transition(&Transition {
            flow: self.flow,
            path,
            moment,
            attempt,
            of,
            leave,
        });
        self.events.emit(LoopEvent::HookRan {
            point: HookPoint::Transition,
            call_id: None,
            decision: decision.clone(),
        });
        match decision {
            HookDecision::Proceed => Gate::Proceed,
            HookDecision::Block { reason } | HookDecision::Failed { reason } => {
                Gate::Refused { reason }
            }
        }
    }

    /// One warning on the same stream as everything else the walk reports.
    fn warn(&mut self, code: &str, message: &str) {
        self.events.emit(LoopEvent::Warning {
            code: code.to_owned(),
            message: message.to_owned(),
        });
    }

    /// Runs a `command` step: its argv as one `run` call through the walk's gate, filed into the
    /// section's conversation, and read as the step's outcome.
    ///
    /// The call is named `flow-command-<n>` for the walk, because a call id is what the result is
    /// matched to in the conversation and a model never minted one for this call. The budget is
    /// the walk's remainder, exactly as a model step's is: the clock the call runs under is what
    /// the duration ceiling has left.
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
        let config = self.walk.config.clone().with_budget(budget);
        let outcome = {
            let walk = &mut *self.walk;
            let mut agent = AgentLoop::new(
                &mut *walk.model,
                &mut *walk.tools,
                &mut *walk.approvals,
                config,
            )
            .with_cancel(walk.cancel.clone());
            if let Some(hooks) = walk.hooks.as_deref_mut() {
                agent = agent.with_hooks(hooks);
            }
            agent.call(&call, &mut *self.events)
        };
        {
            // Filed as a model's call would be — the call, then its result — so the section's
            // next step reads what the program said where it would have read a tool's answer,
            // and the record shows the command that ran and what it printed.
            let scope = &mut self.open[index];
            scope.items.push(Item::ToolCall(call.clone()));
            scope
                .items
                .push(Item::result(call.call_id.clone(), outcome.clone()));
            if let Some(record) = self.walk.record.as_deref_mut() {
                record.called(scope.section(), &scope.items);
            }
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

    /// Which of the open sections this step belongs to.
    ///
    /// The innermost match, which is the section the walk is standing in: a step of a group runs
    /// while no child of that group is open.
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

    /// Sends one step's turn into its section's conversation and folds what it cost into the walk.
    ///
    /// `step_spend` is left holding this step's own figures, which a broken step files with its
    /// conversation.
    fn converse(
        &mut self,
        scope: &str,
        items: &mut Vec<Item>,
        step_spend: &mut RunLedger,
        turn: String,
        budget: Budget,
    ) -> Result<LoopOutcome, LoopError> {
        let names: &[NodeId] = self.promises.get(scope).map_or(&[], Vec::as_slice);
        let schema = OutputSchema::new(answer_schema(names))
            .expect("the derived answer schema is an object schema");
        // Cloned per step: the config is the caller's, and what differs section by section is the
        // shape of the answer and what the walk has left to spend.
        let config = self
            .walk
            .config
            .clone()
            .with_output_schema(Some(schema))
            .with_budget(budget);
        // The loop replaces the ledger it is lent with this step's own figures, so the walk's
        // running total is folded from it here rather than kept in it.
        let outcome = {
            let walk = &mut *self.walk;
            let mut agent = AgentLoop::new(
                &mut *walk.model,
                &mut *walk.tools,
                &mut *walk.approvals,
                config,
            )
            .with_cancel(walk.cancel.clone());
            if let Some(hooks) = walk.hooks.as_deref_mut() {
                agent = agent.with_hooks(hooks);
            }
            agent.run_in(items, step_spend, turn, &mut *self.events)
        };
        self.absorb(step_spend);
        outcome
    }

    /// The step itself, with the catalogue already narrowed to what its node declared.
    fn stepped(&mut self, path: &str, step: &Step, context: &StepContext) -> StepOutcome {
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
        // The document decides which of the three native step kinds this is. Validation already
        // read the same payload before the walk began, so an error here would mean the document
        // changed underneath an in-memory walk.
        match action_of(step) {
            Ok(StepAction::Llm) => {}
            Ok(StepAction::Command(argv)) => return self.command(path, index, &argv, budget),
            Ok(StepAction::Operator { prompt }) => {
                return StepOutcome::Paused { reason: prompt };
            }
            Err(reason) => {
                self.warn("step-kind", &format!("`{path}` {reason}"));
                return StepOutcome::Failed;
            }
        }
        // Also before anything is sent: a step whose declared context this walk may not read is a
        // step nobody could reproduce from the document, and asking a model without it would be
        // answering a different question at full price.
        let turn = match self.turn(path, step, context) {
            Ok(turn) => turn,
            Err(refused) => {
                self.warn(refused.code, &refused.reason);
                return StepOutcome::Failed;
            }
        };
        let mut items = std::mem::take(&mut self.open[index].items);
        let mut step_spend = RunLedger::default();
        let outcome = self.converse(&context.scope, &mut items, &mut step_spend, turn, budget);

        match outcome {
            Ok(finished) => {
                {
                    let scope = &mut self.open[index];
                    if let Some(record) = self.walk.record.as_deref_mut() {
                        record.answered(scope.section(), &finished);
                    }
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
            // it cost — the same rule a single run follows for one that broke on turn twenty.
            Err(error) => {
                {
                    let scope = &mut self.open[index];
                    if let Some(record) = self.walk.record.as_deref_mut() {
                        record.broke(scope.section(), &items, &step_spend);
                    }
                    scope.items = items;
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
}

impl StepRunner for Runner<'_, '_> {
    /// One step, under the write scope **its own node declares**.
    ///
    /// The node's `scope` is laid over the run's for the length of the step and taken off again
    /// however the step ends, so the boundary a document states is the boundary the step meets —
    /// in the tool, before the write, as a refused tool outcome the model is told about
    /// (`AGENTS.md` invariants 9 and 12). **It can only narrow.** Both layers are asked and the
    /// first refusal wins ([`harness_catalogue::Catalogue::narrow`]), so a node cannot re-permit
    /// what the run's own scope denied. A node declaring nothing runs under the run's scope.
    fn run(&mut self, path: &str, step: &Step, context: &StepContext) -> StepOutcome {
        // An operator step reaches no execution boundary at all. In particular it does not read
        // or apply `scope`, consult a call hook or a budget, open a tool call, or ask a provider.
        // The scheduler records it as reached and stops at the typed pause outcome.
        if let Ok(StepAction::Operator { prompt }) = action_of(step) {
            return StepOutcome::Paused { reason: prompt };
        }
        // Read before anything is narrowed, because a `scope` this build cannot read is a failed
        // step and never a fall-through to the run's. The document was validated before the walk
        // began, so this refuses only for a document nothing on this path could have produced.
        let declared = match declared_scope(step) {
            Ok(declared) => declared,
            Err(reason) => {
                self.warn("step-write-scope", &format!("`{path}` {reason}"));
                return StepOutcome::Failed;
            }
        };
        self.walk.tools.catalogue_mut().narrow(declared);
        let outcome = self.stepped(path, step, context);
        // However the step ended. A narrowing that outlived its step would be one node's rule
        // silently governing its siblings, which no document says.
        self.walk
            .tools
            .catalogue_mut()
            .narrow(harness_catalogue::Scope::default());
        outcome
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
    /// a section nobody allowed to run has nothing to file, and a record opened for an attempt
    /// that never happened would be one nobody could resume into anything.
    ///
    /// A halted walk asks nobody. It is going nowhere, and asking the governor about a section that
    /// will not run would be a consultation about nothing.
    fn entering(&mut self, path: &str, attempt: u32) -> Gate {
        if self.halted.get() {
            return Gate::Proceed;
        }
        if let Gate::Refused { reason } = self.ask(path, Moment::Enter, attempt, None) {
            return Gate::Refused { reason };
        }
        if let Some(record) = self.walk.record.as_deref_mut() {
            let above: Vec<Section<'_>> = self.open.iter().map(Scope::section).collect();
            record.opened(Section { path, attempt }, &above);
        }
        self.open.push(Scope {
            path: path.to_owned(),
            attempt,
            items: Vec::new(),
            gives: Handoff::new(),
        });
        Gate::Proceed
    }

    /// Closes this attempt's record, then asks the governor whether its result is accepted.
    ///
    /// **Design 0003 § 3's `leave` moment**, asked after the handoff so that whoever answers has
    /// seen what the section is handing over. The record is closed **first and either way**: a
    /// refused attempt still happened and still cost what it cost, and a governor that declines a
    /// result must not be able to delete the transcript of it.
    ///
    /// Refusing an attempt that came out clean marks it failed, and the document's own `repeat`
    /// bound decides whether that becomes a retreat or the end of one — which is how a governor
    /// forces a retreat without this crate knowing the word.
    fn leaving(&mut self, path: &str, attempt: u32, failed: bool, handoff: &Handoff) -> Gate {
        // Nothing to close for a section the walk halted inside, or one `entering` never opened.
        if let Some(scope) = self
            .open
            .pop_if(|open| open.path == path && open.attempt == attempt)
            && let Some(record) = self.walk.record.as_deref_mut()
        {
            record.closed(scope.section());
        }
        // A walk the wire broke or a person stopped asks nobody: see `entering`.
        if self.halted.get() {
            return Gate::Proceed;
        }
        self.ask(path, Moment::Leave, attempt, Some((failed, handoff)))
    }
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
