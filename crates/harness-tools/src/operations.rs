//! The seven things a tool can be, performed by somebody else.
//!
//! # Why the catalogue does not perform anything
//!
//! `harness-tools` depends on `harness-wire`, `serde` and nothing else — no substrate, no tokio, no
//! sibling repository. That is deliberate: metaharness embeds this crate to serve the same tools to
//! Claude Code over MCP, and a workspace that links no async runtime cannot afford a catalogue that
//! drags one in.
//!
//! So the catalogue holds *what a tool is* and this trait holds *who does it*. There are two
//! implementations today and they differ in exactly one property:
//!
//! | | confinement | who asked |
//! |---|---|---|
//! | [`LocalOperations`](crate::LocalOperations) | none: the process's own filesystem, bounded by path checks this crate makes | nobody — there is no boundary to name a subject at |
//! | `harness-substrate`'s | the driver's: guarded IO, `openat2` containment, cgroups and namespaces around an exec | nobody embedded, a peer-credential subject over a socket |
//!
//! A third — substrate over a socket — is the same trait again, and the tools cannot tell any of
//! them apart. Which one a run holds is a deployment decision, not a different set of things the
//! model may do.

use std::time::Duration;

use harness_wire::Refusal;
use serde_json::Value;

/// Which part of a file one read answers with.
///
/// # Why a window and not a byte ceiling
///
/// `file_read` used to take a byte ceiling and nothing else, so it read from byte 0 every time: a
/// file over the ceiling could never be seen whole, and the middle of any file cost a read of
/// everything before it. Lines are the addressing every editor, every stack trace and every diff
/// already uses, and they are what `file_edit` is quoted against — so a window is stated in them.
///
/// Named fields rather than three `Option<u64>` in a row: `(None, Some(40), Some(200))` is a
/// silent transposition away from `(Some(40), Some(200), None)`, and both compile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReadWindow {
    /// The first line to answer with, counting from 1. [`None`] is line 1.
    pub offset: Option<u64>,
    /// How many lines to answer with. [`None`] is every line that fits under `max_bytes`.
    pub limit: Option<u64>,
    /// How many bytes of the file this read may take. [`None`] is the provider's own default, and
    /// every provider caps whatever is asked for at its own ceiling.
    pub max_bytes: Option<u64>,
}

impl ReadWindow {
    /// The window a caller who wants the file means: from its first line, as much as fits.
    #[must_use]
    pub fn whole() -> Self {
        Self::default()
    }

    /// `limit` lines from `offset`, counting from 1.
    #[must_use]
    pub fn lines(offset: u64, limit: u64) -> Self {
        Self {
            offset: Some(offset),
            limit: Some(limit),
            max_bytes: None,
        }
    }
}

/// How a search is narrowed, beyond the pattern and where it starts.
///
/// Every field's default is the search this crate has always performed — a literal substring, every
/// text file under the path, the matching line and nothing around it. So a caller that wants that
/// passes [`SearchOptions::default()`] and reads exactly as it did before.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchOptions {
    /// Read the pattern as a regular expression rather than as a literal substring.
    ///
    /// Off by default, because a model that meant `a.b` almost always meant `a.b`. A pattern that
    /// does not compile is refused with the regex crate's own words: a search that quietly matched
    /// nothing would read as *this string is not in the tree*, which is a different answer.
    pub regex: bool,
    /// Only files whose workspace-relative path matches this glob — `*.rs`, `crates/**/*.rs`.
    pub glob: Option<String>,
    /// How many lines either side of a match to answer with.
    pub context: Option<u64>,
    /// A ceiling on returned matches, itself capped by the provider's own.
    pub max_results: Option<usize>,
}

