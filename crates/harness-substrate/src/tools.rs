//! The operations that exist only where the machine can confine them.
//!
//! # Publication is the first gate, and it is the quiet one
//!
//! What this provider *admits* is computed from [`Facts`], once, and
//! `harness_tools::Catalogue::of` turns that into which entries exist. On a machine with no
//! delegated cgroup root there is no `run` entry at all — the model is never told about a tool it
//! cannot have, never plans around one, and never spends a turn being refused. On a machine with no
//! substrate backend there are no writing entries either, and the harness is exactly the read-only
//! thing it has always been.
//!
//! # `run`, and deliberately not `bash`
//!
//! An open shell is unbounded by construction. `sh -c` composes, redirects and substitutes, so the
//! subject of the call is not knowable before it runs — and a subject nobody can compute is one
//! nobody can authorize, which collapses the middle gate into nothing.
//!
//! `run` takes an argv and a program from a **declared set**. The set is in the entry's own schema,
//! so the model can read what it may run instead of guessing and being refused; a program outside
//! it is refused by name, listing the set — **here**, with nothing sent to the daemon.
//!
//! Substrate reaches the same place from the other side: `exec.start`'s first capability predicate
//! is `exec.argv-only`. Neither component will run a shell, and neither had to be told by the other.

use std::fmt::Write as _;
use std::time::Duration;

use harness_tools::{Operations, ReadWindow, Refusal, Refused, SearchOptions};
use serde_json::{Value, json};

use crate::{Backend, Facts, Withheld};

/// What a confined workspace can do on this machine.
pub struct ConfinedOperations {
    /// **`Send + Sync`, because the catalogue runs a turn's pure reads side by side.**
    /// `harness_tools::Catalogue::invoke_batch` gives each call a thread, so the provider behind it
    /// is shared across them. Both backends already were — [`Client`](crate::Client) holds a
    /// `Box<dyn Transport + Send + Sync>` and opens a connection per call, and
    /// [`Embedded`](crate::Embedded) holds the driver in an `Arc` behind a runtime — so this is a
    /// bound written down rather than a change to either.
    backend: Box<dyn Backend + Send + Sync>,
    workspace: String,
    programs: Vec<String>,
    writes: bool,
    /// What was declared here and this machine did not admit, with the predicate that decided.
    ///
    /// Held on the provider because the provider is the one place that saw both halves — what the
    /// caller declared and what the machine said — and because the catalogue built from it can no
    /// longer tell: an entry that was never published and one that was never wanted are the same
    /// absence downstream. Empty on a machine that admits what it was asked for.
    withheld: Vec<Withheld>,
    /// How many bytes the read route answers with before it stops — the machine's own
    /// `workspace.read-limit-bytes` where it states one, and [`substrate_wire::MAX_IO_BYTES`]
    /// otherwise.
    ///
    /// Read off [`Facts`] rather than assumed, because both backends ask for exactly this figure:
    /// [`Client`](crate::Client) puts the fact in its query and the embedded driver asks for
    /// `MAX_IO_BYTES`, which is what `HostConfig::minimum` sets its own read limit to and therefore
    /// what its probe reports.
    read_ceiling_bytes: u64,
}

impl std::fmt::Debug for ConfinedOperations {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfinedOperations")
            .field("workspace", &self.workspace)
            .field("programs", &self.programs)
            .field("writes", &self.writes)
            .field("withheld", &self.withheld)
            .finish_non_exhaustive()
    }
}

impl ConfinedOperations {
    /// The provider this machine admits.
    ///
    /// `programs` is the declared set `run` may name. An empty set offers no execution even on a
    /// machine that could confine it: a workflow that named no commands wants none, and a tool that
    /// admitted everything because nobody listed anything is the failure this design exists to
    /// prevent.
    ///
    /// **A declared program set this machine cannot confine is recorded, not only dropped**
    /// ([`Self::withheld`]). Constructing this at all is asking for a confined workspace, so the
    /// writing entries count as declared too and their absence is recorded the same way.
    pub fn new(
        backend: impl Backend + Send + Sync + 'static,
        facts: &Facts,
        workspace: impl Into<String>,
        programs: Vec<String>,
    ) -> Self {
        let confines_execution = facts.confines_execution();
        Self {
            backend: Box::new(backend),
            workspace: workspace.into(),
            withheld: facts.withheld(&programs, true),
            // A machine that cannot confine a process offers none, whatever was declared.
            programs: if confines_execution {
                programs
            } else {
                Vec::new()
            },
            writes: facts.holds_workspaces(),
            read_ceiling_bytes: facts
                .get("workspace.read-limit-bytes")
                .and_then(Value::as_u64)
                .unwrap_or(substrate_wire::MAX_IO_BYTES),
        }
    }

