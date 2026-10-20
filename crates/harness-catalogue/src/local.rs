//! A bounded view of one directory, with no confinement under it, over the `tools` crates.
//!
//! # The provider with no boundary, and why it still exists
//!
//! Reading a tree is [`tool_fs`]'s, searching it [`tool_search`]'s and writing one is
//! [`tool_code_editor`]'s: every path check is that crate's own, canonicalising before a
//! containment check. That is real and it is not confinement — nothing stops the *process*, only
//! those functions, and a bug in them is the whole boundary. `run` stays here: it moves onto its
//! process toolset (`b10x-tool-process`) in `story:run-moves-onto-tool-process`.
//!
//! It is the right provider for a run against the operator's own tree on their own machine, which
//! is every run this component has ever had. A run that wants the effects confined passes
//! `harness-substrate`'s provider instead, and the catalogue cannot tell the difference.
//!
//! **By default it writes nothing and runs nothing.** [`LocalOperations::new`] leaves
//! [`Operations::writes`] `false` and [`Operations::programs`] empty, so a catalogue built on it
//! holds four entries: read, list, search, find.
//!
//! # …and the door out of that, which has to be asked for by name
//!
//! [`LocalOperations::unconfined`] turns on writing and a declared program set. It exists because
//! metaharness serves this same catalogue to a vendor harness over MCP, on the operator's own
//! machine, where the confinement story is a scratch home rather than a cgroup.
//!
//! What it does *not* do is pretend: nothing here confines the process, the bound on an effect is
//! the tools crate's own arithmetic on paths, and the constructor is named so that a reader of the
//! call site knows it.
//!
//! # The name mapping, tested both ways
//!
//! Each harness operation is routed to its `tools` operation directly, in code, by the method that
//! performs it. [`MAPPING`] is the **declared** correspondence beside that — the recorded operation
//! name stays what it was (`file.read`, `shell`) and model-facing names do not change.
//! [`tools_operation`] and [`harness_operation`] read it in each direction, and the suite below
//! checks the round trip and that every id is an operation the tools crates actually expose, so the
//! declaration cannot drift from what the adapter calls.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tool_code_editor::{EditInput, Editor, WriteInput};
use tool_fs::{Fs, GlobInput, ListInput, ReadInput};
use tool_search::{GrepInput, Search};

use crate::{Operations, ReadWindow, Refusal, Refused, SearchOptions};

const MAX_RUN_OUTPUT_BYTES: usize = 64 * 1024;
/// How long an unconfined `run` may hold the turn open before it is killed.
///
/// A bound rather than none: `wait` on a child that never exits stops the run with no record of
/// why. Ten minutes is longer than any suite this has been pointed at and shorter than a night.
const MAX_RUN_SECONDS: u64 = 600;

/// The environment variables a declared program is started with, unless a caller names more.
///
/// An allow list rather than a deny list, because the parent process's environment is where this
/// run's credentials live and the child's arguments came from the model. Naming what may cross
/// keeps a token nobody thought about out of a program somebody else chose.
///
/// The toolchain names are here because the first list did not have them and `cargo` under a
/// relocated rustup then started with `HOME` and `PATH` alone, looked in `~/.rustup`, and failed
/// with *no default toolchain configured* — a build failure that read as the workspace's. None of
/// them carries a secret: each is a path. A proxy URL can carry one, so `HTTPS_PROXY` and its kin
/// are not here; a caller that needs them names them with [`LocalOperations::inheriting`].
const INHERITED_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "LANG",
    "LC_ALL",
    "TERM",
    "TMPDIR",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "CARGO_TARGET_DIR",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// The **declared** harness-name↔tools-id correspondence: `(harness, tools)`.
///
/// Not the router — each `Operations` method below calls its tools operation directly, by name, in
/// code. This table is what a consumer and the two-way test read instead: [`tools_operation`] and
/// [`harness_operation`] answer it in each direction, and `every_mapped_id_is_an_operation_the_tools_crates_expose`
/// is what keeps it honest against the tools crates, so the declaration cannot drift from what the
/// adapter actually calls.
///
/// `run` is **not** here — it has not moved onto `b10x-tool-process` yet, so it stays local. The
/// recorded operation name (`file.read`, `shell`) is a third thing and is not this table;
/// metaharness reads that back, and renaming it is not renaming this.
pub(crate) const MAPPING: &[(&str, &str)] = &[
    ("file_read", "fs.read"),
    ("dir_list", "fs.list"),
    ("search", "search.grep"),
    ("find", "fs.glob"),
    ("file_write", "code.write"),
    ("file_edit", "code.edit"),
];

