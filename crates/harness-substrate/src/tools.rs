//! The tools that exist only where the machine can confine them.
//!
//! # Publication is the first gate, and it is the quiet one
//!
//! [`ConfinedTools::specs`] is computed from [`Facts`], once. On a machine with no delegated cgroup
//! root there is no `run` in the list at all — the model is never told about a tool it cannot have,
//! never plans around one, and never spends a turn being refused. On a machine with no substrate
//! daemon the list is **empty**, and the harness is exactly the read-only thing it has always been.
//!
//! # `run`, and deliberately not `bash`
//!
//! An open shell is unbounded by construction. `sh -c` composes, redirects and substitutes, so the
//! subject of the call is not knowable before it runs — and a subject nobody can compute is one
//! nobody can authorize, which collapses the middle gate into nothing.
//!
//! `run` takes an argv and a program from a **declared set**. The set is in the tool's own schema,
//! so the model can read what it may run instead of guessing and being refused; a program outside
//! it is refused by name, listing the set.
//!
//! Substrate reaches the same place from the other side: `exec.start`'s first capability predicate
//! is `exec.argv-only`. Neither component will run a shell, and neither had to be told by the other.

use serde_json::{Value, json};

use harness_wire::{
    AccessKind, Approval, Effect, Envelope, Idempotency, Risk, Subject, ToolCall, ToolName,
    ToolOutcome, ToolPort, ToolSpec,
};

use crate::{Backend, Facts};

/// Writes one file, whole.
pub const WRITE_TOOL: &str = "workspace_write";
/// Replaces one exact string in one file.
pub const EDIT_TOOL: &str = "workspace_edit";
/// Runs one declared program.
pub const RUN_TOOL: &str = "run";

/// The tools a confined workspace admits on this machine.
pub struct ConfinedTools {
    backend: Box<dyn Backend>,
    workspace: String,
    programs: Vec<String>,
    specs: Vec<ToolSpec>,
}

impl std::fmt::Debug for ConfinedTools {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfinedTools")
            .field("workspace", &self.workspace)
            .field("programs", &self.programs)
            .field("published", &self.specs.len())
            .finish_non_exhaustive()
    }
}

impl ConfinedTools {
    /// The toolset this machine admits.
    ///
    /// `programs` is the declared set `run` may name. An empty set publishes no `run` even on a
    /// machine that could confine one: a workflow that named no commands wants none, and a tool
    /// that admitted everything because nobody listed anything is the failure mode this design
    /// exists to prevent.
    pub fn new(
        backend: impl Backend + 'static,
        facts: &Facts,
        workspace: impl Into<String>,
        programs: Vec<String>,
    ) -> Self {
        let mut specs = Vec::new();
        if facts.holds_workspaces() {
            specs.push(write_spec());
            specs.push(edit_spec());
        }
        if facts.confines_execution() && !programs.is_empty() {
            specs.push(run_spec(&programs));
        }
        Self {
            backend: Box::new(backend),
            workspace: workspace.into(),
            programs,
            specs,
        }
    }

    /// The programs `run` will accept, in the order they were declared.
    pub fn programs(&self) -> &[String] {
        &self.programs
    }

    fn write(&self, arguments: &Value) -> Result<Value, String> {
        let path = string(arguments, "path")?;
        let text = string(arguments, "text")?;
        self.backend
            .file_write(&self.workspace, path, text)
            .map_err(|error| error.to_string())
    }

    fn edit(&self, arguments: &Value) -> Result<Value, String> {
        let path = string(arguments, "path")?;
        let old = string(arguments, "old")?;
        let new = string(arguments, "new")?;

        let current = self
            .backend
            .file_read(&self.workspace, path)
            .map_err(|error| error.to_string())?;
        let matches = current.matches(old).count();
        // Neither *none* nor *several* is an edit. A replacement that hit nothing leaves the model
        // believing a change landed, and one that hit four places changed three things nobody
        // asked about — which is why this is checked here rather than left to a `replace` call.
        match matches {
            0 => return Err(format!("`{path}` does not contain that text, so nothing was changed")),
            1 => {}
            several => {
                return Err(format!(
                    "`{path}` contains that text {several} times; an edit must name one place. \
                     Include more surrounding text."
                ));
            }
        }
        self.backend
            .file_write(&self.workspace, path, &current.replacen(old, new, 1))
            .map_err(|error| error.to_string())
    }

