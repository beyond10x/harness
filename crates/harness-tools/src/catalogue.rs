//! What a run may do, as a list somebody can read.
//!
//! # Entries are named by neutral operations
//!
//! `file.read`, `file.write`, `file.edit`, `dir.list`, `search`, `find`, `shell` — metaharness's own
//! vocabulary (`metaharness_protocol::Operation`), not spelled out as a dependency but matched name
//! for name. That is the point of this crate: a consumer asks *did this run write a file* without
//! knowing whether the harness spells a write `Write`, `workspace_write` or `apply_patch`.
//!
//! # The catalogue is built from what the machine admits
//!
//! An entry is here because something can perform it. A provider that cannot write contributes no
//! writing entry; a machine with no delegated cgroup contributes no `run`. So
//! [`Catalogue::search`] answering four entries rather than seven is the publication gate speaking,
//! not a filter — the model is never told about a tool it cannot have.

use std::collections::BTreeMap;

use harness_wire::{AccessKind, Effect, Envelope, Idempotency, Risk, Subject, ToolName, ToolSpec};
use serde_json::{Value, json};

use crate::{Operations, Refused};

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
    /// Where the **step** now running may write, on top of `scope`.
    ///
    /// Empty for every run that is not a walk, and between the steps of one. Both layers are
    /// asked and a write either refuses is refused, which is what makes a step's declaration
    /// narrowing-only: see [`Catalogue::narrow`].
    step: crate::Scope,
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
    /// Writing entries appear when the provider [`writes`](Operations::writes); `run` appears when
    /// it names at least one program. A provider that can confine execution but was given no
    /// declared set publishes no `run`: a workflow that named no commands wants none, and a tool
    /// that admitted everything because nobody listed anything is the failure this whole design
    /// exists to prevent.
    ///
    /// **The four reading entries are published whatever the provider answers**, and that is a
    /// decision rather than an oversight. `dir_list`, `search` and `find` have no route through a
    /// confined workspace — `Backend` carries no way to walk one — so the confined provider refuses
    /// all three by name, and a run that reads a tree holds [`Split`](crate::Split) with the local
    /// reader on that side, which is what the CLI composes. Gating publication on the provider
    /// would take the three entries away from every `Split` whose *effects* half cannot walk, which
    /// is all of them. A catalogue built on a bare confined provider therefore does publish three
    /// entries whose calls come back as a refusal naming them — an outcome the model reads, not a
    /// silence.
    pub fn of(operations: impl Operations + 'static) -> Self {
        let mut entries = vec![file_read(), dir_list(), search(), find()];
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
            step: crate::Scope::default(),
        }
    }

    /// The same catalogue, restricted to where this run may write.
    ///
    /// Declared before the run starts and never changed during it: a boundary a run could widen is
    /// not a boundary. Every entry stays published — the model is told what exists and refused per
    /// call with a reason naming the way in, rather than handed a shorter list it cannot ask about.
    ///
    /// A **step** of a walk may lay a second, narrower layer over this one for the length of its
    /// turn ([`narrow`](Self::narrow)); it can only take writing away.
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

    /// Narrows this catalogue to what the step now running declared, replacing the last step's.
    ///
    /// # A step may only narrow, never widen
    ///
    /// The run's own scope is not replaced and not consulted second: **both layers are asked and
    /// the first refusal wins**, so a node that says `allowed` where the run says `denied` changes
    /// nothing at all. That is structural rather than checked — there is no arrangement of rules a
    /// document can write that gives back what the command line took away, which is the property
    /// that lets a projection generated by another component be trusted with this at all.
    ///
    /// [`crate::Scope::default`] puts the catalogue back under the run's scope alone, which is
    /// what a step declaring no scope runs under: a document that says nothing does not silently
    /// narrow a run.
    pub fn narrow(&mut self, step: crate::Scope) {
        self.step = step;
    }

    /// Where the step now running may write, on top of [`scope`](Self::scope).
    #[must_use]
    pub fn step_scope(&self) -> &crate::Scope {
        &self.step
    }

    /// Why this operation may not touch this path under **either** layer, or [`None`] if it may.
    ///
    /// The run's answer first, so the refusal a person reads names the boundary they set when both
    /// of them refuse.
    fn refusal(&self, operation: &str, path: &str) -> Option<String> {
        self.scope
            .refusal(operation, path)
            .or_else(|| self.step.refusal(operation, path))
    }

    /// Whether anything at all was declared, by the run or by the step now running.
    fn restricted(&self) -> bool {
        !(self.scope.is_empty() && self.step.is_empty())
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

    /// Every entry's own name, which is what a call names and what a narrowing names.
    ///
    /// # Panics
    ///
    /// As [`Entry::spec`]: only if a constant entry name stops being a legal tool name.
    pub fn names(&self) -> Vec<ToolName> {
        self.entries
            .iter()
            .map(|entry| {
                ToolName::new(entry.name).expect("a constant entry name is a legal tool name")
            })
            .collect()
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
    ///
    /// [`Refused`] rather than a bare sentence, so a refusal the run made by its own rule — a
    /// program outside the declared set — reaches the caller as a fact and not only as prose.
    pub fn invoke(&self, name: &str, arguments: &Value) -> Result<Value, Refused> {
        self.invoke_within(name, arguments, None)
    }

    /// [`invoke`](Self::invoke), told how much of the run's wall-clock budget is left.
    ///
    /// Only `run` has anything to bound; the figure is handed to
    /// [`Operations::run_within`] and every other entry ignores it.
    ///
    /// # Errors
    ///
    /// As [`invoke`](Self::invoke).
    pub fn invoke_within(
        &self,
        name: &str,
        arguments: &Value,
        remaining: Option<std::time::Duration>,
    ) -> Result<Value, Refused> {
        let entry = self.get(name).ok_or_else(|| self.no_such(name))?;
        // **Refused here, by the tool, because here is where this loop's policy lives.** Every
        // other arm adjudicates at a decision seam; this one has none and never grows one, so a
        // tool that must not act on a path refuses on that path exactly as `run` refuses a program
        // nobody declared. Before the operation runs, so a refusal costs nothing but a turn.
        if let Some(path) = arguments.get("path").and_then(Value::as_str) {
            if let Some(refusal) = self.refusal(entry.operation, path) {
                return Err(refusal.into());
            }
            // And by where it lands, not only by how it was spelled. The scope is lexical; a link
            // inside the workspace is a spelling it cannot see, and `ok/link -> target/x` used to
            // walk a write past `target/**=denied`. Only for a write under a declared scope: a
            // read is never the scope's business, and an empty scope restricts nothing.
            if self.restricted()
                && matches!(entry.operation, "file.write" | "file.edit")
                && let Ok(landing) = self.operations.lands(path)
                && landing != path
                && let Some(refusal) = self.refusal(entry.operation, &landing)
            {
                return Err(format!("`{path}` leads to `{landing}`, and {refusal}").into());
            }
        }
        let string = |field: &str| -> Result<&str, String> {
            arguments
                .get(field)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("`{field}` is required by `{name}`"))
        };
        let path = || string("path");
        let under = |field: &str| {
            arguments
                .get(field)
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
        };
        // Every arm but `shell` answers a sentence and nothing more; `shell` is the one that can
        // name what it refused, so it is the one that is not converted.
        match entry.operation {
            "file.read" => self
                .operations
                .file_read(
                    path()?,
                    crate::ReadWindow {
                        offset: arguments.get("offset").and_then(Value::as_u64),
                        limit: arguments.get("limit").and_then(Value::as_u64),
                        max_bytes: arguments.get("max_bytes").and_then(Value::as_u64),
                    },
                )
                .map_err(Refused::from),
            "file.write" => self
                .operations
                .file_write(path()?, string("text")?)
                .map_err(Refused::from),
            "file.edit" => self
                .operations
                .file_edit(path()?, string("old")?, string("new")?)
                .map_err(Refused::from),
            "dir.list" => self
                .operations
                .dir_list(arguments.get("path").and_then(Value::as_str).unwrap_or("."))
                .map_err(Refused::from),
            "search" => self
                .operations
                .search(
                    string("pattern")?,
                    arguments.get("path").and_then(Value::as_str).unwrap_or("."),
                    &crate::SearchOptions {
                        regex: arguments
                            .get("regex")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        glob: arguments
                            .get("glob")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        context: arguments.get("context").and_then(Value::as_u64),
                        max_results: under("max_results"),
                    },
                )
                .map_err(Refused::from),
            "find" => self
                .operations
                .find(
                    string("glob")?,
                    arguments.get("path").and_then(Value::as_str).unwrap_or("."),
                    under("max_results"),
                )
                .map_err(Refused::from),
            "shell" => {
                let items = arguments
                    .get("argv")
                    .and_then(Value::as_array)
                    .filter(|items| !items.is_empty())
                    .ok_or_else(|| {
                        Refused::from("`argv` is required and names the program first")
                    })?;
                // Every item, or nothing. Dropping a non-string item would run a command nobody
                // asked for — `["cargo", 5, "test"]` is not `cargo test`, it is a mistake the model
                // has to hear about.
                let mut argv = Vec::with_capacity(items.len());
                for (index, item) in items.iter().enumerate() {
                    let Some(text) = item.as_str() else {
                        return Err(Refused::from(format!(
                            "`argv[{index}]` is {item}, not a string; every item of an argv is a \
                             string, and nothing was run"
                        )));
                    };
                    argv.push(text.to_owned());
                }
                self.operations.run_within(&argv, remaining)
            }
            other => Err(format!("`{other}` is not an operation this build performs").into()),
        }
    }

    /// Perform several entries at once, one answer per call, in the order they were given.
    ///
    /// # What this buys, and who is allowed to ask for it
    ///
    /// A turn that asks for six reads paid six round trips of tool latency when they ran one after
    /// another, and nothing about a read requires that. Each call gets a thread —
    /// `std::thread::scope`, so nothing outlives this function and nothing is spawned that the
    /// caller has to join — and a turn asks for a handful, not a thousand.
    ///
    /// **This does not check that the calls are safe to run side by side, and it never will.** The
    /// loop decides: it hands over only calls whose invoked envelope does not mutate, which is a
    /// question about `Envelope` that the loop already has to answer for approval and that this
    /// catalogue would have to answer a second, drifting way. Handed a `file_write` and a
    /// `file_edit` of one file, this runs both and the file gets whichever landed last — exactly
    /// as it would if a caller called `invoke_within` twice from two threads of its own.
    ///
    /// A call whose thread panics answers a refusal naming the entry rather than taking the
    /// process with it: the other five have already done their work, and the model needs to hear
    /// which one did not.
    ///
    /// **The panic is caught inside the thread, not at the join**, and that is not a detail.
    /// `std::thread::scope` re-panics on its own thread if *any* scoped thread panicked, whether or
    /// not it was joined first — so a caught-at-the-join refusal was unreachable and one bad call
    /// took the other five answers, and this function's caller, with it. `catch_unwind` around each
    /// body means the scope never sees a panicked thread. It is safe code; `unsafe_code` stays
    /// forbidden.
    ///
    /// # Threads are chunked, because a turn is not always a handful
    ///
    /// The loop hands over the whole batchable prefix of a turn, and a model that opens with two
    /// hundred reads used to get two hundred OS threads and two hundred concurrent tree scans. The
    /// batch is run `MAX_BATCH_THREADS` at a time, each chunk under its own scope, and the
    /// answers concatenated in the order the calls came in.
    #[must_use]
    pub fn invoke_batch(
        &self,
        calls: &[(&str, &Value)],
        remaining: Option<std::time::Duration>,
    ) -> Vec<Result<Value, Refused>> {
        let mut answers = Vec::with_capacity(calls.len());
        for chunk in calls.chunks(MAX_BATCH_THREADS) {
            std::thread::scope(|scope| {
                let running: Vec<_> = chunk
                    .iter()
                    .map(|(name, arguments)| {
                        scope.spawn(move || {
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                self.invoke_within(name, arguments, remaining)
                            }))
                            .unwrap_or_else(|payload| {
                                Err(Refused::from(format!(
                                    "`{name}` panicked while running: {}. Nothing else in this \
                                     batch was affected, and whether it did anything before it \
                                     stopped is unknown.",
                                    panic_words(payload.as_ref())
                                )))
                            })
                        })
                    })
                    .collect();
                for (handle, (name, _)) in running.into_iter().zip(chunk) {
                    answers.push(handle.join().unwrap_or_else(|_| {
                        Err(Refused::from(format!(
                            "`{name}` did not finish: the thread running it stopped without an \
                             answer, so whether it did anything is unknown"
                        )))
                    }));
                }
            });
        }
        answers
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

