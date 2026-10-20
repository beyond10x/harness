//! A bounded view of one directory, with no confinement under it, over the `tools` crates.
//!
//! # The provider with no boundary, and why it still exists
//!
//! Reading a tree is [`tool_fs`]'s and writing one is [`tool_code_editor`]'s: every path check is
//! that crate's own, canonicalising before a containment check. That is real and it is not
//! confinement — nothing stops the *process*, only those functions, and a bug in them is the whole
//! boundary. `search` and `run` stay here, because `tools` `0.1.0` has no search or process
//! toolset.
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
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tool_code_editor::{EditInput, Editor, WriteInput};
use tool_fs::{Fs, GlobInput, ListInput, ReadInput};

use crate::{Operations, ReadWindow, Refusal, Refused, SearchOptions};

const MAX_GREP_RESULTS: usize = 200;
const MAX_GREP_FILE_BYTES: u64 = 1024 * 1024;
/// How deep under its starting directory a walk descends before it stops.
///
/// A bound rather than none: a deep or cyclic tree would otherwise hold the run open. `pub(crate)`
/// because `search` names the figure in its own description — a bound the model cannot read is one
/// it cannot work around, and the entry answers `depth_bound_reached` when it bit.
pub(crate) const MAX_GREP_DEPTH: usize = 12;
const MAX_RUN_OUTPUT_BYTES: usize = 64 * 1024;
/// The most lines of context a search may be asked for either side of a match.
///
/// Five is enough to see a function signature over a match and small enough that two hundred
/// matches with context is still a reply and not a file dump.
const MAX_SEARCH_CONTEXT: u64 = 5;
/// How long an unconfined `run` may hold the turn open before it is killed.
///
/// A bound rather than none: `wait` on a child that never exits stops the run with no record of
/// why. Ten minutes is longer than any suite this has been pointed at and shorter than a night.
const MAX_RUN_SECONDS: u64 = 600;
/// How much of a matched line `search` reports. One minified file on a single line would otherwise
/// bury every other result, so the line is cut — and the match says that it was.
const MAX_MATCH_CHARS: usize = 400;

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

/// Directories skipped while walking. Each is either machine output or another tool's private
/// state, and including them buries the answer the person asked for.
///
/// `pub(crate)` because `search` names them one by one in its own description. "Build output and
/// version-control directories are skipped" tells a model that something was left out and not
/// *what*, so a model looking for a file under `target/` reads a complete-looking empty answer and
/// concludes the file is not there.
pub(crate) const SKIPPED: &[&str] = &[".git", "target", "node_modules", ".venv", "__pycache__"];

/// The **declared** harness-name↔tools-id correspondence: `(harness, tools)`.
///
/// Not the router — each `Operations` method below calls its tools operation directly, by name, in
/// code. This table is what a consumer and the two-way test read instead: [`tools_operation`] and
/// [`harness_operation`] answer it in each direction, and `every_mapped_id_is_an_operation_the_tools_crates_expose`
/// is what keeps it honest against the tools crates, so the declaration cannot drift from what the
/// adapter actually calls.
///
/// `search` and `run` are **not** here — `tools` `0.1.0` has no search or process toolset, so they
/// stay local. The recorded operation name (`file.read`, `shell`) is a third thing and is not this
/// table; metaharness reads that back, and renaming it is not renaming this.
pub(crate) const MAPPING: &[(&str, &str)] = &[
    ("file_read", "fs.read"),
    ("dir_list", "fs.list"),
    ("find", "fs.glob"),
    ("file_write", "code.write"),
    ("file_edit", "code.edit"),
];

/// The `tools` operation id a harness operation is performed by, or `None` for one that stays local
/// (`search`, `run`).
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