    fn run(&self, arguments: &Value) -> Result<Value, String> {
        let argv: Vec<String> = arguments
            .get("argv")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let Some(program) = argv.first() else {
            return Err("`argv` is required and names the program first".to_owned());
        };
        if !self.programs.iter().any(|allowed| allowed == program) {
            return Err(format!(
                "`{program}` is not a program this run may start. Declared: {}.",
                self.programs.join(", ")
            ));
        }
        self.backend
            .exec(&self.workspace, &argv)
            .map_err(|error| error.to_string())
    }
}

impl ToolPort for ConfinedTools {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn subjects(&self, call: &ToolCall) -> Vec<Subject> {
        match call.name.as_str() {
            WRITE_TOOL | EDIT_TOOL => call
                .arguments
                .get("path")
                .and_then(Value::as_str)
                .map(|path| vec![Subject::file(path)])
                .unwrap_or_default(),
            RUN_TOOL => call
                .arguments
                .get("argv")
                .and_then(Value::as_array)
                .and_then(|argv| argv.first())
                .and_then(Value::as_str)
                .map(|program| vec![Subject::process(program)])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        let result = match call.name.as_str() {
            WRITE_TOOL => self.write(&call.arguments),
            EDIT_TOOL => self.edit(&call.arguments),
            RUN_TOOL => self.run(&call.arguments),
            other => Err(format!("`{other}` is not published here")),
        };
        match result {
            Ok(output) => ToolOutcome::ok(output),
            Err(message) => ToolOutcome::failed(message),
        }
    }
}

fn string<'a>(arguments: &'a Value, field: &str) -> Result<&'a str, String> {
    arguments
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("`{field}` is required"))
}

fn write_spec() -> ToolSpec {
    ToolSpec {
        name: ToolName::new(WRITE_TOOL).expect("constant tool name is valid"),
        description: "Write one file in the workspace, whole. Creates it if it is not there. \
                      Replacing an existing file replaces all of it — use `workspace_edit` to \
                      change part of one."
            .to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File relative to the workspace root."},
                "text": {"type": "string", "description": "The whole new contents."},
            },
            "required": ["path", "text"],
            "additionalProperties": false,
        }),
        approval: Approval::NotRequired,
        envelope: Envelope {
            effects: vec![Effect::Write, Effect::Filesystem],
            risk: Risk::Medium,
            // Writing the same bytes twice leaves the same file. What is not idempotent is the
            // *edit* below, which is why they are two tools rather than one with a flag.
            idempotency: Idempotency::Idempotent,
            access: vec![AccessKind::Filesystem],
        },
    }
}

fn edit_spec() -> ToolSpec {
    ToolSpec {
        name: ToolName::new(EDIT_TOOL).expect("constant tool name is valid"),
        description: "Replace one exact piece of text in one workspace file. The text must appear \
                      exactly once; a replacement that matched nothing, or several places, is \
                      refused rather than guessed at."
            .to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File relative to the workspace root."},
                "old": {"type": "string", "description": "The exact text to replace, appearing once."},
                "new": {"type": "string", "description": "What to put in its place."},
            },
            "required": ["path", "old", "new"],
            "additionalProperties": false,
        }),
        approval: Approval::NotRequired,
        envelope: Envelope {
            effects: vec![Effect::Write, Effect::Filesystem],
            risk: Risk::Medium,
            // Running it twice is not running it once: the second attempt finds nothing to replace,
            // and under a retreat that re-runs a whole scope that is exactly what happens.
            idempotency: Idempotency::NonIdempotent,
            access: vec![AccessKind::Filesystem],
        },
    }
}

fn run_spec(programs: &[String]) -> ToolSpec {
    ToolSpec {
        name: ToolName::new(RUN_TOOL).expect("constant tool name is valid"),
        description: format!(
            "Run one program in the confined workspace and answer its output and exit status. \
             This is not a shell: `argv` is a list, nothing is composed, redirected or substituted, \
             and only these programs may be named — {}.",
            programs.join(", ")
        ),
        input_schema: json!({
            "type": "object",
            "properties": {
                "argv": {
                    "type": "array",
                    "minItems": 1,
                    "items": {"type": "string"},
                    "description": "The program and its arguments. The first item must be one of the declared programs.",
                    // The model can read the set instead of guessing at it and being refused.
                    "prefixItems": [{"enum": programs}],
                },
            },
            "required": ["argv"],
            "additionalProperties": false,
        }),
        approval: Approval::NotRequired,
        envelope: Envelope {
            effects: vec![Effect::Process],
            risk: Risk::High,
            idempotency: Idempotency::Conditional,
            access: vec![AccessKind::Process, AccessKind::Filesystem],
        },
    }
}
