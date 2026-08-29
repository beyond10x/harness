//! Three tools, whatever the catalogue holds.
//!
//! # Why an indirection this component's README spent a paragraph rejecting
//!
//! It said: *"Tools are published directly. Each admitted operation is its own model tool with its
//! real input schema — no `search`/`describe`/`invoke` indirection, no vendor tool ceiling to
//! dodge."* That was right about its own reason and is now outweighed by a different one.
//!
//! The evaluation compares four arms across three harnesses, and each harness names its tools
//! differently — `Bash` here, `run` there, `Write` and `workspace_write` for one act. Everything
//! that reads a run therefore had to learn one vendor's vocabulary, and the corpus that judges them
//! was written in Claude Code's. Publishing three verbs over one catalogue makes the **names ours on
//! every harness**, which is worth more than a flat surface on ours alone.
//!
//! The dodge the README warned about is still not the reason. This catalogue has six entries; it
//! would fit under any ceiling. What it buys is one surface, not a smaller one.
//!
//! # What this costs, stated rather than discovered
//!
//! A model that would have called `file_read` directly now spends a turn on `tool_describe` first,
//! or guesses the arguments. Whether that costs turns in practice is an experiment, not a claim —
//! and it is the one the first live run under this surface answers.
//!
//! **It answered.** 33% to 44% of every tool call across three runs was discovery, and 12.2% of
//! one run's calls were the model reaching for an entry by its bare name. The first is what
//! [`Catalogue::brief`](crate::Catalogue::brief) puts in the standing instruction; the second is
//! now routed here rather than refused. A caller who wants the entries published as themselves,
//! with their real schemas, takes [`Flat`](crate::Flat) instead — same catalogue, same names, no
//! indirection. These verbs stay because metaharness serves them over MCP, where one surface
//! across three harnesses is the whole point.

use std::time::Duration;

use harness_wire::{
    AccessKind, Approval, Effect, Envelope, Idempotency, Risk, Subject, ToolCall, ToolName,
    ToolOutcome, ToolPort, ToolSpec,
};
use serde_json::{Value, json};

use crate::Catalogue;

/// Find the tools this run has.
pub const SEARCH_VERB: &str = "tool_search";
/// Read one tool's arguments and effects.
pub const DESCRIBE_VERB: &str = "tool_describe";
/// Call one tool.
pub const INVOKE_VERB: &str = "tool_invoke";

/// The three verbs, over one catalogue.
pub struct Verbs {
    catalogue: Catalogue,
    specs: Vec<ToolSpec>,
}

impl std::fmt::Debug for Verbs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Verbs")
            .field("catalogue", &self.catalogue)
            .finish_non_exhaustive()
    }
}

impl Verbs {
    /// The three verbs over this catalogue.
    pub fn new(catalogue: Catalogue) -> Self {
        Self {
            catalogue,
            specs: vec![search_spec(), describe_spec(), invoke_spec()],
        }
    }

    /// What the catalogue holds, for a caller that wants to look without going through the model.
    pub fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }
}

/// The arguments of a `tool_invoke` that carried none.
///
/// `Null` rather than an empty object, and it makes no difference to anything downstream:
/// `Value::get` on it answers `None`, so every required field is reported missing by name exactly
/// as it would be for `{}`.
static NO_ARGUMENTS: Value = Value::Null;