/// What an operation answers when it did not do the thing: a sentence, and sometimes a name for it.
///
/// # Why this is not a plain `String`, and not a typed error either
///
/// The `Err` of an operation is a **sentence the model reads** — a tool that failed has to say so
/// in words the next turn can act on — and this crate deliberately never replaced that with an
/// error enum, because the enum would have to be rendered into a sentence anyway and the two would
/// drift.
///
/// That argument holds for *failures* and fails for one *refusal*. `run` refusing a program outside
/// the declared set is not the tool failing; it is the run's own rule saying no, and it is the
/// thing an evaluation asks about by name. Left as prose it reached the record as
/// `ToolCompleted { failed: true }` — the shape of a compile error — and the only way to count it
/// downstream was to match the sentence.
///
/// So the sentence stays and the name rides beside it. [`Refusal::message`] is where the words are
/// written, so the tag and the prose are one string with one author rather than two descriptions of
/// one decision. A provider with nothing to name writes `Err("…".to_owned().into())` and nothing
/// changes for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Refused {
    message: String,
    refusal: Option<Refusal>,
}

impl Refused {
    /// The words the model reads.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Which rule of the run's own refused this, when it was a rule and not a failure.
    #[must_use]
    pub fn refusal(&self) -> Option<&Refusal> {
        self.refusal.as_ref()
    }

    /// The message, given away.
    #[must_use]
    pub fn into_message(self) -> String {
        self.message
    }

    /// The two halves, given away together.
    #[must_use]
    pub fn into_parts(self) -> (String, Option<Refusal>) {
        (self.message, self.refusal)
    }
}

impl From<String> for Refused {
    fn from(message: String) -> Self {
        Self {
            message,
            refusal: None,
        }
    }
}

impl From<&str> for Refused {
    fn from(message: &str) -> Self {
        Self::from(message.to_owned())
    }
}

/// A named refusal, saying itself in its own words.
impl From<Refusal> for Refused {
    fn from(refusal: Refusal) -> Self {
        Self {
            message: refusal.message(),
            refusal: Some(refusal),
        }
    }
}

impl std::fmt::Display for Refused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Refused {}

/// What performs a catalogue entry.
///
/// Every reading and writing method answers `Result<Value, String>`: the `Err` is a sentence the
/// model reads, because a tool that failed has to say so in words the next turn can act on. A typed
/// error there would have to be rendered into one anyway, and the two would drift.
///
/// [`run`](Operations::run) is the exception and [`Refused`] carries the reason: execution is the
/// one operation with a **declared set** a call can fall outside of, and that refusal is a fact
/// about the run rather than a failure of the tool. It keeps its sentence and gains a name.
///
/// # `Send + Sync`, and what it bought
///
/// A turn that asks for six reads used to pay six round trips of tool latency for no reason: the
/// calls are pure, they were already published and already inside every bound, and nothing about a
/// read requires the next one to wait for it. [`Catalogue::invoke_batch`](crate::Catalogue) runs
/// them on one thread each under `std::thread::scope`, which needs the provider to be shareable
/// across threads. Both implementations already were; a third that is not has to say so by holding
/// its unshareable part behind a lock, which is a thing to write down rather than a thing to
/// discover under a race.
pub trait Operations: Send + Sync {
    /// Read a window of one text file.
    ///
    /// # Errors
    ///
    /// The path could not be read, is not a file, is outside the workspace, or the window names
    /// lines the file does not have.
    fn file_read(&self, path: &str, window: ReadWindow) -> Result<Value, String>;

    /// Write one file, whole.
    ///
    /// # Errors
    ///
    /// The path could not be written, or is outside the workspace.
    fn file_write(&self, path: &str, text: &str) -> Result<Value, String>;

    /// Replace one exact piece of text in one file.
    ///
    /// The one-match rule lives in the implementation rather than here, because *what counts as one
    /// place* is a judgement about the file's bytes and the two implementations read those bytes
    /// through different doors.
    ///
    /// # Errors
    ///
    /// The text appears no times or several, or the file could not be read or written.
    fn file_edit(&self, path: &str, old: &str, new: &str) -> Result<Value, String>;

