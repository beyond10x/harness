//! What a run may do, as a list somebody can read.
//!
//! # Entries are named by neutral operations
//!
//! `file.read`, `file.write`, `file.edit`, `dir.list`, `search`, `shell` — metaharness's own
//! vocabulary (`metaharness_protocol::Operation`), not spelled out as a dependency but matched name
//! for name. That is the point of this crate: a consumer asks *did this run write a file* without
//! knowing whether the harness spells a write `Write`, `workspace_write` or `apply_patch`.
//!
//! # The catalogue is built from what the machine admits
//!
//! An entry is here because something can perform it. A provider that cannot write contributes no
//! writing entry; a machine with no delegated cgroup contributes no `run`. So
//! [`Catalogue::search`] answering four entries rather than six is the publication gate speaking,
//! not a filter — the model is never told about a tool it cannot have.

use std::collections::BTreeMap;

use harness_wire::{AccessKind, Effect, Envelope, Idempotency, Risk, Subject, ToolName, ToolSpec};
use serde_json::{Value, json};

use crate::Operations;

/// One thing a run may do.
pub struct Entry {
    /// The neutral operation, as metaharness names it: `file.write`, `shell`.
    pub operation: &'static str,
    /// The name `tool_invoke` takes and `tool_search` answers.
    pub name: &'static str,
    /// One line, for a reader choosing between entries.
    pub summary: String,
    /// What its arguments are.
    pub input_schema: Value,
    /// What it does, how much a wrong call costs, what it must reach.
    pub envelope: Envelope,
}

impl Entry {
    /// The concrete things one call would touch.
    ///
    /// A spec is a claim and this is the fact — the same split `harness_wire::ToolPort::subjects`
    /// draws, kept here so a gate reads one shape whichever entry it is looking at.
    ///
    /// The path is reported **as the caller wrote it**. A gate has to see `../../etc/passwd` as the
    /// model sent it; the tidy answer canonicalisation would give is exactly wrong for the one call
    /// whose whole problem is where it was going.
    pub fn subjects(&self, arguments: &Value) -> Vec<Subject> {
        match self.operation {
            "shell" => arguments
                .get("argv")
                .and_then(Value::as_array)
                .and_then(|argv| argv.first())
                .and_then(Value::as_str)
                .map(|program| vec![Subject::process(program)])
                .unwrap_or_default(),
            _ => vec![Subject::file(
                arguments.get("path").and_then(Value::as_str).unwrap_or("."),
            )],
        }
    }

    /// The entry as a tool of its own, for a consumer that publishes flatly.
    ///
    /// Not used by [`crate::Verbs`], which publishes three tools whatever the catalogue holds. It
    /// exists because an MCP client that lists tools gets the entries themselves, and because a
    /// caller who wants the old flat surface should not have to rebuild a `ToolSpec` from parts.
    ///
    /// # Panics
    ///
    /// Panics only if a constant entry name stops being a legal tool name.
    pub fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: ToolName::new(self.name).expect("a constant entry name is a legal tool name"),
            description: self.summary.clone(),
            input_schema: self.input_schema.clone(),
            approval: harness_wire::Approval::NotRequired,
            envelope: self.envelope.clone(),
        }
    }
}

/// Every entry a run may reach, and the thing that performs them.
pub struct Catalogue {
    operations: Box<dyn Operations>,
    entries: Vec<Entry>,
}