/// What the model reads: a failure is an outcome, never an error, or the next turn assumes the
/// effect landed.
///
/// The one place a [`Refused`] becomes a [`ToolOutcome`], for both published surfaces — the three
/// verbs and the flat one. A refusal the run made by rule keeps its name here; every other failure
/// is words and only words, which is what it always was.
pub(crate) fn outcome(result: Result<Value, Refused>) -> harness_wire::ToolOutcome {
    match result {
        Ok(output) => harness_wire::ToolOutcome::ok(output),
        Err(refused) => match refused.into_parts() {
            (_, Some(refusal)) => harness_wire::ToolOutcome::refused(refusal),
            (message, None) => harness_wire::ToolOutcome::failed(message),
        },
    }
}

/// How many of a batch's calls run side by side.
///
/// The loop hands over every batchable call of a turn at once, and one OS thread each is fine for
/// the six reads that motivated batching and wrong for the two hundred a model can ask for: two
/// hundred threads, each walking a tree, is a way to spend a machine rather than a turn. Eight is
/// more than the parallelism a handful of reads needs and small enough that the largest batch costs
/// the same eight threads as the smallest one that fills them.
const MAX_BATCH_THREADS: usize = 8;

/// What a caught panic said, where it said anything a reader can act on.
///
/// A payload is `Box<dyn Any>` and only the two ordinary shapes carry words: `panic!("…")` leaves a
/// `&str`, a formatted one leaves a `String`. Anything else is named as what it is rather than
/// dressed up — a message invented for it would read as the panic's own.
fn panic_words(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "it carried no message this build can read".to_owned()
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
        summary: "Read a window of one text file, as numbered lines. Each line comes back as its \
                  number, right-aligned in six columns, then a tab, then the line — the number is \
                  this reply's, not the file's, so strip it before quoting text to `file_edit`. \
                  `lines` says which window you got and how many lines the file has, and \
                  `truncated` says the window stops before the file's last line or that a line in \
                  it was cut. Any part of any file is reachable by moving `offset` — except where \
                  `lines.total` comes back `null`, which means the reply could not establish how \
                  many lines there are and says in `note` why. Read that before assuming."
            .to_owned(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File relative to the workspace root."},
                "offset": {"type": "integer", "minimum": 1, "description": "First line to read, counting from 1. Default 1."},
                "limit": {"type": "integer", "minimum": 1, "description": "How many lines to read. Default: as many as fit under the byte ceiling."},
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
        summary: format!(
            "Find text in the workspace's files. A literal substring by default; set `regex` for a \
             regular expression. `glob` narrows which files are read and `context` answers the \
             lines either side of each match. Two things are never searched and the reply cannot \
             tell you afterwards: these directories and everything under them — {} — and anything \
             more than {} directories below where the search started, which the reply reports as \
             `depth_bound_reached`. Start the search lower down to reach past either.",
            crate::local::SKIPPED.join(", "),
            crate::local::MAX_GREP_DEPTH,
        ),
        input_schema: json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "What to find: a literal substring, or a regular expression when `regex` is true."},
                "path": {"type": "string", "description": "Directory or file to search."},
                "regex": {"type": "boolean", "description": "Read `pattern` as a regular expression. Default false. One that does not compile is refused, never silently matched against nothing."},
                "glob": {"type": "string", "description": "Only files matching this glob — `*.rs` is that name anywhere in the tree, `crates/**/*.rs` is the whole path."},
                "context": {"type": "integer", "minimum": 0, "maximum": 5, "description": "Lines either side of each match, under `before` and `after` with their own numbers. Default 0, capped at 5."},
                "max_results": {"type": "integer", "description": "Ceiling on returned matches."},
            },
            "required": ["pattern"],
            "additionalProperties": false,
        }),
        envelope: reading(),
    }
}