/// The `tools` operation id a harness operation is performed by, or `None` for one that stays local
/// (`run`).
#[must_use]
pub fn tools_operation(harness: &str) -> Option<&'static str> {
    MAPPING
        .iter()
        .find(|(name, _)| *name == harness)
        .map(|(_, id)| *id)
}

/// The harness operation a `tools` operation id answers, the inverse of [`tools_operation`].
#[must_use]
pub fn harness_operation(tools: &str) -> Option<&'static str> {
    MAPPING
        .iter()
        .find(|(_, id)| *id == tools)
        .map(|(name, _)| *name)
}

/// A bounded view of one directory, read through [`tool_fs`], searched through [`tool_search`] and
/// written through [`tool_code_editor`], with `run` performed here.
#[derive(Debug, Clone)]
pub struct LocalOperations {
    root: PathBuf,
    /// Reads: `fs.read`, `fs.list`, `fs.glob`.
    fs: Fs,
    /// Search: `search.grep`.
    search: Search,
    /// Writes: `code.write`, `code.edit`. Held whether or not writing is offered; whether it is
    /// reachable is `programs`, exactly as the catalogue's gate reads it, so holding it publishes
    /// nothing.
    editor: Editor,
    /// `None` is read-only. `Some` is the declared program set, which may itself be empty — a
    /// provider that writes files and starts nothing.
    programs: Option<Vec<String>>,
    /// Environment variables a declared program is started with, beyond [`INHERITED_ENV`].
    inherited: Vec<String>,
}