impl std::fmt::Debug for Catalogue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Catalogue")
            .field(
                "entries",
                &self.entries.iter().map(|e| e.name).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

impl Catalogue {
    /// The catalogue this provider admits.
    ///
    /// Reading entries are always there. Writing entries appear when the provider
    /// [`writes`](Operations::writes); `run` appears when it names at least one program. A provider
    /// that can confine execution but was given no declared set publishes no `run`: a workflow that
    /// named no commands wants none, and a tool that admitted everything because nobody listed
    /// anything is the failure this whole design exists to prevent.
    pub fn of(operations: impl Operations + 'static) -> Self {
        let mut entries = vec![file_read(), dir_list(), search()];
        if operations.writes() {
            entries.push(file_write());
            entries.push(file_edit());
        }
        if !operations.programs().is_empty() {
            entries.push(run(operations.programs()));
        }
        Self {
            operations: Box::new(operations),
            entries,
        }
    }

    /// Every entry, in the order they were built.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// One entry by name.
    pub fn get(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    /// The entries a query names, or all of them.
    ///
    /// No query answers the whole catalogue, which is small on purpose: the indirection exists so
    /// one surface serves three harnesses, not to hide a large catalogue behind a search box.
    pub fn search(&self, query: Option<&str>, effect: Option<&str>) -> Value {
        let matches: Vec<Value> = self
            .entries
            .iter()
            .filter(|entry| {
                query.is_none_or(|needle| {
                    let needle = needle.to_ascii_lowercase();
                    entry.name.to_ascii_lowercase().contains(&needle)
                        || entry.operation.contains(&needle)
                        || entry.summary.to_ascii_lowercase().contains(&needle)
                })
            })
            .filter(|entry| {
                effect.is_none_or(|wanted| {
                    entry
                        .envelope
                        .effects
                        .iter()
                        .any(|effect| format!("{effect:?}").eq_ignore_ascii_case(wanted))
                })
            })
            .map(|entry| {
                json!({
                    "name": entry.name,
                    "operation": entry.operation,
                    "summary": entry.summary,
                    "effects": entry.envelope.effects.iter().map(|e| format!("{e:?}").to_lowercase()).collect::<Vec<_>>(),
                    "risk": format!("{:?}", entry.envelope.risk).to_lowercase(),
                })
            })
            .collect();

        // A filtered answer says what it hid, and how to see it.
        //
        // The failure this closes was measured, not imagined. On 2026-08-24 a run through the b10x
        // loop opened with `tool_search {"effect": "read"}`, got three tools back, and never learnt
        // that `file_write`, `file_edit` and `run` existed — it read two files and then *reported
        // the task as done without doing it*. A filter the model chose became a ceiling it could
        // not see, and the answer looked complete because nothing said otherwise.
        //
        // Only when something was withheld, so an unfiltered call — the one the description asks
        // for — stays exactly as short as it was.
        let withheld = self.entries.len() - matches.len();
        if withheld == 0 {
            return json!({"tools": matches});
        }
        json!({
            "tools": matches,
            "total": self.entries.len(),
            "withheld_by_filter": withheld,
            "note": format!(
                "{withheld} of this run's {} tools do not match that filter and are not listed \
                 above. Call `tool_search` with no arguments to see all of them.",
                self.entries.len()
            ),
        })
    }

    /// One entry in full, or a refusal naming what is here.
    ///
    /// # Errors
    ///
    /// The name is not one the catalogue holds. The refusal lists every name that is, because the
    /// model's next move is to pick one and a bare *not found* does not help it.
    pub fn describe(&self, name: &str) -> Result<Value, String> {
        let entry = self.get(name).ok_or_else(|| self.no_such(name))?;
        Ok(json!({
            "name": entry.name,
            "operation": entry.operation,
            "summary": entry.summary,
            "input_schema": entry.input_schema,
            "envelope": entry.envelope,
        }))
    }

    /// Perform one entry.
    ///
    /// # Errors
    ///
    /// The name is not one the catalogue holds — refused **here**, with nothing performed and
    /// nothing sent anywhere — or the operation itself failed, in which case the words are its own.
    pub fn invoke(&self, name: &str, arguments: &Value) -> Result<Value, String> {
        let entry = self.get(name).ok_or_else(|| self.no_such(name))?;
        let string = |field: &str| -> Result<&str, String> {
            arguments
                .get(field)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("`{field}` is required by `{name}`"))
        };
        let path = || string("path");
        match entry.operation {
            "file.read" => self
                .operations
                .file_read(path()?, arguments.get("max_bytes").and_then(Value::as_u64)),
            "file.write" => self.operations.file_write(path()?, string("text")?),
            "file.edit" => self
                .operations
                .file_edit(path()?, string("old")?, string("new")?),
            "dir.list" => self
                .operations
                .dir_list(arguments.get("path").and_then(Value::as_str).unwrap_or(".")),
            "search" => self.operations.search(
                string("pattern")?,
                arguments.get("path").and_then(Value::as_str).unwrap_or("."),
                arguments
                    .get("max_results")
                    .and_then(Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok()),
            ),
            "shell" => {
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
                if argv.is_empty() {
                    return Err("`argv` is required and names the program first".to_owned());
                }
                self.operations.run(&argv)
            }
            other => Err(format!("`{other}` is not an operation this build performs")),
        }
    }

    fn no_such(&self, name: &str) -> String {
        format!(
            "`{name}` is not a tool this run has. Available: {}.",
            self.entries
                .iter()
                .map(|entry| entry.name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn reading() -> Envelope {
    Envelope {
        effects: vec![Effect::Read, Effect::Filesystem],
        risk: Risk::Low,
        idempotency: Idempotency::Idempotent,
        access: vec![AccessKind::Filesystem],
    }
}

fn writing(idempotency: Idempotency) -> Envelope {
    Envelope {
        effects: vec![Effect::Write, Effect::Filesystem],
        risk: Risk::Medium,
        idempotency,
        access: vec![AccessKind::Filesystem],
    }
}

fn file_read() -> Entry {
    Entry {
        operation: "file.read",
        name: "file_read",
        summary:
            "Read one text file. The reply says whether it was truncated, so a partial read is \
                  never mistaken for a whole file."
                .to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File relative to the workspace root."},
                "max_bytes": {"type": "integer", "description": "Byte ceiling for this read."},
            },
            "required": ["path"],
            "additionalProperties": false,
        }),
        envelope: reading(),
    }
}

fn dir_list() -> Entry {
    Entry {
        operation: "dir.list",
        name: "dir_list",
        summary: "List one directory. Paths are relative to the workspace root; `.` is the root."
            .to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory relative to the workspace root."},
            },
            "additionalProperties": false,
        }),
        envelope: reading(),
    }
}