    /// List one directory.
    ///
    /// # Errors
    ///
    /// The path is not a directory, or is outside the workspace.
    fn dir_list(&self, path: &str) -> Result<Value, String>;

    /// Find a pattern in the tree's text files.
    ///
    /// Literal by default; [`SearchOptions::regex`] makes it a regular expression, and the two
    /// other fields narrow which files are read and how much of each match is answered.
    ///
    /// # Errors
    ///
    /// The pattern is empty, does not compile as a regular expression, the glob does not compile,
    /// or the path is outside the workspace.
    fn search(&self, pattern: &str, path: &str, options: &SearchOptions) -> Result<Value, String>;

    /// Every file under `path` whose workspace-relative name matches a glob.
    ///
    /// The tool that was missing: without it, finding a file cost one `dir_list` per directory
    /// level, and a model that wanted `*.rs` had to guess where they were.
    ///
    /// Defaulted to a refusal naming it, so a provider written before this existed keeps compiling
    /// and answers the model in words rather than by pretending the tree holds nothing.
    ///
    /// **This one is reachable from the model, unlike the defaults below.** `find`, `dir_list` and
    /// `search` are the three reading entries [`Catalogue::of`](crate::Catalogue::of) publishes
    /// whatever the provider says, because the provider a run holds for them is normally not the
    /// one it holds for its effects: the confined provider refuses all three by name and the CLI
    /// composes [`Split`] so the local reader answers them. A run that hands the catalogue a bare
    /// confined provider gets three published entries whose calls come back as this refusal — in
    /// words, on the call, which is the outcome invariant 9 asks for and not a silence.
    ///
    /// # Errors
    ///
    /// The glob does not compile, the path is outside the workspace, or this provider does not
    /// offer it.
    fn find(&self, glob: &str, path: &str, max_results: Option<usize>) -> Result<Value, String> {
        let _ = (glob, path, max_results);
        Err("`find` is not offered by this workspace".to_owned())
    }

    /// Run one program and answer what it did.
    ///
    /// **An argv, never a command line.** Nothing here builds a string a shell would take apart,
    /// and an implementation that has no way to confine a process answers
    /// [`Unavailable`](Self::unavailable) rather than shelling out unconfined.
    ///
    /// # Errors
    ///
    /// The program is not one this run may start — [`Refusal::ProgramNotDeclared`], which is the
    /// one refusal here that is named rather than only worded — or it could not be launched.
    fn run(&self, argv: &[String]) -> Result<Value, Refused>;

    /// [`run`](Self::run), told how much of the run's wall-clock budget is left.
    ///
    /// A program is the one thing here that outlives the call that started it by minutes, and
    /// the loop's deadline check between calls cannot reach into it. An implementation that
    /// starts a process bounds it by the smaller of its own ceiling and `remaining`, and says in
    /// the result that it did. `None` is a run with no deadline.
    ///
    /// Defaulted to the unbounded [`run`](Self::run), so an implementation that has nothing to
    /// bound needs nothing more — but one that starts a process without honouring this is one a
    /// deadline cannot stop.
    ///
    /// # Errors
    ///
    /// As [`run`](Self::run).
    fn run_within(&self, argv: &[String], remaining: Option<Duration>) -> Result<Value, Refused> {
        let _ = remaining;
        self.run(argv)
    }

    /// Where a write to `path` would land, relative to the workspace root, with every link followed.
    ///
    /// The write scope is matched against the path as the caller spelled it, and a link inside
    /// the workspace is a second spelling of its target that no lexical check can see: under
    /// `target/**=denied`, a write to `ok/link` that points at `target/x` overwrote `target/x`.
    /// The catalogue puts this answer through the scope as well, so the rule sees where the bytes
    /// go and not only what the call said.
    ///
    /// Defaulted to the path as written, which is exactly right for a provider whose writes never
    /// follow a link — substrate's guarded filesystem resolves with `RESOLVE_NO_SYMLINKS` — and
    /// for one that has no tree to look at. A provider that *does* follow links on the way to a
    /// write has to answer here, or its scope is a spelling check.
    ///
    /// # Errors
    ///
    /// The path leaves the workspace or leads nowhere it could write. The catalogue does not act
    /// on the error: the write itself refuses the same path with the same words, and one answer
    /// to one question is the point.
    fn lands(&self, path: &str) -> Result<String, String> {
        Ok(path.to_owned())
    }

