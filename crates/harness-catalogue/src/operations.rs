//! The seven things a tool can be, performed by somebody else.
//!
//! # Why the catalogue does not perform anything
//!
//! `harness-catalogue` depends on `harness-wire`, the `tools` crates and `serde` — no substrate,
//! no tokio, no async runtime. That is deliberate: metaharness embeds this
//! crate to serve the same tools to Claude Code over MCP, and a workspace that links no async
//! runtime cannot afford a catalogue that drags one in.
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

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::time::Duration;

use harness_wire::Refusal;
use serde_json::{Map, Value, json};
use tool_toolchain::executor::{Group, Plan};
use tool_toolchain::{RunOutput, RunRequest};

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

    /// The directory on this host that [`run_within`](Self::run_within) starts a program in, if
    /// this process can look at one.
    ///
    /// A toolchain call is judged before anything starts, and part of the judgement is where the
    /// path arguments it names land: `tool_toolchain::executor` resolves them under this
    /// directory, links followed, refuses one that lands outside it, and puts the landing of a
    /// written path its plan declares through the write scope. A catalogue's toolchain entries
    /// need it for that, and nothing else reads it.
    ///
    /// Defaulted to [`None`], which is a provider that names no such directory — substrate's
    /// confined workspace is an identifier on the other side of the driver — and a toolchain call
    /// through one is refused with nothing started: a path nobody can resolve is one the write
    /// scope cannot be asked about where it lands. [`LocalOperations`](crate::LocalOperations)
    /// answers its root, and [`Split`] its effecting half's, or else its reading half's.
    fn host_root(&self) -> Option<&Path> {
        None
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

    /// The effecting half's directory, or the reading half's where it names none.
    ///
    /// The composition `harness-cli` builds is one tree: substrate adopts the directory the local
    /// reader reads, and the confined provider names no host directory of its own, so the reader's
    /// root is where a confined program's paths land.
    fn host_root(&self) -> Option<&Path> {
        self.effects.host_root().or_else(|| self.reads.host_root())
    }

    fn programs(&self) -> &[String] {
        self.effects.programs()
    }

    fn writes(&self) -> bool {
        self.effects.writes()
    }
}

/// [`Operations::run_within`] as a `tool_toolchain` [`Runner`](tool_toolchain::Runner): the seam
/// through which every argv of a toolchain call reaches the provider a catalogue holds.
///
/// # One executor, local and confined
///
/// `tool_toolchain::executor` makes the checks of the `build-test` contract — the arguments,
/// where the path arguments the call names land, the write scope on the written paths its plan
/// declares, the declared programs, one deadline for the whole call — and hands each argv that
/// passes them to its runner. This is that runner, and all
/// it does is call the provider: [`LocalOperations`](crate::LocalOperations) starts the program
/// through `b10x-tool-process`, and `harness-substrate`'s confined provider through substrate,
/// inside the sandbox its driver was opened with — the providers' declared environment and
/// read-only roots, applied to this exec as to every other. The declared sandbox the executor
/// hands over with each argv ([`RunRequest::sandbox`]) is therefore not read here: the confined
/// provider applies the same declaration already, and the local one has nowhere to apply it.
///
/// # Each run answers the provider's own reply
///
/// The executor shapes a call's answer — one run, `runs`, or `toolchains` by provider — out of the
/// [`RunOutput`] a runner returns, and reads nothing of it but `stdout`, for the `empty-stdout`
/// check. A provider's reply is a [`Value`] of its own fields: the local provider's is a
/// `RunOutput` field for field, and the confined one's is substrate's observation, which says what
/// confinement was applied and has no place in a `RunOutput`. So each reply is kept, in the order
/// the runs started, and [`answered`](Self::answered) puts it where the executor put its run.
pub(crate) struct ProviderRunner<'a> {
    operations: &'a dyn Operations,
    root: &'a Path,
    /// Each run's reply, with the provider whose tool planned it, in the order they ran.
    replies: RefCell<Vec<(String, Value)>>,
    /// The provider's last refusal, whole. The executor carries a runner's refusal as its
    /// sentence alone, and a provider's [`Refusal`] is a name a reader of the record counts.
    refused: RefCell<Option<Refused>>,
}

impl<'a> ProviderRunner<'a> {
    /// Every argv through `operations`, in `root`: the directory it names on this host
    /// ([`Operations::host_root`]).
    pub(crate) fn new(operations: &'a dyn Operations, root: &'a Path) -> Self {
        Self {
            operations,
            root,
            replies: RefCell::new(Vec::new()),
            refused: RefCell::new(None),
        }
    }