fn search() -> Entry {
    Entry {
        operation: "search",
        name: "search",
        summary:
            "Find a literal substring in the workspace's text files. Not a regular expression. \
                  Build output and version-control directories are skipped."
                .to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Literal substring to find."},
                "path": {"type": "string", "description": "Directory or file to search."},
                "max_results": {"type": "integer", "description": "Ceiling on returned matches."},
            },
            "required": ["pattern"],
            "additionalProperties": false,
        }),
        envelope: reading(),
    }
}

fn file_write() -> Entry {
    Entry {
        operation: "file.write",
        name: "file_write",
        summary:
            "Write one file, whole. Creates it if it is not there. Replacing an existing file \
                  replaces all of it — use `file_edit` to change part of one."
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
        // Writing the same bytes twice leaves the same file.
        envelope: writing(Idempotency::Idempotent),
    }
}

fn file_edit() -> Entry {
    Entry {
        operation: "file.edit",
        name: "file_edit",
        summary:
            "Replace one exact piece of text in one file. The text must appear exactly once; a \
                  replacement that matched nothing, or several places, is refused rather than \
                  guessed at."
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
        // Running it twice is not running it once: the second attempt finds nothing to replace, and
        // under a workflow that retreats and re-runs a whole scope that is exactly what happens.
        envelope: writing(Idempotency::NonIdempotent),
    }
}

fn run(programs: &[String]) -> Entry {
    Entry {
        operation: "shell",
        name: "run",
        summary: format!(
            "Run one program and answer its output and exit status. This is not a shell: `argv` is \
             a list, nothing is composed, redirected or substituted, and only these programs may be \
             named — {}.",
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
                    // The model reads the set instead of guessing at it and being refused.
                    "prefixItems": [{"enum": programs}],
                },
            },
            "required": ["argv"],
            "additionalProperties": false,
        }),
        envelope: Envelope {
            effects: vec![Effect::Process],
            risk: Risk::High,
            idempotency: Idempotency::Conditional,
            access: vec![AccessKind::Process, AccessKind::Filesystem],
        },
    }
}

/// Every entry's name, for a caller that wants the vocabulary without a provider.
pub fn entry_names() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("file.read", "file_read"),
        ("file.write", "file_write"),
        ("file.edit", "file_edit"),
        ("dir.list", "dir_list"),
        ("search", "search"),
        ("shell", "run"),
    ])
}

/// The neutral operation one entry is, without a provider or a live catalogue.
///
/// # Why a reader needs this and `Entry::operation` will not do
///
/// A run is judged **after** it happened, from its record. What the record carries for an owned
/// surface is the verb the model called and the entry it named inside it — `tool_invoke` with
/// `{"name": "file_write", …}` — and the catalogue that answered is long gone. So the mapping has
/// to be a fact about the vocabulary rather than a property of one process's provider, or every
/// consumer downstream would have to keep its own copy and the copies would drift. That drift is
/// the exact failure this whole tool surface exists to remove.
///
/// Derived from [`entry_names`] rather than written out again, so the two cannot disagree.
#[must_use]
pub fn operation_of(entry: &str) -> Option<&'static str> {
    entry_names()
        .into_iter()
        .find_map(|(operation, name)| (name == entry).then_some(operation))
}