impl LocalOperations {
    /// Opens `root` as the only directory these tools can see, read-only.
    ///
    /// # Errors
    ///
    /// Returns a message when `root` is not a readable directory.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, String> {
        Self::open(root, None)
    }

    /// Names more of this process's environment that a declared program may see.
    ///
    /// The default list is what a toolchain needs and nothing that carries a credential. A run
    /// that needs a proxy, or a variable its build reads, names it here — by name, at the call
    /// site, where a reader can see what was let through.
    #[must_use]
    pub fn inheriting(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.inherited.extend(names.into_iter().map(Into::into));
        self
    }

    /// The same view, allowed to change it, with no confinement under the change.
    ///
    /// `programs` is what `run` will start, by exact `argv[0]`; an empty set publishes no `run`
    /// entry at all. Everything a caller gains here it gains on the strength of the tools crates'
    /// path arithmetic and nothing else — see the module documentation for what that is and is not.
    ///
    /// # Errors
    ///
    /// Returns a message when `root` is not a readable directory.
    pub fn unconfined(root: impl AsRef<Path>, programs: Vec<String>) -> Result<Self, String> {
        Self::open(root, Some(programs))
    }

    fn open(root: impl AsRef<Path>, programs: Option<Vec<String>>) -> Result<Self, String> {
        let fs = Fs::new(&root).map_err(|error| error.to_string())?;
        let search = Search::new(&root).map_err(|error| error.to_string())?;
        let editor = Editor::new(&root).map_err(|error| error.to_string())?;
        Ok(Self {
            root: fs.root().to_path_buf(),
            fs,
            search,
            editor,
            programs,
            inherited: Vec::new(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `fs.read` through [`tool_fs`], with the whole-file `sha256` stripped.
    ///
    /// The digest is a `tool-contract/1` property harness does not publish yet, and the substrate
    /// provider beside this one answers none, so the conformance suite that asks both for the same
    /// answer would break on its presence. Everything else of the reply is the tools crate's, field
    /// for field.
    fn read(&self, relative: &str, window: ReadWindow) -> Result<Value, String> {
        let input = ReadInput {
            path: relative.to_owned(),
            offset: window.offset,
            limit: window.limit,
            max_bytes: window.max_bytes,
        };
        let output = self.fs.read(&input).map_err(|error| error.to_string())?;
        let mut value = serde_json::to_value(output).expect("a read output serialises");
        if let Some(object) = value.as_object_mut() {
            object.remove("sha256");
        }
        Ok(value)
    }

    fn exec(
        &self,
        argv: &[String],
        programs: &[String],
        remaining: Option<Duration>,
    ) -> Result<Value, Refused> {
        let Some(program) = argv.first() else {
            return Err("`argv` must name a program".into());
        };
        if !programs.iter().any(|allowed| allowed == program) {
            // Named, not only worded. The sentence is [`Refusal::message`]'s and is what the model
            // reads; the value beside it is what makes *the run would not start this* countable on
            // the record instead of being one more failed call.
            return Err(Refusal::ProgramNotDeclared {
                program: program.clone(),
                declared: programs.to_vec(),
            }
            .into());
        }

        let mut command = std::process::Command::new(program);
        command
            .args(&argv[1..])
            .current_dir(&self.root)
            // Cleared, then filled from [`INHERITED_ENV`] alone. A child started here inherits the
            // whole of this process's environment otherwise, which is how a credential held for the
            // harness reaches a program the model picked the arguments for.
            .env_clear()
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt as _;
            // A private process group makes the timeout cover descendants that inherited the
            // output pipes, not only the shell directly returned by `spawn`.
            command.process_group(0);
        }
        // `var_os`, not `var`: a value that is not UTF-8 is still the value, and `var` would drop
        // it without a word.
        for name in INHERITED_ENV
            .iter()
            .copied()
            .chain(self.inherited.iter().map(String::as_str))
        {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let mut child = command
            .spawn()
            .map_err(|error| Refused::from(format!("`{program}`: {error}")))?;

        // Drained on threads rather than after `wait`: a child that fills a pipe buffer blocks
        // until somebody reads it, and a `wait` that has not read is the deadlock.
        let out = child.stdout.take().expect("stdout was piped");
        let err = child.stderr.take().expect("stderr was piped");
        let out = std::thread::spawn(move || drain(out));
        let err = std::thread::spawn(move || drain(err));

        // The smaller of this module's ceiling and what the run has left: the loop's deadline
        // check between calls cannot reach into this one, so the bound has to be set here.
        let ceiling = Duration::from_secs(MAX_RUN_SECONDS);
        let bound = remaining.map_or(ceiling, |left| left.min(ceiling));
        let deadline = Instant::now() + bound;
        let mut killed = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => return Err(format!("`{program}`: {error}").into()),
            }
            if Instant::now() >= deadline {
                killed = true;
                terminate(&mut child);
                break child
                    .wait()
                    .map_err(|error| Refused::from(format!("`{program}`: {error}")))?;
            }
            std::thread::sleep(Duration::from_millis(25));
        };

        let empty = || Drained {
            text: String::new(),
            omitted: 0,
        };
        let stdout = out.join().unwrap_or_else(|_| empty());
        let stderr = err.join().unwrap_or_else(|_| empty());
        let omitted = stdout.omitted + stderr.omitted;
        Ok(json!({
            "argv": argv,
            "exit": status.code(),
            "stdout": stdout.text,
            "stderr": stderr.text,
            "truncated": omitted > 0,
            // Across both streams. Each stream's own marker names its share, in the place the
            // bytes were dropped from, so a reader never has to work out which one was cut.
            "omitted_bytes": omitted,
            // Named, because a killed process's exit code says nothing about the task.
            "timed_out": killed,
            // And how long it was given, so a kill at the run's deadline is not read as the
            // program's own slowness.
            "timeout_ms": u64::try_from(bound.as_millis()).unwrap_or(u64::MAX),
        }))
    }
}

impl Operations for LocalOperations {
    fn file_read(&self, path: &str, window: ReadWindow) -> Result<Value, String> {
        self.read(path, window)
    }

    fn dir_list(&self, path: &str) -> Result<Value, String> {
        let input = ListInput {
            path: Some(path.to_owned()),
        };
        let output = self.fs.list(&input).map_err(|error| error.to_string())?;
        Ok(serde_json::to_value(output).expect("a list output serialises"))
    }

    /// `search.grep` through [`tool_search`], its reply field for field and its refusals in its own
    /// words.
    ///
    /// Nothing is stripped, unlike a read: every field `search.grep` answers is one harness's
    /// `search` answered before it moved, and the substrate provider beside this one performs no
    /// search to be compared with.
    fn search(&self, pattern: &str, path: &str, options: &SearchOptions) -> Result<Value, String> {
        let input = GrepInput {
            pattern: pattern.to_owned(),
            path: Some(path.to_owned()),
            regex: Some(options.regex),
            glob: options.glob.clone(),
            context: options.context,
            max_results: options
                .max_results
                .map(|count| u64::try_from(count).unwrap_or(u64::MAX)),
        };
        let output = self
            .search
            .grep(&input)
            .map_err(|error| error.to_string())?;
        Ok(serde_json::to_value(output).expect("a grep output serialises"))
    }

    fn find(&self, glob: &str, path: &str, max_results: Option<usize>) -> Result<Value, String> {
        let input = GlobInput {
            glob: glob.to_owned(),
            path: Some(path.to_owned()),
            max_results: max_results.map(|count| u64::try_from(count).unwrap_or(u64::MAX)),
        };
        let output = self.fs.glob(&input).map_err(|error| error.to_string())?;
        Ok(serde_json::to_value(output).expect("a glob output serialises"))
    }

    // The two below refuse unless the caller asked for them by name. The catalogue never offers
    // what `writes()` denies, so a model cannot reach a refusal here — they are for a caller who
    // went around the catalogue, who is answered rather than served.

    fn file_write(&self, path: &str, text: &str) -> Result<Value, String> {
        if self.programs.is_none() {
            return Err(Self::unavailable("file_write"));
        }
        let input = WriteInput {
            path: path.to_owned(),
            text: text.to_owned(),
            expected_sha256: None,
        };
        let output = self
            .editor
            .write(&input)
            .map_err(|error| error.to_string())?;
        let mut value = serde_json::to_value(output).expect("a write output serialises");
        // `sha256` is a `code-editor` property harness does not publish yet; the reply stays the
        // `{path, bytes}` it was.
        if let Some(object) = value.as_object_mut() {
            object.remove("sha256");
        }
        Ok(value)
    }

    fn file_edit(&self, path: &str, old: &str, new: &str) -> Result<Value, String> {
        if self.programs.is_none() {
            return Err(Self::unavailable("file_edit"));
        }
        let input = EditInput {
            path: path.to_owned(),
            old: old.to_owned(),
            new: new.to_owned(),
            expected_sha256: None,
        };
        let output = self
            .editor
            .edit(&input)
            .map_err(|error| error.to_string())?;
        let mut value = serde_json::to_value(output).expect("an edit output serialises");
        // `sha256` and the unified `diff` are `code-editor` properties harness does not publish
        // yet; the reply stays the `{path, bytes}` it was.
        if let Some(object) = value.as_object_mut() {
            object.remove("sha256");
            object.remove("diff");
        }
        Ok(value)
    }

    fn run(&self, argv: &[String]) -> Result<Value, Refused> {
        self.run_within(argv, None)
    }

    fn run_within(&self, argv: &[String], remaining: Option<Duration>) -> Result<Value, Refused> {
        let Some(programs) = self.programs.as_deref() else {
            return Err(Self::unavailable("run").into());
        };
        self.exec(argv, programs, remaining)
    }

    /// The same resolution a write does, answered as the workspace-relative name the write would
    /// report — so the scope judges the file the bytes reach, under the name it actually has.
    fn lands(&self, path: &str) -> Result<String, String> {
        self.editor.lands(path).map_err(|error| error.to_string())
    }

    fn programs(&self) -> &[String] {
        self.programs.as_deref().unwrap_or_default()
    }

    fn writes(&self) -> bool {
        self.programs.is_some()
    }
}

#[cfg(unix)]
fn terminate(child: &mut std::process::Child) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    if let Ok(pid) = i32::try_from(child.id()) {
        let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
    }
    let _ = child.kill();
}