    /// The executor's answer to `plan`, each run in it replaced by the provider's reply for that
    /// run, every provider the plan names present in it, and a provider the deadline cut short
    /// saying so.
    ///
    /// `answer` is [`None`] where the executor answered no output at all, which is a plan of one
    /// provider with no process to start: a command run once per item of an empty list.
    ///
    /// # What the executor's answer leaves out, and this puts back
    ///
    /// The executor keeps a provider in a role's `toolchains` only when one of its processes ran,
    /// so a provider with no process to start, and one the deadline passed before it started,
    /// went missing, and an answer without them reads as a role they were never part of. Here
    /// each planned provider has its key: an empty `{"runs": []}` for one with nothing to start,
    /// and the cut-short marker for one that did not start ([`completed`]).
    pub(crate) fn answered(&self, answer: Option<Value>, plan: &Plan) -> Value {
        let mut replies: BTreeMap<String, VecDeque<Value>> = BTreeMap::new();
        for (provider, reply) in self.replies.take() {
            replies.entry(provider).or_default().push_back(reply);
        }
        let [group] = plan.groups.as_slice() else {
            let mut executed = answer
                .and_then(|mut answer| {
                    answer
                        .get_mut("toolchains")
                        .and_then(Value::as_object_mut)
                        .map(std::mem::take)
                })
                .unwrap_or_default();
            let mut toolchains = Map::new();
            for group in &plan.groups {
                let ran = replies.remove(&group.provider).unwrap_or_default();
                let answer = completed(executed.remove(&group.provider), ran, group);
                toolchains.insert(group.provider.clone(), answer);
            }
            return json!({ "toolchains": toolchains });
        };
        let ran = replies.remove(&group.provider).unwrap_or_default();
        completed(answer, ran, group)
    }

    /// What the executor refused with `message`: the provider's own refusal, its name kept, when
    /// the sentence is the provider's, and the sentence alone when the executor wrote it.
    pub(crate) fn refusal(&self, message: String) -> Refused {
        match self.refused.take() {
            Some(refused) if refused.message() == message => refused,
            _ => Refused::from(message),
        }
    }
}

/// One provider's answer: the executor's with the provider's own replies in it, or an empty list
/// of runs where it answered none — and, where the call's deadline passed before every process
/// the provider planned had started, an answer that says so.
///
/// # A list the deadline cut short says it was cut (invariant 8)
///
/// The shared deadline stops a plan between processes, and the executor then answers the runs
/// that started and nothing about the rest, so two runs of three read exactly like a plan of two.
/// A provider with planned processes that did not start answers `runs` (those that did, possibly
/// none), `deadline_passed: true`, `not_started` (how many did not) and a `note` saying it in
/// words, whatever shape its tool answers when whole: the marker is the shape, so a cut-short
/// answer cannot be read as a complete one. Nothing but the deadline leaves a planned process
/// unstarted in an answer; a refusal and a failed check end the call as an error.
fn completed(executed: Option<Value>, mut ran: VecDeque<Value>, group: &Group) -> Value {
    let planned = group.plan.argv.len();
    let started = ran.len();
    let mut answer = match executed {
        Some(mut answer) => {
            restore(&mut answer, &mut ran);
            answer
        }
        None => json!({ "runs": Vec::from(ran) }),
    };
    if started < planned {
        let not_started = planned - started;
        let mut cut = match answer {
            Value::Object(object) if object.contains_key("runs") => object,
            run => {
                let mut object = Map::new();
                object.insert("runs".to_owned(), Value::Array(vec![run]));
                object
            }
        };
        cut.insert("deadline_passed".to_owned(), Value::Bool(true));
        cut.insert("not_started".to_owned(), json!(not_started));
        cut.insert(
            "note".to_owned(),
            Value::String(format!(
                "the call's deadline passed before {not_started} of the {planned} processes `{}` \
                 planned could start; `runs` holds the {started} that did",
                group.provider
            )),
        );
        answer = Value::Object(cut);
    }
    answer
}

/// One provider's replies, put where the executor answered its runs.
fn restore(group: &mut Value, replies: &mut VecDeque<Value>) {
    if let Some(runs) = group.get_mut("runs").and_then(Value::as_array_mut) {
        for run in runs {
            if let Some(reply) = replies.pop_front() {
                *run = reply;
            }
        }
    } else if let Some(reply) = replies.pop_front() {
        *group = reply;
    }
}

impl tool_toolchain::Runner for &ProviderRunner<'_> {
    type Error = Refused;

    fn root(&self) -> &Path {
        self.root
    }

    fn run(&self, request: RunRequest<'_>) -> Result<RunOutput, Refused> {
        let reply = self
            .operations
            .run_within(request.argv, request.time_left)
            .inspect_err(|refused| {
                *self.refused.borrow_mut() = Some(refused.clone());
            })?;
        // The executor reads `stdout` of this and nothing else; every other field of the answer is
        // the reply's own, put back by `answered`. A reply that is not a `RunOutput` is carried by
        // its `stdout`, and what is filled in beside it never reaches an answer.
        let output =
            serde_json::from_value::<RunOutput>(reply.clone()).unwrap_or_else(|_| RunOutput {
                argv: request.argv.to_vec(),
                exit: None,
                stdout: reply
                    .get("stdout")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                stderr: String::new(),
                truncated: false,
                omitted_bytes: 0,
                timed_out: false,
                timeout_ms: 0,
            });
        self.replies
            .borrow_mut()
            .push((request.provider.to_owned(), reply));
        Ok(output)
    }
}