impl Verbs {
    /// The catalogue entry this call names, and the arguments meant for it.
    ///
    /// # Two ways in, because the model uses both
    ///
    /// The published way is `tool_invoke {name, arguments}`. The other is the model calling an
    /// entry by its bare name — `file_read {"path": …}` — which it does because the standing
    /// instruction lists the entries and their arguments, and because every other harness it has
    /// ever seen publishes tools flat. Measured on a live run under this surface: **10 of 82 tool
    /// calls (12.2%) were a bare entry name**, and each one was a dead turn.
    ///
    /// So a bare entry name is a **route**, not a publication: [`ToolPort::specs`] still answers
    /// the three verbs, and nothing tells the model this works. What it stops is a turn burnt on
    /// a refusal for a call the run could have performed, with the entry's own spec deciding
    /// approval either way.
    fn invoked_entry<'a>(
        &'a self,
        call: &'a ToolCall,
    ) -> Option<(&'a crate::catalogue::Entry, &'a Value)> {
        if call.name.as_str() == INVOKE_VERB {
            let name = call.arguments.get("name").and_then(Value::as_str)?;
            let entry = self.catalogue.get(name)?;
            let arguments = call.arguments.get("arguments").unwrap_or(&NO_ARGUMENTS);
            return Some((entry, arguments));
        }
        // The arguments are the entry's own here — there is no envelope to unwrap.
        self.catalogue
            .get(call.name.as_str())
            .map(|entry| (entry, &call.arguments))
    }

    /// One call, whichever of the three verbs — or which entry — it names.
    ///
    /// `&self` rather than `&mut self`: nothing here mutates, and
    /// [`call_batch`](ToolPort::call_batch) needs to answer the verbs that are not invocations
    /// while the invocations are running on their own threads.
    fn answer(&self, call: &ToolCall, remaining: Option<Duration>) -> Result<Value, String> {
        let arguments = &call.arguments;
        match call.name.as_str() {
            SEARCH_VERB => Ok(self.catalogue.search(
                arguments.get("query").and_then(Value::as_str),
                arguments.get("effect").and_then(Value::as_str),
            )),
            DESCRIBE_VERB => match arguments.get("name").and_then(Value::as_str) {
                Some(name) => self.catalogue.describe(name),
                None => Err("`name` is required and names one tool".to_owned()),
            },
            INVOKE_VERB => match arguments.get("name").and_then(Value::as_str) {
                Some(name) => self.catalogue.invoke_within(
                    name,
                    arguments.get("arguments").unwrap_or(&NO_ARGUMENTS),
                    remaining,
                ),
                None => Err("`name` is required and names the tool to call".to_owned()),
            },
            // An entry called by its own name is performed rather than refused; see
            // [`Self::invoked_entry`]. A name that is neither a verb nor an entry is refused by
            // the catalogue, listing every name this run does have.
            other => self.catalogue.invoke_within(other, arguments, remaining),
        }
    }
}

impl ToolPort for Verbs {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    /// What the **catalogue** can do, which is the question the three verbs hide.
    ///
    /// The verbs are the same on every run; this is what differs, and it is what a reader of the
    /// record actually wants to know about the run's reach.
    fn operations(&self) -> Vec<&'static str> {
        self.catalogue.operations()
    }

    /// The subjects of the **entry being invoked**, not of the verb.
    ///
    /// A gate that read `tool_invoke`'s own arguments would see one opaque blob for every call in
    /// the run. What it needs is the file or the program underneath, so the verb is unwrapped
    /// before the question is answered — which is the whole reason `subjects` is per-call rather
    /// than per-spec.
    fn subjects(&self, call: &ToolCall) -> Vec<Subject> {
        self.invoked_entry(call)
            .map(|(entry, arguments)| entry.subjects(arguments))
            .unwrap_or_default()
    }

    /// The spec of the **entry being invoked**, not of the verb.
    ///
    /// `tool_invoke`'s own envelope declares every effect any entry can have, because it has to;
    /// gating on it would ask a person before every `file_read`. What decides is what the named
    /// entry does, under the entry's own name — so an approver is asked about `file_write` and a
    /// refusal says `file_write`, not the verb it came through. A `tool_invoke` that names no
    /// entry, or one this run does not have, touches nothing: the catalogue refuses it by name
    /// before anything runs, and the model learns the names — so it is described as the read it
    /// amounts to rather than as the run it is not.
    /// The spec of the entry, whether it was named inside `tool_invoke` or called by its own name.
    fn invoked(&self, call: &ToolCall) -> Option<ToolSpec> {
        let Some(published) = self.specs.iter().find(|spec| spec.name == call.name) else {
            // Not a verb. A bare entry name is routed rather than refused, and what decides
            // approval is the entry's own spec — the same one an approver would have been handed
            // had the call arrived through `tool_invoke`.
            return self
                .catalogue
                .get(call.name.as_str())
                .map(crate::catalogue::Entry::spec);
        };
        if call.name.as_str() != INVOKE_VERB {
            return Some(published.clone());
        }
        Some(self.invoked_entry(call).map_or_else(
            || ToolSpec {
                envelope: Envelope::default(),
                ..published.clone()
            },
            |(entry, _)| entry.spec(),
        ))
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        self.call_within(call, None)
    }

    fn call_within(&mut self, call: &ToolCall, remaining: Option<Duration>) -> ToolOutcome {
        outcome(self.answer(call, remaining))
    }

    /// Runs the invocations side by side and answers the catalogue questions where they stand.
    ///
    /// `tool_search` and `tool_describe` read a list this process already holds, so a thread for
    /// one would cost more than it saved; they are answered in place and the invocations go to
    /// [`Catalogue::invoke_batch`](crate::Catalogue::invoke_batch). Positions are kept either way,
    /// because the loop matches outcomes to calls by index.
    fn call_batch(&mut self, calls: &[ToolCall], remaining: Option<Duration>) -> Vec<ToolOutcome> {
        let routed: Vec<Option<(&str, &Value)>> = calls
            .iter()
            .map(|call| {
                self.invoked_entry(call)
                    .map(|(entry, arguments)| (entry.name, arguments))
            })
            .collect();
        let invocations: Vec<(&str, &Value)> = routed.iter().flatten().copied().collect();
        let mut answered = self
            .catalogue
            .invoke_batch(&invocations, remaining)
            .into_iter();
        routed
            .iter()
            .zip(calls)
            .map(|(routed, call)| match routed {
                Some((name, _)) => answered.next().map_or_else(
                    || {
                        ToolOutcome::failed(format!(
                            "`{name}` was asked for in a batch and no answer came back with it"
                        ))
                    },
                    outcome,
                ),
                None => outcome(self.answer(call, remaining)),
            })
            .collect()
    }
}

