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
        subjects_of_operation(self.operation, arguments)
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
    /// Where this run may write. Empty restricts nothing — see [`crate::Scope`].
    scope: crate::Scope,
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
            scope: crate::Scope::default(),
        }
    }

    /// The same catalogue, restricted to where this run may write.
    ///
    /// Declared before the run starts and never changed during it: a boundary a run could widen is
    /// not a boundary. Every entry stays published — the model is told what exists and refused per
    /// call with a reason naming the way in, rather than handed a shorter list it cannot ask about.
    #[must_use]
    pub fn scoped(mut self, scope: crate::Scope) -> Self {
        self.scope = scope;
        self
    }

    /// Where this run may write.
    #[must_use]
    pub fn scope(&self) -> &crate::Scope {
        &self.scope
    }

    /// Every entry, in the order they were built.
    /// Every neutral operation this catalogue can perform.
    ///
    /// What a reader of the record wants when the tool list says `tool_search`, `tool_describe`,
    /// `tool_invoke` on every run: *what could this one actually do*. Derived from the entries the
    /// publication gate admitted, so it is a fact about the machine and never a claim about it.
    #[must_use]
    pub fn operations(&self) -> Vec<&'static str> {
        self.entries.iter().map(|entry| entry.operation).collect()
    }

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

    /// The whole catalogue as prose a standing instruction can carry.
    ///
    /// # Why the model should not have to ask
    ///
    /// The three verbs make discovery a *turn*: `tool_search` to learn what exists, then
    /// `tool_describe` per entry to learn how to call it, and only then the work. Measured across
    /// three live runs, **33% to 44% of every tool call was one of those two** — four calls of ten
    /// spent finding out, each one a full model round trip that is billed, replayed in every later
    /// turn, and adds nothing to the tree.
    ///
    /// It also puts the catalogue in the wrong half of the request. A `tool_search` answer lands in
    /// the **conversation**, which grows and is re-sent at the full rate; this puts it in the
    /// **instructions**, which are the same bytes on every turn and are what a prompt cache is able
    /// to hold. The constant head of a request was about 450 tokens — under the 1,024-token minimum
    /// a cache entry needs — and this is what lifts it over.
    ///
    /// The verbs stay. This is not a flat surface by the back door: `tool_invoke` is still the only
    /// way to act, `tool_search` still answers a filtered question about a catalogue that may be
    /// larger than this text, and a run may still describe an entry it wants to re-check. What
    /// changes is that none of that is *required* before the first useful call.
    ///
    /// Rendered from the live catalogue, so it cannot describe a tool this run does not have — the
    /// failure a hand-written instruction made twice.
    #[must_use]
    pub fn brief(&self) -> String {
        use std::fmt::Write;
        let mut text = String::new();
        for entry in &self.entries {
            let schema =
                serde_json::to_string(&entry.input_schema).unwrap_or_else(|_| "{}".to_owned());
            let _ = writeln!(
                text,
                "- `{}` ({}) — {}\n  arguments: {schema}",
                entry.name, entry.operation, entry.summary
            );
        }
        text
    }

    /// Perform one entry.
    ///
    /// # Errors
    ///
    /// The name is not one the catalogue holds — refused **here**, with nothing performed and
    /// nothing sent anywhere — or the operation itself failed, in which case the words are its own.
    pub fn invoke(&self, name: &str, arguments: &Value) -> Result<Value, String> {
        let entry = self.get(name).ok_or_else(|| self.no_such(name))?;
        // **Refused here, by the tool, because here is where this loop's policy lives.** Every
        // other arm adjudicates at a decision seam; this one has none and never grows one, so a
        // tool that must not act on a path refuses on that path exactly as `run` refuses a program
        // nobody declared. Before the operation runs, so a refusal costs nothing but a turn.
        if let Some(path) = arguments.get("path").and_then(Value::as_str)
            && let Some(refusal) = self.scope.refusal(entry.operation, path)
        {
            return Err(refusal);
        }
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
                let items = arguments
                    .get("argv")
                    .and_then(Value::as_array)
                    .filter(|items| !items.is_empty())
                    .ok_or_else(|| "`argv` is required and names the program first".to_owned())?;
                // Every item, or nothing. Dropping a non-string item would run a command nobody
                // asked for — `["cargo", 5, "test"]` is not `cargo test`, it is a mistake the model
                // has to hear about.
                let mut argv = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let Some(text) = item.as_str() else {
                        return Err(format!(
                            "`argv[{index}]` is {item}, not a string; every item of an argv is a \
                             string, and nothing was run"
                        ));
                    };
                    argv.push(text.to_owned());
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

/// The concrete things one entry's call would touch, without a catalogue to ask.
///
/// # Why a reader needs this and [`Entry::subjects`] will not do
///
/// The same argument [`operation_of`] carries. A run is judged **after** it happened, from its
/// record: the catalogue that answered is gone, and what is left is the entry's name and the
/// arguments it was called with. A consumer that kept its own copy of this rule is a copy that
/// drifts, and the drift is the exact failure this vocabulary exists to remove.
///
/// Empty for an entry outside the vocabulary — it reached no tool, so it touched nothing.
#[must_use]
pub fn subjects_of(entry: &str, arguments: &Value) -> Vec<Subject> {
    operation_of(entry)
        .map(|operation| subjects_of_operation(operation, arguments))
        .unwrap_or_default()
}

/// The one rule, shared by the live catalogue and by a reader of a finished run.
///
/// The path is reported **as the caller wrote it**. A gate has to see `../../etc/passwd` as the
/// model sent it; the tidy answer canonicalisation would give is exactly wrong for the one call
/// whose whole problem is where it was going.
fn subjects_of_operation(operation: &str, arguments: &Value) -> Vec<Subject> {
    match operation {
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