#[cfg(not(unix))]
fn terminate(child: &mut std::process::Child) {
    let _ = child.kill();
}

/// What one of a child's streams said, bounded at both ends.
struct Drained {
    text: String,
    omitted: u64,
}

/// Reads a child's stream to the end, keeping the first and last half of
/// [`MAX_RUN_OUTPUT_BYTES`].
///
/// # The head is not where the answer is
///
/// This kept the **first** 64 KiB and dropped the rest. For the one program a run cares most about
/// — a test suite — the head is the compiler's progress and the answer is the last twenty lines:
/// `test result: FAILED. 3 passed; 1 failed`, and which test. A model handed the head learns that
/// something was compiled. So both ends are kept, and the middle says how many bytes went missing
/// between them.
///
/// It keeps reading after the cap rather than stopping: a reader that walked away would block the
/// child on a full pipe, and the point of the cap is the size of the answer, not the child's fate.
/// Memory stays at the cap — the tail is a ring, not a buffer of everything seen so far.
fn drain(stream: impl std::io::Read) -> Drained {
    let half = MAX_RUN_OUTPUT_BYTES / 2;
    let (head, tail, beyond_head) = read_ends(stream, half);
    keep_both_ends(&head, &tail, head.len() as u64 + beyond_head)
}

/// Reads `stream` to the end, keeping its first `half` bytes and its last `half` bytes.
///
/// The third answer is how many bytes came **after** the head — enough, with the two ends, to say
/// how many went missing between them.
fn read_ends(mut stream: impl std::io::Read, half: usize) -> (Vec<u8>, Vec<u8>, u64) {
    let mut head: Vec<u8> = Vec::new();
    let mut tail: std::collections::VecDeque<u8> = std::collections::VecDeque::new();
    let mut beyond_head: u64 = 0;
    let mut buffer = [0_u8; 8192];
    while let Ok(read) = stream.read(&mut buffer) {
        if read == 0 {
            break;
        }
        let mut bytes = &buffer[..read];
        let room = half.saturating_sub(head.len());
        if room > 0 {
            let taken = room.min(bytes.len());
            head.extend_from_slice(&bytes[..taken]);
            bytes = &bytes[taken..];
        }
        beyond_head += bytes.len() as u64;
        // A chunk at a time, then one drain: a program that prints a hundred megabytes would
        // otherwise pay a push and a pop per byte to keep a 32 KiB window.
        tail.extend(bytes.iter().copied());
        if tail.len() > half {
            tail.drain(..tail.len() - half);
        }
    }
    (head, tail.into_iter().collect(), beyond_head)
}