    /// The programs [`run`](Self::run) will accept.
    ///
    /// Empty means execution is not offered at all, and the catalogue leaves the entry out — so the
    /// model is never told about a tool it cannot have and never spends a turn being refused.
    fn programs(&self) -> &[String] {
        &[]
    }

    /// Whether this implementation can change anything at all.
    ///
    /// `false` for a read-only provider, and the catalogue leaves out every writing entry when it
    /// is. One question in one place, so two callers cannot disagree about which entries exist.
    fn writes(&self) -> bool {
        false
    }

    /// The refusal an implementation answers for something it does not offer.
    ///
    /// A sentence rather than a silence: a tool that answered nothing would look to the model like
    /// one that had worked.
    fn unavailable(what: &str) -> String
    where
        Self: Sized,
    {
        format!("`{what}` is not offered by this workspace")
    }
}

/// Reads from one provider, effects from another.
///
/// # The composition the two real providers force
///
/// Neither implementation is complete on its own and neither should be. `LocalOperations` reads a
/// tree and refuses everything that outlives a call, because it has no boundary to put one behind.
/// `harness-substrate`'s provider writes and executes inside a confined workspace and has no
/// listing or search route at all — `Backend` does not carry one, and reading the host filesystem
/// to fake it would step around the containment it exists for.
///
/// So a run that reads a tree *and* changes it holds both. This is that, and it is deliberately not
/// a merge: which provider answers is decided per operation, once, here — not by trying one and
/// falling through to the other, which would make a refusal from the first look like a route to the
/// second.
pub struct Split {
    reads: Box<dyn Operations>,
    effects: Box<dyn Operations>,
}

impl Split {
    /// Reads through `reads`, writes and execution through `effects`.
    pub fn new(reads: impl Operations + 'static, effects: impl Operations + 'static) -> Self {
        Self {
            reads: Box::new(reads),
            effects: Box::new(effects),
        }
    }
}

impl Operations for Split {
    fn file_read(&self, path: &str, window: ReadWindow) -> Result<Value, String> {
        self.reads.file_read(path, window)
    }

    fn dir_list(&self, path: &str) -> Result<Value, String> {
        self.reads.dir_list(path)
    }

    fn search(&self, pattern: &str, path: &str, options: &SearchOptions) -> Result<Value, String> {
        self.reads.search(pattern, path, options)
    }

    fn find(&self, glob: &str, path: &str, max_results: Option<usize>) -> Result<Value, String> {
        self.reads.find(glob, path, max_results)
    }

    fn file_write(&self, path: &str, text: &str) -> Result<Value, String> {
        self.effects.file_write(path, text)
    }

    fn file_edit(&self, path: &str, old: &str, new: &str) -> Result<Value, String> {
        self.effects.file_edit(path, old, new)
    }

    fn run(&self, argv: &[String]) -> Result<Value, Refused> {
        self.effects.run(argv)
    }

    fn run_within(&self, argv: &[String], remaining: Option<Duration>) -> Result<Value, Refused> {
        self.effects.run_within(argv, remaining)
    }

    /// The provider that performs the write is the one that knows where it lands.
    fn lands(&self, path: &str) -> Result<String, String> {
        self.effects.lands(path)
    }

    fn programs(&self) -> &[String] {
        self.effects.programs()
    }

    fn writes(&self) -> bool {
        self.effects.writes()
    }
}
