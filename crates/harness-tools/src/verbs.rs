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

impl ToolPort for Verbs {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    /// The subjects of the **entry being invoked**, not of the verb.
    ///
    /// A gate that read `tool_invoke`'s own arguments would see one opaque blob for every call in
    /// the run. What it needs is the file or the program underneath, so the verb is unwrapped
    /// before the question is answered — which is the whole reason `subjects` is per-call rather
    /// than per-spec.
    /// What the **catalogue** can do, which is the question the three verbs hide.
    ///
    /// The verbs are the same on every run; this is what differs, and it is what a reader of the
    /// record actually wants to know about the run's reach.
    fn operations(&self) -> Vec<&'static str> {
        self.catalogue.operations()
    }

    fn subjects(&self, call: &ToolCall) -> Vec<Subject> {
        if call.name.as_str() != INVOKE_VERB {
            return Vec::new();
        }
        let Some(name) = call.arguments.get("name").and_then(Value::as_str) else {
            return Vec::new();
        };
        let arguments = call
            .arguments
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        self.catalogue
            .get(name)
            .map(|entry| entry.subjects(&arguments))
            .unwrap_or_default()
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        let arguments = &call.arguments;
        let result = match call.name.as_str() {
            SEARCH_VERB => Ok(self.catalogue.search(
                arguments.get("query").and_then(Value::as_str),
                arguments.get("effect").and_then(Value::as_str),
            )),
            DESCRIBE_VERB => match arguments.get("name").and_then(Value::as_str) {
                Some(name) => self.catalogue.describe(name),
                None => Err("`name` is required and names one tool".to_owned()),
            },
            INVOKE_VERB => match arguments.get("name").and_then(Value::as_str) {
                Some(name) => self.catalogue.invoke(
                    name,
                    arguments
                        .get("arguments")
                        .unwrap_or(&Value::Object(serde_json::Map::new())),
                ),
                None => Err("`name` is required and names the tool to call".to_owned()),
            },
            other => Err(format!("`{other}` is not published here")),
        };
        match result {
            Ok(output) => ToolOutcome::ok(output),
            Err(message) => ToolOutcome::failed(message),
        }
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