    /// What this run declared that the machine would not admit.
    ///
    /// The counterpart to [`Operations::programs`] and [`Operations::writes`]: those say what the
    /// catalogue publishes, and this says what it does **not**, for a caller that has to put the
    /// run's shape in front of a person. Empty is the ordinary answer and means exactly nothing was
    /// withheld — never that nobody looked.
    #[must_use]
    pub fn withheld(&self) -> &[Withheld] {
        &self.withheld
    }
}

/// How much of a file one read answers with unless the caller asks for less.
///
/// The same figure the unconfined provider uses, so a run's replies do not change shape when it is
/// confined.
const MAX_READ_BYTES: u64 = 64 * 1024;

/// The most a caller may ask for in one read, however large a number it names.
const MAX_READ_BYTES_CEILING: u64 = 256 * 1024;

/// How much of one line a read answers with.
///
/// The same figure the unconfined provider uses, for the same reason the byte ceilings match: a
/// run's replies must not change shape when it is confined.
const MAX_READ_LINE_CHARS: usize = 2_000;

impl Operations for ConfinedOperations {
    /// The same window, the same numbered lines, over what the wire route can reach.
    ///
    /// # What the route offers, and what that costs here
    ///
    /// [`Backend::file_read`] answers **from byte 0 up to a ceiling** — `workspace.read-limit-bytes`
    /// where the machine states one, `substrate_wire::MAX_IO_BYTES` otherwise — and answers a
    /// `String`, so there is no offset to seek with. The line window is therefore applied to the
    /// bytes the ceiling let through, and a window that starts past the last line those bytes hold
    /// is **refused by name, saying which line the read reached**. Silently answering nothing would
    /// look exactly like a file with no such lines, which is the failure invariant 8 exists to stop.
    ///
    /// # When the ceiling cut the file, this reply says so and stops counting
    ///
    /// It did not, and that was the same failure wearing the other hat. A 3 MiB file read at
    /// `offset: 24500, limit: 500` answered `{"truncated": false, "lines": {"to": 25000, "total":
    /// 25000}}` — the prefix's last line read as the file's last line, the prefix's line count read
    /// as the file's, and a model that had seen a third of the file was told it had seen all of it.
    ///
    /// So when the route returned as many bytes as its ceiling allows, this reply answers
    /// `truncated: true`, `lines.total: null` — the count is not knowable from a prefix, and a
    /// number that counted only the prefix is worse than no number — and names the ceiling in
    /// `route_ceiling_bytes` with a `note` saying that lines past it cannot be reached on this
    /// path, whatever `offset` says. `bytes` is `null` for the same reason, with the bytes that
    /// *were* read under `bytes_read`.
    ///
    /// **"As many bytes as the ceiling allows" is the test, so a file of exactly the ceiling reads
    /// as cut.** The route's own answer carries `eof` (`substrate_wire::FileSlice`), which would be
    /// exact; [`Backend::file_read`] hands back a `String` and nothing else, so carrying it is a
    /// change to that trait and both its implementations. Erring towards *cut* is the safe half of
    /// the mistake: it never claims a file was read whole when it was not.
    ///
    /// The bound is also why the reply is bounded at all: a result is replayed on **every** later
    /// turn. A live run on 2026-08-24 read three files in one turn and the next turn's replay grew
    /// by 24,630 tokens, which pushed the conversation past its bound and bought a prefix rewrite.
    fn file_read(&self, path: &str, window: ReadWindow) -> Result<Value, String> {
        let ceiling = window
            .max_bytes
            .unwrap_or(MAX_READ_BYTES)
            .min(MAX_READ_BYTES_CEILING);
        let offset = window.offset.unwrap_or(1);
        if offset == 0 {
            return Err(format!(
                "`offset` is the first line to read and lines are numbered from 1, so 0 names no \
                 line. `{path}` was not read."
            ));
        }
        let whole = self
            .backend
            .file_read(&self.workspace, path)
            .map_err(|error| error.to_string())?;
        let bytes = whole.len() as u64;
        // The route answered its whole ceiling, so what came back is the front of something larger
        // — or a file of exactly that size, which is answered the same way for want of the route's
        // own `eof`. Anything shorter is the file.
        let ceiling_cut = bytes >= self.read_ceiling_bytes;
        let lines: Vec<&str> = whole.lines().collect();
        let total = lines.len() as u64;
        if offset > 1 && offset > total {
            return Err(format!(
                "this confined read of `{path}` reaches line {total} — the route answers from the \
                 start of the file up to a byte ceiling of {} bytes, and that is where it stopped. \
                 `offset` names line {offset}, past it, so nothing was read.",
                self.read_ceiling_bytes
            ));
        }

        let mut text = String::new();
        let mut cut = Vec::new();
        let mut answered_bytes: u64 = 0;
        let mut kept: u64 = 0;
        let mut last: u64 = 0;
        for (index, line) in lines.iter().enumerate().skip(
            usize::try_from(offset - 1)
                .unwrap_or(usize::MAX)
                .min(lines.len()),
        ) {
            let number = index as u64 + 1;
            let weight = line.len() as u64 + 1;
            let within_limit = window.limit.is_none_or(|count| kept < count);
            let within_ceiling = kept == 0 || answered_bytes + weight <= ceiling;
            if !(within_limit && within_ceiling) {
                break;
            }
            let shown: String = line.chars().take(MAX_READ_LINE_CHARS).collect();
            if line.chars().count() > MAX_READ_LINE_CHARS {
                cut.push(number);
            }
            let _ = writeln!(text, "{number:>6}\t{shown}");
            kept += 1;
            answered_bytes += weight;
            last = number;
        }

        let mut answer = json!({
            "path": path,
            // Absence as absence: past the ceiling this read cannot see the file's size, and the
            // prefix's size under this name would be read as the file's.
            "bytes": if ceiling_cut { Value::Null } else { json!(bytes) },
            "truncated": ceiling_cut || last < total || !cut.is_empty(),
            "text": text,
            "lines": {
                "from": offset,
                "to": if kept == 0 { offset.saturating_sub(1) } else { last },
                "total": if ceiling_cut { Value::Null } else { json!(total) },
            },
            "truncated_lines": cut,
        });
        if ceiling_cut {
            answer["bytes_read"] = json!(bytes);
            answer["route_ceiling_bytes"] = json!(self.read_ceiling_bytes);
            answer["note"] = json!(format!(
                "this workspace's read route answers the first {} bytes of a file and no more, and \
                 `{path}` filled them, so how many lines it has is not knowable from here and the \
                 lines past that ceiling cannot be reached on this path at any `offset`. A tool \
                 that walks the tree — `search`, `find` — reads it through the other provider.",
                self.read_ceiling_bytes
            ));
        }
        Ok(answer)
    }