/// A bounded view of one directory, read through [`tool_fs`] and written through
/// [`tool_code_editor`], with `search` and `run` performed here.
#[derive(Debug, Clone)]
pub struct LocalOperations {
    root: PathBuf,
    /// Reads: `fs.read`, `fs.list`, `fs.glob`.
    fs: Fs,
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
        let editor = Editor::new(&root).map_err(|error| error.to_string())?;
        Ok(Self {
            root: fs.root().to_path_buf(),
            fs,
            editor,
            programs,
            inherited: Vec::new(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves a caller-supplied path inside the workspace, for `search` alone.
    ///
    /// Reads and writes resolve through the tools crates; this is the one place `search` reaches
    /// the host filesystem, so it canonicalises before the containment check exactly as those do.
    fn resolve(&self, relative: &str) -> Result<PathBuf, String> {
        let candidate = self.root.join(relative);
        let canonical = candidate
            .canonicalize()
            .map_err(|error| format!("`{relative}`: {error}"))?;
        if !canonical.starts_with(&self.root) {
            return Err(format!(
                "`{relative}` resolves outside the workspace and was refused"
            ));
        }
        Ok(canonical)
    }

    fn display(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
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

    fn grep(
        &self,
        pattern: &str,
        relative: &str,
        options: &SearchOptions,
    ) -> Result<Value, String> {
        if pattern.is_empty() {
            return Err("`pattern` is required and must not be empty".to_owned());
        }
        let limit = options
            .max_results
            .unwrap_or(MAX_GREP_RESULTS)
            .min(MAX_GREP_RESULTS);
        // Capped rather than refused: a caller who asked for forty lines of context wanted to see
        // around the match, and five is seeing around it. The cap is in the entry's own schema —
        // and the reply echoes the figure that was actually used when it differs from the one that
        // was asked for, because a cap the reader cannot see is a bound they will not work around.
        let asked_context = options.context.unwrap_or(0);
        let context = usize::try_from(asked_context.min(MAX_SEARCH_CONTEXT)).unwrap_or(usize::MAX);
        let expression = search_expression(pattern, options.regex)?;
        let filter = options.glob.as_deref().map(PathGlob::compile).transpose()?;
        let root = self.resolve(relative)?;

        let mut matches = Vec::new();
        let mut truncated = false;
        let mut skipped_large_files = 0_u64;
        let mut skipped_large_paths = Vec::new();
        let boundary = self.root.clone();
        let depth_bound_reached = walk(&root, &boundary, 0, &mut |file| {
            if matches.len() >= limit {
                truncated = true;
                return false;
            }
            let shown = self.display(file);
            if filter.as_ref().is_some_and(|glob| !glob.matches(&shown)) {
                return true;
            }
            let Ok(metadata) = file.metadata() else {
                return true;
            };
            if metadata.len() > MAX_GREP_FILE_BYTES {
                skipped_large_files = skipped_large_files.saturating_add(1);
                if skipped_large_paths.len() < 20 {
                    skipped_large_paths.push(shown);
                }
                truncated = true;
                return true;
            }
            let Ok(text) = fs::read_to_string(file) else {
                // Binary or unreadable. Skipping is right; reporting each one is noise.
                return true;
            };
            let lines: Vec<&str> = text.lines().collect();
            for (index, line) in lines.iter().enumerate() {
                if matches.len() >= limit {
                    truncated = true;
                    return false;
                }
                let hit = match &expression {
                    Some(expression) => expression.is_match(line),
                    None => line.contains(pattern),
                };
                if !hit {
                    continue;
                }
                let mut hit = json!({
                    "path": shown,
                    "line": index + 1,
                    "text": cut_to(line),
                    // Always present, so the shape is stable and a reader who checks the flag
                    // never has to wonder whether its absence means "whole" or "old reply".
                    "line_truncated": line.chars().nth(MAX_MATCH_CHARS).is_some(),
                });
                // Only when context was asked for. Two empty arrays on every match of every
                // search would be bytes replayed on every later turn to say *nothing here*.
                if context > 0 {
                    hit["before"] = around(&lines, index.saturating_sub(context)..index);
                    hit["after"] =
                        around(&lines, index + 1..(index + 1 + context).min(lines.len()));
                }
                matches.push(hit);
            }
            true
        });

        let mut answer = json!({
            "pattern": pattern,
            "matches": matches,
            "truncated": truncated,
            // Always present, like `truncated`, and for the same reason: a walk that stopped
            // descending at [`MAX_GREP_DEPTH`] answered a subset of the tree, and a reply that said
            // nothing about it reads exactly like one that searched everything.
            "depth_bound_reached": depth_bound_reached,
            "skipped_large_files": skipped_large_files,
            "skipped_large_paths": skipped_large_paths,
        });
        // Echoed only when they were asked for, so a reader of the record can tell a literal
        // search from a regular expression without holding the call beside the answer.
        if options.regex {
            answer["regex"] = json!(true);
        }
        if let Some(glob) = &options.glob {
            answer["glob"] = json!(glob);
        }
        // Only when the cap bit. A caller who asked for forty lines got five, and a reply that did
        // not say so leaves them reading five as though it were forty.
        if asked_context > MAX_SEARCH_CONTEXT {
            answer["context"] = json!(MAX_SEARCH_CONTEXT);
            answer["note"] = json!(format!(
                "`context` was {asked_context} and this build answers at most \
                 {MAX_SEARCH_CONTEXT} lines either side of a match, so {MAX_SEARCH_CONTEXT} is \
                 what these matches carry."
            ));
        }
        Ok(answer)
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

    fn search(&self, pattern: &str, path: &str, options: &SearchOptions) -> Result<Value, String> {
        self.grep(pattern, path, options)
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

/// Compiles a model-supplied regular expression once, before walking anything.
///
/// Both program limits are explicit: the crate defaults would let one pattern choose far more
/// memory. A compile failure is named instead of being misreported as an empty search result.
fn search_expression(pattern: &str, enabled: bool) -> Result<Option<regex::Regex>, String> {
    if !enabled {
        return Ok(None);
    }
    regex::RegexBuilder::new(pattern)
        .size_limit(1 << 20)
        .dfa_size_limit(1 << 20)
        .build()
        .map(Some)
        .map_err(|error| {
            format!("`{pattern}` is not a regular expression this build can compile: {error}")
        })
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

/// A match's line, cut at [`MAX_MATCH_CHARS`] on a character boundary.
fn cut_to(line: &str) -> String {
    line.chars().take(MAX_MATCH_CHARS).collect()
}

/// The lines of `range`, each under its own number, for the context around a match.
fn around(lines: &[&str], range: std::ops::Range<usize>) -> Value {
    Value::Array(
        lines[range.clone()]
            .iter()
            .zip(range)
            .map(|(line, index)| {
                json!({
                    "line": index + 1,
                    "text": cut_to(line),
                    "line_truncated": line.chars().nth(MAX_MATCH_CHARS).is_some(),
                })
            })
            .collect(),
    )
}

/// One glob, matched the way a person means it.
///
/// **A glob with no `/` in it matches the file's name anywhere in the tree**; one with a `/`
/// matches the whole workspace-relative path, and `*` does not cross a directory separator there.
/// That is ripgrep's rule, taken rather than invented: a model that asks for `*.rs` means every
/// Rust file, and answering it with the four in the workspace root would read as *there are four*.
struct PathGlob {
    matcher: globset::GlobMatcher,
    by_name: bool,
}

impl PathGlob {
    /// # Errors
    ///
    /// The glob does not compile, with globset's own words — a pattern nobody can match is refused
    /// rather than quietly matching nothing.
    fn compile(pattern: &str) -> Result<Self, String> {
        let matcher = globset::GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|error| format!("`{pattern}` is not a glob this build can compile: {error}"))?
            .compile_matcher();
        Ok(Self {
            matcher,
            by_name: !pattern.contains('/'),
        })
    }

    fn matches(&self, relative: &str) -> bool {
        let candidate = if self.by_name {
            relative.rsplit('/').next().unwrap_or(relative)
        } else {
            relative
        };
        self.matcher.is_match(candidate)
    }
}

/// Walks files under `directory`, calling `visit` until it returns `false`.
///
/// Every entry is re-checked against `boundary`. Checking only the path the caller supplied is not
/// enough: `is_dir` and `read_dir` both follow symlinks, so a link inside the workspace pointing
/// anywhere else would be walked and its contents returned under a workspace-relative name.
///
/// Depth is bounded so a deep or cyclic tree cannot hold the run open.
///
/// **Answers whether that bound stopped a descent**, which is the whole reason it returns anything.
/// A walk that stopped at [`MAX_GREP_DEPTH`] visited a subset of the tree, and `search` answered
/// `truncated: false` over it — a bound nothing reported, which is invariant 8. The caller puts it
/// in the reply as `depth_bound_reached`.
///
/// [`SKIPPED`] directories are not reported the same way: they are named in the entry's own
/// description, so a model reads *which* directories are not searched before it calls rather than
/// afterwards.
fn walk(
    directory: &Path,
    boundary: &Path,
    depth: usize,
    visit: &mut dyn FnMut(&Path) -> bool,
) -> bool {
    if depth > MAX_GREP_DEPTH {
        return true;
    }
    if !contained(directory, boundary) {
        return false;
    }
    if directory.is_file() {
        visit(directory);
        return false;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return false;
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let mut bounded = false;
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !contained(&path, boundary) {
            continue;
        }
        if path.is_dir() {
            if SKIPPED.contains(&name.as_ref()) {
                continue;
            }
            bounded |= walk(&path, boundary, depth + 1, visit);
        } else if !visit(&path) {
            return bounded;
        }
    }
    bounded
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

/// Whether `path` really lives inside `boundary`, following every link on the way.
fn contained(path: &Path, boundary: &Path) -> bool {
    path.canonicalize()
        .is_ok_and(|resolved| resolved.starts_with(boundary))
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
        // `search` and `run` have no `tools` operation in 0.1.0 and stay local, so the table names
        // neither — in either direction.
        assert_eq!(tools_operation("search"), None);
        assert_eq!(tools_operation("run"), None);
        assert_eq!(harness_operation("fs.walk"), None);
    }

    #[test]
    fn every_mapped_id_is_an_operation_the_tools_crates_expose() {
        // The table cannot name an id the tools crates do not answer: the `fs` toolset answers the
        // three reads and the `code-editor` toolset the two writes, and nothing else is mapped.
        let fs = ["fs.read", "fs.list", "fs.glob"];
        let code = ["code.write", "code.edit"];
        for (_, id) in MAPPING {
            assert!(
                fs.contains(id) || code.contains(id),
                "{id} is not an operation of the fs or code-editor toolset"
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