/// Joins the two ends of a stream of `total` bytes, with a marker naming what fell between them.
///
/// Its own function, and pure, because the arithmetic here is the part that can be wrong — the
/// character boundaries, the count in the marker, whether the last line survives — and the only
/// test of it drove a real child process, so it returned early on any machine without
/// `/usr/bin/seq` and checked nothing there at all.
///
/// `head` and `tail` are the two ends as [`read_ends`] kept them; `total` is the whole stream's
/// length. When the two ends are the whole stream, nothing is omitted and no marker is written.
fn keep_both_ends(head: &[u8], tail: &[u8], total: u64) -> Drained {
    if head.len() as u64 + tail.len() as u64 >= total {
        let mut whole = head.to_vec();
        whole.extend_from_slice(tail);
        return Drained {
            text: String::from_utf8_lossy(&whole).into_owned(),
            omitted: 0,
        };
    }

    // Both cuts land on character boundaries. A cut through a multi-byte character becomes U+FFFD,
    // which reads as damage to the program's output rather than to this reply.
    let head = &head[..whole_prefix(head)];
    let tail = &tail[whole_suffix(tail)..];
    // Counted from what is actually in the reply, so the bytes dropped for a character boundary
    // are in the figure too. A marker that undercounted would be a smaller lie than a silent cut
    // and still a lie.
    let omitted = total - head.len() as u64 - tail.len() as u64;
    let mut text = String::from_utf8_lossy(head).into_owned();
    let _ = write!(text, "\n… {omitted} bytes omitted here …\n");
    text.push_str(&String::from_utf8_lossy(tail));
    Drained { text, omitted }
}