/// What the model reads: a failure is an outcome, never an error, or the next turn assumes the
/// effect landed.
fn outcome(result: Result<Value, String>) -> ToolOutcome {
    match result {
        Ok(output) => ToolOutcome::ok(output),
        Err(message) => ToolOutcome::failed(message),
    }
}

/// The verbs themselves describe nothing and change nothing.
///
/// `tool_search` and `tool_describe` genuinely read a list this process already holds — no
/// filesystem, no process, nothing that outlives the call. `tool_invoke` is the interesting one: its
/// *own* envelope cannot be honest, because what it does depends entirely on the entry it names.
/// Declaring it `Read`/`Low` would be a lie about a call that may start a process, so it declares
/// what it is: [`Risk::Conditional`]-shaped, in the only field that can carry that.
fn verb_envelope(effects: Vec<Effect>, risk: Risk, idempotency: Idempotency) -> Envelope {
    Envelope {
        effects,
        risk,
        idempotency,
        access: Vec::new(),
    }
}

fn search_spec() -> ToolSpec {
    ToolSpec {
        name: ToolName::new(SEARCH_VERB).expect("a constant verb name is legal"),
        description: "List the tools this run has. Call it with no arguments to see all of them — \
                      the list is short. Each entry names an operation, what it does and how much a \
                      wrong call costs."
            .to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Substring of a name, operation or summary."},
                "effect": {"type": "string", "description": "Only tools with this effect: read, write, network, process, filesystem."},
            },
            "additionalProperties": false,
        }),
        approval: Approval::NotRequired,
        envelope: verb_envelope(vec![Effect::Read], Risk::Low, Idempotency::Idempotent),
    }
}

fn describe_spec() -> ToolSpec {
    ToolSpec {
        name: ToolName::new(DESCRIBE_VERB).expect("a constant verb name is legal"),
        description: "Read one tool's arguments, effects and risk before calling it. A name this \
                      run does not have is refused, listing the names it does."
            .to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "The tool, as `tool_search` named it."},
            },
            "required": ["name"],
            "additionalProperties": false,
        }),
        approval: Approval::NotRequired,
        envelope: verb_envelope(vec![Effect::Read], Risk::Low, Idempotency::Idempotent),
    }
}

fn invoke_spec() -> ToolSpec {
    ToolSpec {
        name: ToolName::new(INVOKE_VERB).expect("a constant verb name is legal"),
        description:
            "Call one tool by name, with its own arguments. A name this run does not have \
                      is refused here, before anything happens, listing the names it does."
                .to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "The tool, as `tool_search` named it."},
                "arguments": {"type": "object", "description": "Its arguments, as `tool_describe` gave them."},
            },
            "required": ["name", "arguments"],
            "additionalProperties": false,
        }),
        approval: Approval::NotRequired,
        // Every effect any entry can have, because this verb can reach any of them and a narrower
        // declaration would be a claim about a call whose target is not known until it arrives.
        envelope: Envelope {
            effects: vec![
                Effect::Read,
                Effect::Write,
                Effect::Filesystem,
                Effect::Process,
            ],
            risk: Risk::High,
            idempotency: Idempotency::Conditional,
            access: vec![AccessKind::Filesystem, AccessKind::Process],
        },
    }
}