fn find() -> Entry {
    Entry {
        operation: "find",
        name: "find",
        summary: format!(
            "List the files whose path matches a glob under the workspace. `*.rs` is that name at \
             any depth; `crates/**/*.rs` matches the whole workspace-relative path. Use this \
             instead of walking the tree one `dir_list` at a time. Two places it does not look, \
             and an empty list will not tell you which: these directories and everything under \
             them — {} — and anything more than {} directories below where the walk started, which \
             the reply reports as `depth_bound_reached`. Set `path` lower down to reach past \
             either.",
            crate::local::SKIPPED.join(", "),
            crate::local::MAX_GREP_DEPTH,
        ),
        input_schema: json!({
            "type": "object",
            "properties": {
                "glob": {"type": "string", "description": "The glob to match. Without a `/` it matches the file's own name at any depth; with one it matches the workspace-relative path and `*` does not cross a `/`."},
                "path": {"type": "string", "description": "Directory to search under. Default the workspace root."},
                "max_results": {"type": "integer", "description": "Ceiling on returned paths."},
            },
            "required": ["glob"],
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
        // **A retry declaration, and no longer an approval one.** Running it twice is not running
        // it once: the second attempt finds nothing to replace, and under a workflow that retreats
        // and re-runs a whole scope that is exactly what happens — which is what a scheduler reads
        // this field for.
        //
        // Until 2026-08-29 `Envelope::needs_approval` also asked about every non-idempotent
        // mutation whatever the ceiling, and the consequence at the command line was backwards:
        // `--approve-up-to high` let `run` and a whole-file `file_write` through unasked and
        // stopped to ask about every `file_edit`, pushing an unattended run toward rewriting files
        // whole when the narrower edit was the safer act. The clause is gone; risk alone decides,
        // and `file_edit` is asked about at exactly the ceiling `file_write` is.
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
        ("find", "find"),
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