/// How much of `bytes` is whole UTF-8, so a cut lands between characters rather than inside one.
fn whole_prefix(bytes: &[u8]) -> usize {
    match std::str::from_utf8(bytes) {
        // "the bytes ran out mid-character" is the cut this made; anything else is the stream's
        // own encoding and is reported lossily, as it always was.
        Err(error) if error.error_len().is_none() => error.valid_up_to(),
        Ok(_) | Err(_) => bytes.len(),
    }
}

/// Where the whole characters start in `bytes`, so a tail does not open mid-character.
fn whole_suffix(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|byte| byte & 0b1100_0000 != 0b1000_0000)
        .unwrap_or(bytes.len())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::TempDir;

    use super::{LocalOperations, MAPPING, harness_operation, tools_operation};
    use crate::{Operations, SearchOptions};

    fn workspace() -> TempDir {
        tempfile::tempdir().expect("a temporary workspace")
    }

    fn writing(root: &Path) -> LocalOperations {
        LocalOperations::unconfined(root, vec!["/bin/echo".to_owned()]).expect("a workspace")
    }

    #[test]
    fn the_name_mapping_round_trips_in_both_directions() {
        for (harness, tools) in MAPPING {
            assert_eq!(
                tools_operation(harness),
                Some(*tools),
                "{harness} -> {tools}"
            );
            assert_eq!(
                harness_operation(tools),
                Some(*harness),
                "{tools} -> {harness}"
            );
        }
        // `search` is `search.grep` since tools `0.3.0`, read in both directions.
        assert_eq!(tools_operation("search"), Some("search.grep"));
        assert_eq!(harness_operation("search.grep"), Some("search"));
        // `run` has not moved onto `b10x-tool-process` and stays local, so the table does not name
        // it — in either direction.
        assert_eq!(tools_operation("run"), None);
        assert_eq!(harness_operation("fs.walk"), None);
    }

    #[test]
    fn every_mapped_id_is_an_operation_the_tools_crates_expose() {
        // The table cannot name an id the tools crates do not answer. The `fs` and `search` ids are
        // read off the contract each crate carries; `b10x-tool-code-editor` keeps its parsed
        // contract private, so its two writes are named here.
        let mut exposed: Vec<String> = tool_fs::contract()
            .operations
            .iter()
            .chain(&tool_search::contract().operations)
            .map(|operation| operation.name.clone())
            .collect();
        exposed.extend(["code.write".to_owned(), "code.edit".to_owned()]);
        for (_, id) in MAPPING {
            assert!(
                exposed.iter().any(|name| name == id),
                "{id} is not an operation of the fs, search or code-editor toolset: {exposed:?}"
            );
        }
    }

    #[test]
    fn a_read_strips_the_tools_whole_file_digest_and_keeps_every_other_field() {
        let dir = workspace();
        std::fs::write(dir.path().join("notes.txt"), "alpha\nbeta\ngamma\n").expect("the seed");
        let operations = writing(dir.path());
        let answer = operations
            .file_read("notes.txt", crate::ReadWindow::whole())
            .expect("the read");
        let object = answer.as_object().expect("an object");
        assert!(!object.contains_key("sha256"), "the digest is stripped");
        assert_eq!(object["lines"]["total"], serde_json::json!(3));
        assert!(object["text"].as_str().expect("text").contains("beta"));
    }

    #[test]
    fn a_write_and_an_edit_answer_the_path_and_byte_count_alone() {
        let dir = workspace();
        let operations = writing(dir.path());
        let written = operations
            .file_write("file.txt", "one\ntwo\n")
            .expect("the write");
        assert_eq!(written, serde_json::json!({"path": "file.txt", "bytes": 8}));
        let edited = operations
            .file_edit("file.txt", "two", "three")
            .expect("the edit");
        assert_eq!(edited, serde_json::json!({"path": "file.txt", "bytes": 10}));
    }

    #[test]
    fn search_finds_a_literal_and_names_the_file() {
        let dir = workspace();
        std::fs::write(dir.path().join("notes.txt"), "alpha\nbeta\n").expect("the seed");
        let operations = writing(dir.path());
        let answer = operations
            .search("beta", ".", &SearchOptions::default())
            .expect("the search");
        assert_eq!(answer["matches"][0]["path"], serde_json::json!("notes.txt"));
        assert_eq!(answer["matches"][0]["line"], serde_json::json!(2));
    }

    #[test]
    fn the_adapter_wires_every_search_option_into_search_grep_and_strips_nothing() {
        // The adapter's wiring only: every `SearchOptions` field reaches `GrepInput`, and the reply
        // comes back with nothing stripped or added. Both sides are `tool_search`, so a change in
        // the pinned crate moves both and stays green here; the drift guard against 0.15.1's reply
        // is `tests/search_against_0_15_1.rs`.
        let dir = workspace();
        std::fs::write(
            dir.path().join("lib.rs"),
            "fn alpha() {}\nfn beta() {}\nfn gamma() {}\n",
        )
        .expect("the seed");
        let options = SearchOptions {
            regex: true,
            glob: Some("*.rs".to_owned()),
            context: Some(9),
            max_results: Some(1),
        };
        let answer = writing(dir.path())
            .search("fn (alpha|beta)", ".", &options)
            .expect("the search");
        let grep = tool_search::Search::new(dir.path())
            .expect("a workspace")
            .grep(&tool_search::GrepInput {
                pattern: "fn (alpha|beta)".to_owned(),
                path: Some(".".to_owned()),
                regex: Some(true),
                glob: Some("*.rs".to_owned()),
                context: Some(9),
                max_results: Some(1),
            })
            .expect("the grep");
        assert_eq!(
            answer,
            serde_json::to_value(grep).expect("serialises"),
            "{answer}"
        );
        assert_eq!(answer["context"], serde_json::json!(5), "the cap is echoed");
        assert_eq!(answer["truncated"], serde_json::json!(true), "{answer}");
    }

    /// The paths of a search reply's matches, in order.
    fn matched(answer: &serde_json::Value) -> Vec<&str> {
        answer["matches"]
            .as_array()
            .expect("a list of matches")
            .iter()
            .map(|found| found["path"].as_str().expect("a path"))
            .collect()
    }

    fn capped(max_results: usize) -> SearchOptions {
        SearchOptions {
            max_results: Some(max_results),
            ..SearchOptions::default()
        }
    }

    #[test]
    fn exactly_max_results_matches_is_a_complete_answer() {
        // harness's own grep tested the cap before every file and line, so exactly `max_results`
        // matches followed by anything at all answered `truncated: true`. tools `0.3.0` says a
        // match past the cap exists, and nothing else.
        let dir = workspace();
        std::fs::write(dir.path().join("a.txt"), "needle\nneedle\nother\n").expect("the seed");
        std::fs::write(dir.path().join("b.txt"), "other\n").expect("the seed");
        let operations = writing(dir.path());

        let answer = operations
            .search("needle", ".", &capped(2))
            .expect("the search");
        assert_eq!(matched(&answer), ["a.txt", "a.txt"]);
        assert_eq!(answer["truncated"], serde_json::json!(false), "{answer}");

        let answer = operations
            .search("needle", ".", &capped(1))
            .expect("the search");
        assert_eq!(matched(&answer), ["a.txt"]);
        assert_eq!(answer["truncated"], serde_json::json!(true), "{answer}");
    }

    #[test]
    fn after_the_cap_a_search_counts_the_large_files_it_meets_before_the_next_match() {
        let dir = workspace();
        let large = format!(
            "needle{}",
            "x".repeat(usize::try_from(tool_search::MAX_GREP_FILE_BYTES).expect("fits"))
        );
        std::fs::write(dir.path().join("a.txt"), "needle\n").expect("the seed");
        std::fs::write(dir.path().join("b.txt"), &large).expect("the seed");
        std::fs::write(dir.path().join("c.txt"), "needle\n").expect("the seed");
        std::fs::write(dir.path().join("d.txt"), &large).expect("the seed");
        let answer = writing(dir.path())
            .search("needle", ".", &capped(1))
            .expect("the search");
        assert_eq!(matched(&answer), ["a.txt"]);
        assert_eq!(answer["truncated"], serde_json::json!(true));
        assert_eq!(
            answer["skipped_large_paths"],
            serde_json::json!(["b.txt"]),
            "b.txt is met before the match past the cap in c.txt; d.txt is never reached"
        );
        assert_eq!(answer["skipped_large_files"], serde_json::json!(1));
    }

    #[test]
    fn an_empty_pattern_is_refused_in_search_greps_own_words() {
        let dir = workspace();
        let refused = writing(dir.path())
            .search("", ".", &SearchOptions::default())
            .expect_err("an empty pattern is refused");
        assert_eq!(
            refused,
            "`pattern` is required and must not be empty: a search has to name what it is \
             looking for. Nothing was read."
        );
    }

    #[test]
    fn a_file_holding_a_nul_byte_is_binary_and_is_not_searched() {
        let dir = workspace();
        std::fs::write(dir.path().join("blob.dat"), "needle\0\0\x01\n").expect("the seed");
        std::fs::write(dir.path().join("notes.txt"), "needle\n").expect("the seed");
        let answer = writing(dir.path())
            .search("needle", ".", &SearchOptions::default())
            .expect("the search");
        assert_eq!(matched(&answer), ["notes.txt"], "{answer}");
    }

    #[test]
    fn an_entry_named_like_a_skipped_directory_is_not_searched() {
        // A worktree's `.git` is a file, and the entry's own description says `.git` is never
        // searched.
        let dir = workspace();
        std::fs::write(
            dir.path().join(".git"),
            "gitdir: ../main/.git/worktrees/x needle\n",
        )
        .expect("the seed");
        std::fs::write(dir.path().join("notes.md"), "needle\n").expect("the seed");
        let answer = writing(dir.path())
            .search("needle", ".", &SearchOptions::default())
            .expect("the search");
        assert_eq!(matched(&answer), ["notes.md"], "{answer}");
    }

    #[test]
    fn an_empty_glob_is_refused_rather_than_matching_no_file() {
        let dir = workspace();
        std::fs::write(dir.path().join("notes.txt"), "needle\n").expect("the seed");
        let options = SearchOptions {
            glob: Some(String::new()),
            ..SearchOptions::default()
        };
        let refused = writing(dir.path())
            .search("needle", ".", &options)
            .expect_err("an empty glob names no file and is refused");
        assert!(refused.contains("`glob` must not be empty"), "{refused}");
    }

    #[test]
    fn a_path_that_climbs_out_is_refused_whether_or_not_anything_is_there() {
        let dir = workspace();
        std::fs::write(dir.path().join("notes.txt"), "needle\n").expect("the seed");
        let operations = writing(dir.path());
        for path in ["..", "../nowhere-at-all"] {
            let refused = operations
                .search("needle", path, &SearchOptions::default())
                .expect_err("a path out of the workspace is refused");
            assert_eq!(
                refused,
                format!("`{path}` resolves outside the workspace and was refused"),
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_named_pipe_in_the_tree_is_passed_over_rather_than_read() {
        // Opening a pipe for reading blocks until a writer comes, and none will: harness's own
        // grep read every entry that was not a directory, so one pipe held the call open for good.
        let dir = workspace();
        std::fs::write(dir.path().join("own.txt"), "needle\n").expect("the seed");
        let made = std::process::Command::new("mkfifo")
            .arg(dir.path().join("pipe"))
            .status()
            .expect("mkfifo runs");
        assert!(made.success(), "the pipe is made");
        let operations = writing(dir.path());
        let (answer, answered) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = answer.send(operations.search("needle", ".", &SearchOptions::default()));
        });
        let answer = answered
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the search answers rather than blocking on the pipe")
            .expect("the search");
        assert_eq!(matched(&answer), ["own.txt"], "{answer}");
    }

    #[test]
    fn a_declared_program_runs_and_an_undeclared_one_is_refused_by_name() {
        let dir = workspace();
        let operations = writing(dir.path());
        let ran = operations
            .run(&["/bin/echo".to_owned(), "hi".to_owned()])
            .expect("the declared program runs");
        assert_eq!(ran["exit"], serde_json::json!(0));
        let refused = operations
            .run(&["/bin/false".to_owned()])
            .expect_err("an undeclared program is refused");
        assert!(refused.refusal().is_some(), "the refusal is named");
    }
}