    /// One file, whole, through the confined route — unless this workspace said it changes nothing.
    ///
    /// # Why the check is here as well as in the catalogue
    ///
    /// [`Operations::writes`] is the single question `harness_tools::Catalogue::of` asks to decide
    /// which entries exist, so a model can never reach this on a workspace that answered `false`.
    /// An **embedder** can: the trait is public and the unconfined provider has always answered a
    /// caller who went around the catalogue rather than serving one. Until this check existed, the
    /// same `writes() == false` meant *refused* through one implementation and *written* through
    /// the other, which made the one question two answers — the thing the trait's own note about
    /// `writes` ("one question in one place") exists to prevent.
    ///
    /// In a real deployment the two agree on the outcome and disagree only about who says so: a
    /// `false` here comes from `Facts::holds_workspaces`, and a daemon that serves no workspaces
    /// would refuse the write itself. This makes the refusal the provider's own, in the sentence
    /// [`Operations::unavailable`] writes for every implementation.
    fn file_write(&self, path: &str, text: &str) -> Result<Value, String> {
        if !self.writes {
            return Err(Self::unavailable("file_write"));
        }
        self.backend
            .file_write(&self.workspace, path, text)
            .map_err(|error| error.to_string())
    }

    /// Read the file, replace one place in it, write it back.
    ///
    /// **A file the read route could not answer whole is refused rather than edited.** This writes
    /// what it read, so a file whose first ceiling of bytes is all that came back would have been
    /// written back at that length — an edit that silently deleted everything past the ceiling.
    /// The same ceiling test [`file_read`](Self::file_read) uses, and the same erring towards *cut*.
    ///
    /// Refused before anything is read where this workspace changes nothing, for the reason
    /// [`file_write`](Self::file_write) gives.
    fn file_edit(&self, path: &str, old: &str, new: &str) -> Result<Value, String> {
        if !self.writes {
            return Err(Self::unavailable("file_edit"));
        }
        let current = self
            .backend
            .file_read(&self.workspace, path)
            .map_err(|error| error.to_string())?;
        if current.len() as u64 >= self.read_ceiling_bytes {
            return Err(format!(
                "`{path}` filled this workspace's {} byte read ceiling, so an edit here would \
                 write back only what was read and drop whatever is past it. Nothing was changed.",
                self.read_ceiling_bytes
            ));
        }
        // Neither *none* nor *several* is an edit. A replacement that hit nothing leaves the model
        // believing a change landed, and one that hit four places changed three things nobody asked
        // about — which is why this is checked here rather than left to a `replace` call.
        match current.matches(old).count() {
            0 => {
                return Err(format!(
                    "`{path}` does not contain that text, so nothing was changed"
                ));
            }
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

    fn dir_list(&self, _path: &str) -> Result<Value, String> {
        // Not served through the backend today: `Backend` has no listing route, and inventing one
        // by reading the host filesystem would step around the very containment this provider is
        // for. A run that needs a listing gets it from the read-only provider beside this one.
        Err(Self::unavailable("dir_list through a confined workspace"))
    }

    fn search(&self, _p: &str, _path: &str, _options: &SearchOptions) -> Result<Value, String> {
        Err(Self::unavailable("search through a confined workspace"))
    }

    /// The same absence as `dir_list` and `search`, and for the same reason.
    ///
    /// `Backend` carries no way to walk a workspace, and walking the host filesystem to answer
    /// would step around the containment this provider exists for. A run that needs to find a file
    /// gets it from the reading provider beside this one, which is what `harness_tools::Split`
    /// composes.
    fn find(&self, _glob: &str, _path: &str, _max: Option<usize>) -> Result<Value, String> {
        Err(Self::unavailable("find through a confined workspace"))
    }

    fn run(&self, argv: &[String]) -> Result<Value, Refused> {
        self.run_within(argv, None)
    }

    fn run_within(&self, argv: &[String], remaining: Option<Duration>) -> Result<Value, Refused> {
        // The catalogue refuses an empty argv before it gets here, but this is a public trait
        // method and an embedder can call it directly; a refusal by name is what the unconfined
        // provider answers, and a panic mid-turn is not.
        let Some(program) = argv.first() else {
            return Err("`argv` must name a program".into());
        };
        if !self.programs.iter().any(|allowed| allowed == program) {
            // The same named refusal the unconfined provider answers, in the same words — the
            // sentence has one author, [`Refusal::message`], so the two providers cannot drift.
            return Err(Refusal::ProgramNotDeclared {
                program: program.clone(),
                declared: self.programs.clone(),
            }
            .into());
        }
        self.backend
            .exec(&self.workspace, argv, remaining)
            .map_err(|error| Refused::from(error.to_string()))
    }

    fn programs(&self) -> &[String] {
        &self.programs
    }

    fn writes(&self) -> bool {
        self.writes
    }
}
