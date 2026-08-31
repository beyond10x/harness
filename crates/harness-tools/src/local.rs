//! A bounded view of one directory, with no confinement under it.
//!
//! # The provider with no boundary, and why it still exists
//!
//! Every path check here is this module's own: `resolve` canonicalises and then checks containment,
//! and [`walk`] re-checks every entry it descends into. That is real and it is not confinement —
//! nothing stops the *process*, only these functions, and a bug in them is the whole boundary.
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
//! machine, where the confinement story is a scratch home rather than a cgroup — and the
//! alternative was a second copy of [`resolve`](LocalOperations::resolve), [`walk`] and
//! [`contained`] living over there. Three copies of a path check are three places for one of them
//! to stop being true.
//!
//! What it does *not* do is pretend: nothing here confines the process, the bound on an effect is
//! this module's own arithmetic on paths, and the constructor is named so that a reader of the call
//! site knows it. A run that wants the effects actually confined passes `harness-substrate`'s
//! provider instead, and the catalogue cannot tell the difference.

use std::fmt::Write as _;
use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{Operations, ReadWindow, Refusal, Refused, SearchOptions};

const MAX_LIST_ENTRIES: usize = 500;
const MAX_READ_BYTES: u64 = 64 * 1024;
const MAX_READ_BYTES_CEILING: u64 = 256 * 1024;
const MAX_GREP_RESULTS: usize = 200;
const MAX_GREP_FILE_BYTES: u64 = 1024 * 1024;
/// How deep under its starting directory a walk descends before it stops.
///
/// A bound rather than none: a deep or cyclic tree would otherwise hold the run open. `pub(crate)`
/// because `search` and `find` name the figure in their own descriptions — a bound the model cannot
/// read is one it cannot work around, and both entries answer `depth_bound_reached` when it bit.
pub(crate) const MAX_GREP_DEPTH: usize = 12;
const MAX_RUN_OUTPUT_BYTES: usize = 64 * 1024;
/// The most paths one `find` answers with.
///
/// Larger than a listing's 500-entry page would suggest, because a glob is already the filter: a
/// `find` of `**/*.rs` in a real workspace is the answer, not a page of it. Above this the reply
/// says it was cut and the caller narrows the glob.
const MAX_FIND_RESULTS: usize = 500;
/// The most lines of context a search may be asked for either side of a match.
///
/// Five is enough to see a function signature over a match and small enough that two hundred
/// matches with context is still a reply and not a file dump.
const MAX_SEARCH_CONTEXT: u64 = 5;
/// How much of one line a read answers with.
///
/// A minified bundle is one line of two megabytes, and a window of ten lines that included it
/// would be the whole read. The line is cut and its number is reported, so a model that quoted it
/// back to `file_edit` is refused for not matching rather than left to wonder.
const MAX_READ_LINE_CHARS: usize = 2_000;
/// How many bytes of one line are held while it is being read.
///
/// Four bytes per character is the widest UTF-8 gets, so this holds at least
/// [`MAX_READ_LINE_CHARS`] characters whatever the encoding — and a file with no newline in it at
/// all cannot make a read allocate its whole length.
const MAX_READ_LINE_BYTES: usize = MAX_READ_LINE_CHARS * 4;
/// How far a read scans while counting the lines of a file.
///
/// `lines.total` is what stops a window being mistaken for a whole file, and counting lines means
/// reading to the end. On a multi-gigabyte artefact that is a full sequential scan **on every
/// read**, bounded by nothing — the loop's deadline check between calls cannot reach inside one
/// call, and the window itself is over after 256 KiB at the most.
///
/// So the scan stops here and the reply says so rather than answering a number it did not finish:
/// `lines.total` is `null` and `lines_counted_to` names the last line the scan reached. Sixteen
/// mebibytes is far past any source file and far short of a build artefact, and a `null` there is
/// absence kept as absence rather than a count that would be wrong.
const MAX_LINE_COUNT_BYTES: u64 = 16 * 1024 * 1024;
/// How long an unconfined `run` may hold the turn open before it is killed.
///
/// A bound rather than none: `wait` on a child that never exits stops the run with no record of
/// why. Ten minutes is longer than any suite this has been pointed at and shorter than a night.
const MAX_RUN_SECONDS: u64 = 600;
/// How much of a matched line `search` reports. One minified file on a single line would
/// otherwise bury every other result, so the line is cut — and the match says that it was.
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
/// `pub(crate)` because `search` and `find` name them one by one in their own descriptions. "Build
/// output and version-control directories are skipped" tells a model that something was left out
/// and not *what*, so a model looking for a file under `target/` reads a complete-looking empty
/// answer and concludes the file is not there.
pub(crate) const SKIPPED: &[&str] = &[".git", "target", "node_modules", ".venv", "__pycache__"];

#[derive(Debug, Clone)]
pub struct LocalOperations {
    root: PathBuf,
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
        Ok(Self {
            root: Self::open(root)?,
            programs: None,
            inherited: Vec::new(),
        })
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
    /// entry at all. Everything a caller gains here it gains on the strength of this module's path
    /// arithmetic and nothing else — see the module documentation for what that is and is not.
    ///
    /// # Errors
    ///
    /// Returns a message when `root` is not a readable directory.
    pub fn unconfined(root: impl AsRef<Path>, programs: Vec<String>) -> Result<Self, String> {
        Ok(Self {
            root: Self::open(root)?,
            programs: Some(programs),
            inherited: Vec::new(),
        })
    }

    fn open(root: impl AsRef<Path>) -> Result<PathBuf, String> {
        let root = root
            .as_ref()
            .canonicalize()
            .map_err(|error| format!("workspace `{}`: {error}", root.as_ref().display()))?;
        if !root.is_dir() {
            return Err(format!("workspace `{}` is not a directory", root.display()));
        }
        Ok(root)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves a caller-supplied path inside the workspace.
    ///
    /// Canonicalization happens before the containment check, so a symlink or a `..` that leaves
    /// the workspace is refused by where it *lands*, not by how it is spelled.
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

    fn list(&self, relative: &str) -> Result<Value, String> {
        let target = self.resolve(relative)?;
        if !target.is_dir() {
            return Err(format!("`{relative}` is not a directory"));
        }
        let mut entries = Vec::new();
        let mut truncated = false;
        let mut reader = fs::read_dir(&target)
            .map_err(|error| format!("`{relative}`: {error}"))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        reader.sort_by_key(std::fs::DirEntry::file_name);
        for entry in reader {
            if entries.len() >= MAX_LIST_ENTRIES {
                truncated = true;
                break;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            // A link's own type is not its target's, so a link to a directory used to be listed as
            // a file with the length of the link text. Naming it a link says what it is without
            // this listing having to follow it, which is a decision for whoever opens it.
            let file_type = entry.file_type().ok();
            let kind = match file_type {
                Some(kind) if kind.is_symlink() => "symlink",
                Some(kind) if kind.is_dir() => "directory",
                _ => "file",
            };
            entries.push(json!({
                "name": name,
                "kind": kind,
                "bytes": entry
                    .metadata()
                    .ok()
                    .filter(|_| kind == "file")
                    .map(|meta| meta.len()),
            }));
        }
        Ok(json!({
            "path": self.display(&target),
            "entries": entries,
            // Named, so the model knows the listing is partial rather than assuming it is whole.
            "truncated": truncated,
        }))
    }

    /// Opens one resolved file and returns the metadata every read reply carries.
    fn open_read(
        &self,
        relative: &str,
    ) -> Result<(PathBuf, u64, std::io::BufReader<fs::File>), String> {
        let target = self.resolve(relative)?;
        if !target.is_file() {
            return Err(format!("`{relative}` is not a file"));
        }
        let bytes = fs::metadata(&target)
            .map_err(|error| format!("`{relative}`: {error}"))?
            .len();
        let file = fs::File::open(&target).map_err(|error| format!("`{relative}`: {error}"))?;
        Ok((target, bytes, std::io::BufReader::new(file)))
    }

    /// One window of one file, as numbered lines.
    ///
    /// # Why the reply is `cat -n` and not the file's own bytes
    ///
    /// What a model does with a read is quote part of it back to `file_edit`, and an edit lands
    /// where the quoted text is. Numbering every line gives it a way to say *which* occurrence it
    /// means and gives a person reading the record a way to check. It is the shape Claude Code's
    /// `Read` answers with, and it is the reason its edits land: the number is a prefix the model
    /// strips, and the entry's own description says so.
    ///
    /// # Why the file is walked past the window, and how far
    ///
    /// `lines.total` is the thing that keeps a window from being mistaken for a file — without it
    /// a model that read lines 1..64 of a 4,000-line file has no way to know there is more, which
    /// is invariant 8 in a different costume. Counting them means reading past the window. It is
    /// sequential I/O and **one line is in memory at a time**: the multi-gigabyte artefact the
    /// earlier byte-ceiling read was careful not to pull into memory is still not pulled into
    /// memory.
    ///
    /// It is still a scan, though, and on that artefact it is a scan of the whole thing on every
    /// read with nothing to stop it. So it stops at [`MAX_LINE_COUNT_BYTES`]: past there
    /// `lines.total` is `null` and `lines_counted_to` says which line the scan reached. `bytes` is
    /// the file's own size from its metadata either way, so the size of the thing is never in
    /// doubt — only how many lines it holds.
    fn read(&self, relative: &str, window: ReadWindow) -> Result<Value, String> {
        let ceiling = window
            .max_bytes
            .unwrap_or(MAX_READ_BYTES)
            .min(MAX_READ_BYTES_CEILING);
        let offset = window.offset.unwrap_or(1);
        if offset == 0 {
            return Err(format!(
                "`offset` is the first line to read and lines are numbered from 1, so 0 names no \
                 line. `{relative}` was not read."
            ));
        }
        let (target, bytes, mut reader) = self.open_read(relative)?;

        let mut text = String::new();
        let mut cut = Vec::new();
        let mut total: u64 = 0;
        let mut kept: u64 = 0;
        let mut answered_bytes: u64 = 0;
        let mut scanned: u64 = 0;
        let mut last: u64 = 0;
        let mut closed = false;
        let mut counting_bounded = false;
        let mut byte_ceiling_cut = false;
        while let Some(line) = next_line(&mut reader, MAX_READ_LINE_BYTES)
            .map_err(|error| format!("`{relative}`: {error}"))?
        {
            total += 1;
            scanned += line.length + 1;
            if total < offset || closed {
                // The scan is the whole cost of this call on a large file, so it is the thing that
                // is bounded. Checked after the line is counted, so `total` names a line that was
                // actually reached.
                if scanned >= MAX_LINE_COUNT_BYTES {
                    counting_bounded = true;
                    break;
                }
                continue;
            }
            // The window closes on whichever bound arrives first — the line count the caller asked
            // for, or the byte ceiling. The first line is always answered even when it is over the
            // ceiling on its own, because a read that answered nothing would look like an empty
            // file. The separator counts towards the ceiling: it is a byte of the file, and
            // ignoring it would answer more than the ceiling allowed for a file of short lines.
            let weight = line.length + 1;
            let within_limit = window.limit.is_none_or(|count| kept < count);
            let within_ceiling = kept == 0 || answered_bytes + weight <= ceiling;
            if !(within_limit && within_ceiling) {
                closed = true;
                continue;
            }
            let shown_bytes = if kept == 0 && weight > ceiling {
                byte_ceiling_cut = true;
                let allowed = usize::try_from(ceiling.saturating_sub(1))
                    .unwrap_or(usize::MAX)
                    .min(line.kept.len());
                &line.kept[..whole_prefix(&line.kept[..allowed])]
            } else {
                &line.kept
            };
            let content = String::from_utf8_lossy(shown_bytes);
            let shown: String = content.chars().take(MAX_READ_LINE_CHARS).collect();
            if byte_ceiling_cut || !line.whole || content.chars().count() > MAX_READ_LINE_CHARS {
                cut.push(total);
            }
            let _ = writeln!(text, "{total:>6}\t{shown}");
            kept += 1;
            answered_bytes = answered_bytes.saturating_add(weight.min(ceiling));
            last = total;
        }

        // A window that starts past the end is a refusal and not an empty answer: the model asked
        // for something that is not there, and *no lines* would read as *the file is empty*. Line 1
        // of an empty file is not that case — it is the file, answered whole.
        if offset > 1 && offset > total {
            return Err(if counting_bounded {
                format!(
                    "`{relative}` was scanned to line {total} — this build stops counting lines \
                     after {MAX_LINE_COUNT_BYTES} bytes — and `offset` names line {offset}, past \
                     where the scan reached. Nothing was read."
                )
            } else {
                format!(
                    "`{relative}` has {total} lines and `offset` names line {offset}, which is \
                     past the end. Nothing was read."
                )
            });
        }
        let mut answer = json!({
            "path": self.display(&target),
            "bytes": bytes,
            // **The window stops before the file's last line, or a line in it was cut.** Not "the
            // model has seen all of this file": a window with an `offset` in the middle that ends
            // on the last line answers `false` and the lines before `offset` were never in it. A
            // one-line minified bundle whose only line was cut at [`MAX_READ_LINE_CHARS`] answers
            // `true`, because reaching the last line is not reading it.
            //
            // `true` as well when the line count was bounded: the scan stopped before the end, so
            // whether the window reached the last line is not known here.
            "truncated": counting_bounded || byte_ceiling_cut || last < total || !cut.is_empty(),
            "text": text,
            // So a window is never mistaken for the whole file. `to` is one below `from` when the
            // window answered nothing at all, which is only an empty file read from line 1.
            //
            // `total` is `null` when the scan stopped at [`MAX_LINE_COUNT_BYTES`]: how many lines
            // the file has is then unknown, and a number that counted part of it would be read as
            // the whole count. Absence stays absence.
            "lines": {
                "from": offset,
                "to": if kept == 0 { offset.saturating_sub(1) } else { last },
                "total": if counting_bounded { Value::Null } else { json!(total) },
            },
            // Every line this reply cut at [`MAX_READ_LINE_CHARS`], by number. Always present, so
            // an empty list means *nothing was cut* rather than *an older reply*. No marker is put
            // in the text itself: the text is what gets quoted back to `file_edit`, and a marker
            // inside it would be quoted too.
            "truncated_lines": cut,
        });
        if counting_bounded {
            // Only where counting stopped, for the reason `note` is only on a filtered
            // `tool_search`: a field that says *nothing happened* on every ordinary read is bytes
            // replayed on every later turn to say nothing.
            answer["lines_counted_to"] = json!(total);
            answer["note"] = json!(format!(
                "line counting stopped after {MAX_LINE_COUNT_BYTES} bytes of `{}`, at line \
                 {total}, so how many lines it has is not known. `bytes` is still the whole file's \
                 size, and a window past line {total} is refused rather than guessed at.",
                self.display(&target)
            ));
        }
        Ok(answer)
    }

    /// Resolves a path the caller intends to *create*, which `resolve` cannot: `canonicalize` on a
    /// path that is not there yet answers nothing at all.
    ///
    /// So it canonicalises the deepest ancestor that does exist — that is the part a symlink could
    /// have moved — checks containment there, and rebuilds. A `..` left in the unresolved tail is
    /// refused rather than appended, because `PathBuf::push` treats it as a name and the containment
    /// check would then pass on a path that escapes.
    ///
    /// Presence is `symlink_metadata`, never `exists`. `Path::exists` follows links, so a dangling
    /// link inside the workspace answered "not there" and the walk carried on up to the workspace
    /// root, whose containment check of course passes — leaving `fs::write` to follow the link and
    /// create the file wherever it pointed. Asking about the link itself stops the walk at it, and
    /// then `canonicalize` fails on a link that leads nowhere and refuses one that leads out.
    fn resolve_new(&self, relative: &str) -> Result<PathBuf, String> {
        let candidate = self.root.join(relative);
        let mut tail = Vec::new();
        let mut existing = candidate.as_path();
        while existing.symlink_metadata().is_err() {
            let (Some(name), Some(parent)) = (existing.file_name(), existing.parent()) else {
                return Err(format!(
                    "`{relative}` names no path this workspace can write"
                ));
            };
            tail.push(name.to_owned());
            existing = parent;
        }

        let mut resolved = existing.canonicalize().map_err(|error| {
            format!(
                "`{relative}`: `{}` is a link that leads nowhere this workspace can write ({error})",
                self.display(existing)
            )
        })?;
        if !resolved.starts_with(&self.root) {
            return Err(format!(
                "`{relative}` resolves outside the workspace and was refused"
            ));
        }
        for name in tail.iter().rev() {
            if name == ".." {
                return Err(format!(
                    "`{relative}` climbs out of a directory that does not exist and was refused"
                ));
            }
            resolved.push(name);
        }
        Ok(resolved)
    }

    fn write(&self, relative: &str, text: &str) -> Result<Value, String> {
        let target = self.resolve_new(relative)?;
        // Belt and braces on top of `resolve_new`: a link put here between that check and this call
        // would still be followed by `fs::write`, and the bytes would land wherever it points.
        if target
            .symlink_metadata()
            .is_ok_and(|meta| meta.file_type().is_symlink())
        {
            return Err(format!(
                "`{relative}` is a symlink, and this workspace writes files rather than through \
                 links. Name the file itself."
            ));
        }
        if target.parent().is_some_and(|parent| !parent.is_dir()) {
            return Err(format!(
                "`{relative}` has a parent directory that does not exist; create the directory by \
                 an admitted operation before writing the file"
            ));
        }
        fs::write(&target, text).map_err(|error| format!("`{relative}`: {error}"))?;
        Ok(json!({"path": self.display(&target), "bytes": text.len()}))
    }

    fn edit(&self, relative: &str, old: &str, new: &str) -> Result<Value, String> {
        if old.is_empty() {
            return Err("`old` must not be empty: an edit has to name where it lands".to_owned());
        }
        let target = self.resolve(relative)?;
        let text = fs::read_to_string(&target).map_err(|error| format!("`{relative}`: {error}"))?;

        // Exactly one place, or nothing happens. An edit that matched twice would silently change
        // the wrong one, and the caller has no way to find out which.
        match text.matches(old).count() {
            0 => Err(format!("that text appears nowhere in `{relative}`")),
            1 => {
                let replaced = text.replacen(old, new, 1);
                fs::write(&target, &replaced).map_err(|error| format!("`{relative}`: {error}"))?;
                Ok(json!({"path": self.display(&target), "bytes": replaced.len()}))
            }
            found => Err(format!(
                "that text appears {found} times in `{relative}`; include enough surrounding \
                 text to name one place"
            )),
        }
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

    fn find_paths(
        &self,
        glob: &str,
        relative: &str,
        max_results: Option<usize>,
    ) -> Result<Value, String> {
        // An empty glob matches no path, so a `find` with one answered an empty list — which reads
        // as *there are no such files* rather than as *you named no pattern*. `search` refuses an
        // empty pattern by name and this is the same refusal.
        if glob.is_empty() {
            return Err(
                "`glob` is required and must not be empty: a find has to name what it is looking \
                 for. `*.rs` is that name at any depth; `crates/**/*.rs` is the whole path."
                    .to_owned(),
            );
        }
        let limit = max_results
            .unwrap_or(MAX_FIND_RESULTS)
            .min(MAX_FIND_RESULTS);
        let filter = PathGlob::compile(glob)?;
        let root = self.resolve(relative)?;

        let mut paths: Vec<Value> = Vec::new();
        let mut truncated = false;
        let boundary = self.root.clone();
        let depth_bound_reached = walk(&root, &boundary, 0, &mut |file| {
            if paths.len() >= limit {
                truncated = true;
                return false;
            }
            let shown = self.display(file);
            if filter.matches(&shown) {
                paths.push(Value::String(shown));
            }
            true
        });

        Ok(json!({
            "paths": paths,
            "truncated": truncated,
            // Always present, like `truncated`. `find` says it lists every match under the
            // workspace; where the walk stopped descending, it listed the matches above a depth
            // instead, and nothing else in the reply would say so.
            "depth_bound_reached": depth_bound_reached,
        }))
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
/// A walk that stopped at [`MAX_GREP_DEPTH`] visited a subset of the tree, and `find` and `search`
/// answered `truncated: false` over it — a bound nothing reported, which is invariant 8. The caller
/// puts it in the reply as `depth_bound_reached`.
///
/// [`SKIPPED`] directories are not reported the same way: they are named in both entries' own
/// descriptions, so a model reads *which* directories are not searched before it calls rather than
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

/// One line of a file: what is kept of it, how long it really was, and whether the two agree.
struct Line {
    /// The line's bytes, up to what the reader was told to keep, with a `\r` before the newline
    /// dropped.
    kept: Vec<u8>,
    /// Its true length in the file, before anything was dropped — separator excluded, `\r`
    /// included. What the byte ceiling is charged.
    length: u64,
    /// Whether `kept` holds the whole line. Not `kept.len() == length`: a stripped `\r` makes the
    /// two differ on a file the reader saw all of, and a reply that called that line *cut* would
    /// name every line of a CRLF file.
    whole: bool,
}

/// The next line from `reader`, keeping at most `keep` of its bytes.
///
/// Its own function rather than [`BufRead::read_until`] for one reason: `read_until` on a file with
/// no newline in it allocates the whole file. A minified bundle is exactly that, and a *read* of it
/// must not be a way to spend a gigabyte of this process's memory. The line's true length is
/// counted whatever is kept, so the reply can say the line was cut.
///
/// **A `\r` before the newline is dropped**, which is what `str::lines` does and therefore what the
/// confined provider has always answered. Until that was true here, one CRLF file read differently
/// on the two providers and the `\r` a model quoted back to `file_edit` matched nothing — the edit
/// was refused for text it had just been handed. A trailing `\r` at the very end of a file with no
/// newline after it is kept, because that is a `\r` and not a line ending, and `str::lines` keeps
/// it too.
///
/// `None` at the end of the file. A last line with no trailing newline is a line.
fn next_line(reader: &mut impl BufRead, keep: usize) -> std::io::Result<Option<Line>> {
    let mut line = Line {
        kept: Vec::new(),
        length: 0,
        whole: true,
    };
    let mut any = false;
    loop {
        let (ends_at, available) = {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return Ok(any.then_some(line));
            }
            any = true;
            let ends_at = available.iter().position(|byte| *byte == b'\n');
            let end = ends_at.unwrap_or(available.len());
            let room = keep.saturating_sub(line.kept.len());
            line.whole &= end <= room;
            line.kept.extend_from_slice(&available[..end.min(room)]);
            line.length += end as u64;
            (ends_at, available.len())
        };
        match ends_at {
            Some(at) => {
                reader.consume(at + 1);
                if line.whole && line.kept.last() == Some(&b'\r') {
                    line.kept.pop();
                }
                return Ok(Some(line));
            }
            None => reader.consume(available),
        }
    }
}

/// Whether `path` really lives inside `boundary`, following every link on the way.
fn contained(path: &Path, boundary: &Path) -> bool {
    path.canonicalize()
        .is_ok_and(|resolved| resolved.starts_with(boundary))
}

/// Acts on the process's own filesystem, within this module's own path arithmetic.
///
/// Under [`new`](LocalOperations::new) the three effecting operations are refusals, and the reason
/// is not policy: there is no boundary here to put an effect behind. Under
/// [`unconfined`](LocalOperations::unconfined) they do the thing, and a reader of *that* call site
/// has been told what is and is not underneath it.
impl Operations for LocalOperations {
    fn file_read(&self, path: &str, window: ReadWindow) -> Result<Value, String> {
        self.read(path, window)
    }

    fn dir_list(&self, path: &str) -> Result<Value, String> {
        self.list(path)
    }

    fn search(&self, pattern: &str, path: &str, options: &SearchOptions) -> Result<Value, String> {
        self.grep(pattern, path, options)
    }

    fn find(&self, glob: &str, path: &str, max_results: Option<usize>) -> Result<Value, String> {
        self.find_paths(glob, path, max_results)
    }

    // The three below refuse unless the caller asked for them by name. The catalogue never offers
    // what `writes()` denies, so a model cannot reach a refusal here — they are for a caller who
    // went around the catalogue, who is answered rather than served.

    fn file_write(&self, path: &str, text: &str) -> Result<Value, String> {
        if self.programs.is_none() {
            return Err(Self::unavailable("file_write"));
        }
        self.write(path, text)
    }

    fn file_edit(&self, path: &str, old: &str, new: &str) -> Result<Value, String> {
        if self.programs.is_none() {
            return Err(Self::unavailable("file_edit"));
        }
        self.edit(path, old, new)
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
        self.resolve_new(path).map(|target| self.display(&target))
    }

    fn programs(&self) -> &[String] {
        self.programs.as_deref().unwrap_or_default()
    }

    fn writes(&self) -> bool {
        self.programs.is_some()
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use tempfile::{TempDir, tempdir};

    use super::*;

    fn workspace() -> TempDir {
        tempdir().expect("a temporary directory")
    }

    fn writing(root: &Path) -> LocalOperations {
        LocalOperations::unconfined(root, Vec::new()).expect("the workspace opens")
    }

    fn reading(root: &Path) -> LocalOperations {
        LocalOperations::new(root).expect("the workspace opens")
    }

    #[test]
    fn a_link_that_leads_nowhere_is_not_a_door_out_of_the_workspace() {
        // `Path::exists` follows links, so a dangling one used to look absent: the walk climbed to
        // the workspace root, containment passed, and the write created the file outside.
        let outside = workspace();
        let inside = workspace();
        let escaped = outside.path().join("escaped.txt");
        symlink(&escaped, inside.path().join("link")).expect("a link");

        let refusal = writing(inside.path())
            .file_write("link", "owned")
            .expect_err("refused");
        assert!(refusal.contains("link"), "{refusal}");
        assert!(!escaped.exists(), "the write followed the link out");
    }

    #[test]
    fn a_link_to_a_file_outside_the_workspace_does_not_get_to_overwrite_it() {
        let outside = workspace();
        let inside = workspace();
        let victim = outside.path().join("victim.txt");
        fs::write(&victim, "original").expect("the file is written");
        symlink(&victim, inside.path().join("link")).expect("a link");

        assert!(writing(inside.path()).file_write("link", "owned").is_err());
        assert_eq!(
            fs::read_to_string(&victim).expect("the file is read"),
            "original"
        );
    }

    #[test]
    fn a_link_that_stays_inside_the_workspace_is_still_a_way_to_reach_its_target() {
        // Refusing every link would break a workspace that uses one internally. The boundary is
        // where the link *lands*, so one that lands inside is followed and the write goes to the
        // target, under the target's own name.
        let inside = workspace();
        let target = inside.path().join("real.txt");
        fs::write(&target, "before").expect("the file is written");
        symlink(&target, inside.path().join("link")).expect("a link");

        let value = writing(inside.path())
            .file_write("link", "after")
            .expect("the write lands");
        assert_eq!(value["path"], json!("real.txt"));
        assert_eq!(
            fs::read_to_string(&target).expect("the file is read"),
            "after"
        );
    }

    #[test]
    fn where_a_path_lands_is_the_targets_own_name_and_a_new_path_is_its_own() {
        // What the scope is given to judge: the file the bytes reach, under the name it has.
        let inside = workspace();
        fs::create_dir(inside.path().join("target")).expect("the directory is made");
        fs::write(inside.path().join("target/x"), "built").expect("the file is written");
        symlink(inside.path().join("target/x"), inside.path().join("link")).expect("a link");
        let operations = writing(inside.path());

        assert_eq!(operations.lands("link").expect("lands"), "target/x");
        assert_eq!(operations.lands("./target/x").expect("lands"), "target/x");
        assert_eq!(
            operations.lands("new/dir/file.txt").expect("lands"),
            "new/dir/file.txt"
        );
        assert!(
            operations.lands("../escape.txt").is_err(),
            "outside is the write's refusal, and this answers the same"
        );
    }

    #[test]
    fn a_new_file_under_directories_that_do_not_exist_yet_is_refused_without_side_effects() {
        let inside = workspace();
        let refusal = writing(inside.path())
            .file_write("new/dir/file.txt", "hello")
            .expect_err("a missing parent is refused");
        assert!(
            refusal.contains("parent directory that does not exist"),
            "{refusal}"
        );
        assert!(!inside.path().join("new").exists());
    }

    #[test]
    fn a_path_that_climbs_out_or_starts_at_the_root_is_refused() {
        let inside = workspace();
        let operations = writing(inside.path());
        assert!(operations.file_write("../escape.txt", "owned").is_err());
        assert!(
            operations
                .file_write("/etc/b10x-escape.txt", "owned")
                .is_err()
        );
        let sibling = inside
            .path()
            .parent()
            .expect("a temporary directory has a parent")
            .join("escape.txt");
        assert!(!sibling.exists(), "the write escaped upwards");
    }

    #[test]
    fn a_large_file_is_bounded_before_it_is_read_rather_than_after() {
        let inside = workspace();
        fs::write(
            inside.path().join("big.txt"),
            "filler\n".repeat(30 * 1024) + "last\n",
        )
        .expect("the file is written");

        let value = reading(inside.path())
            .file_read(
                "big.txt",
                ReadWindow {
                    max_bytes: Some(1024),
                    ..ReadWindow::whole()
                },
            )
            .expect("the read answers");
        assert_eq!(value["bytes"], json!(215_045), "the whole file's size");
        assert_eq!(value["truncated"], json!(true));
        assert_eq!(value["lines"]["total"], json!(30 * 1024 + 1));
        assert_eq!(
            value["lines"]["to"],
            json!(1024 / 7),
            "as many whole lines as the byte ceiling holds, and not one past it"
        );
    }

    #[test]
    fn a_first_line_over_the_requested_byte_window_is_visibly_truncated() {
        let inside = workspace();
        fs::write(inside.path().join("one.txt"), "abcdefghij\n").expect("written");
        let value = reading(inside.path())
            .file_read(
                "one.txt",
                ReadWindow {
                    max_bytes: Some(4),
                    ..ReadWindow::whole()
                },
            )
            .expect("the bounded first line is answered");
        assert_eq!(value["truncated"], json!(true));
        assert_eq!(value["truncated_lines"], json!([1]));
        assert_eq!(value["text"], json!("     1\tabc\n"));
    }

    #[test]
    fn a_line_too_long_to_answer_whole_is_cut_on_a_character_boundary_and_named() {
        // The cut is at a character count, so a three-byte character cannot be halved by the byte
        // cap that holds the line while it is read. U+FFFD in the reply would read as damage to
        // the file rather than to this answer.
        let inside = workspace();
        fs::write(inside.path().join("wide.txt"), "€".repeat(3_000)).expect("the file is written");

        let value = reading(inside.path())
            .file_read("wide.txt", ReadWindow::whole())
            .expect("the read answers");
        assert_eq!(
            value["truncated_lines"],
            json!([1]),
            "the cut is named by line number"
        );
        assert_eq!(value["truncated"], json!(true));
        assert_eq!(
            value["text"],
            json!(format!("     1\t{}\n", "€".repeat(MAX_READ_LINE_CHARS))),
            "cut at the character count, with no replacement character in it"
        );
    }

    #[test]
    fn a_read_answers_numbered_lines_and_says_which_window_of_the_file_they_are() {
        // The shape a model can quote back to `file_edit`: the number is a prefix it strips, and a
        // window is never mistaken for the file because `lines.total` is there.
        let inside = workspace();
        fs::write(inside.path().join("five.rs"), "a\nb\nc\nd\ne\n").expect("the file is written");
        let local = reading(inside.path());

        let value = local
            .file_read("five.rs", ReadWindow::lines(2, 2))
            .expect("the read answers");
        assert_eq!(value["text"], json!("     2\tb\n     3\tc\n"));
        assert_eq!(value["lines"], json!({"from": 2, "to": 3, "total": 5}));
        assert_eq!(value["truncated"], json!(true), "line 5 is not in it");

        let value = local
            .file_read("five.rs", ReadWindow::lines(4, 99))
            .expect("the read answers");
        assert_eq!(value["text"], json!("     4\td\n     5\te\n"));
        assert_eq!(value["lines"], json!({"from": 4, "to": 5, "total": 5}));
        assert_eq!(
            value["truncated"],
            json!(false),
            "a window that ends at the last line has reached the end"
        );
    }

    #[test]
    fn a_window_that_starts_past_the_end_is_refused_with_the_number_of_lines_there_are() {
        let inside = workspace();
        fs::write(inside.path().join("three.txt"), "a\nb\nc\n").expect("the file is written");
        let local = reading(inside.path());

        let refusal = local
            .file_read("three.txt", ReadWindow::lines(9, 10))
            .expect_err("refused");
        assert!(refusal.contains("has 3 lines"), "{refusal}");
        assert!(refusal.contains("line 9"), "{refusal}");

        let refusal = local
            .file_read(
                "three.txt",
                ReadWindow {
                    offset: Some(0),
                    ..ReadWindow::whole()
                },
            )
            .expect_err("refused");
        assert!(refusal.contains("numbered from 1"), "{refusal}");

        // An empty file read from its first line is the file, not a window past its end.
        fs::write(inside.path().join("empty.txt"), "").expect("the file is written");
        let value = local
            .file_read("empty.txt", ReadWindow::whole())
            .expect("the read answers");
        assert_eq!(value["text"], json!(""));
        assert_eq!(value["lines"], json!({"from": 1, "to": 0, "total": 0}));
    }

    #[test]
    fn a_match_that_was_cut_says_so() {
        // Invariant 8: nothing is truncated silently. A 400-character cap is fine; a cap the
        // reader cannot see is a line the model believes it has read whole.
        let inside = workspace();
        let long = format!("{}needle{}", "a".repeat(500), "b".repeat(500));
        fs::write(inside.path().join("long.txt"), format!("{long}\nneedle\n"))
            .expect("the file is written");

        let value = reading(inside.path())
            .search("needle", ".", &SearchOptions::default())
            .expect("the search answers");
        let matches = value["matches"].as_array().expect("matches");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0]["line_truncated"], json!(true));
        assert_eq!(
            matches[0]["text"].as_str().expect("text").chars().count(),
            MAX_MATCH_CHARS
        );
        assert_eq!(matches[1]["line_truncated"], json!(false));
    }

    #[test]
    fn search_names_large_files_it_did_not_inspect() {
        let inside = workspace();
        fs::write(
            inside.path().join("large.txt"),
            format!(
                "needle{}",
                "x".repeat(
                    usize::try_from(MAX_GREP_FILE_BYTES).expect("the search bound fits usize")
                )
            ),
        )
        .expect("large file");
        let value = reading(inside.path())
            .search("needle", ".", &SearchOptions::default())
            .expect("search answers incompletely");
        assert_eq!(value["matches"], json!([]));
        assert_eq!(value["truncated"], json!(true));
        assert_eq!(value["skipped_large_files"], json!(1));
        assert_eq!(value["skipped_large_paths"], json!(["large.txt"]));
    }

    #[test]
    fn search_checks_the_large_file_bound_in_bytes_at_both_edges() {
        let inside = workspace();
        let limit = usize::try_from(MAX_GREP_FILE_BYTES).expect("the search bound fits usize");
        for (name, total) in [("below.txt", limit - 1), ("exact.txt", limit)] {
            let prefix = "needle ";
            fs::write(
                inside.path().join(name),
                format!("{prefix}{}", "x".repeat(total - prefix.len())),
            )
            .expect("bounded file");
        }
        let prefix = "needle é";
        fs::write(
            inside.path().join("multibyte.txt"),
            format!("{prefix}{}", "x".repeat(limit - prefix.len())),
        )
        .expect("bounded multibyte file");
        fs::write(
            inside.path().join("over.txt"),
            format!("needle {}", "x".repeat(limit + 1 - "needle ".len())),
        )
        .expect("over-bound file");

        let value = reading(inside.path())
            .search("needle", ".", &SearchOptions::default())
            .expect("search answers and marks the skipped file");
        let paths = value["matches"]
            .as_array()
            .expect("matches")
            .iter()
            .map(|hit| hit["path"].as_str().expect("path"))
            .collect::<Vec<_>>();
        assert_eq!(paths, vec!["below.txt", "exact.txt", "multibyte.txt"]);
        assert_eq!(value["skipped_large_files"], json!(1));
        assert_eq!(value["skipped_large_paths"], json!(["over.txt"]));
        assert_eq!(value["truncated"], json!(true));
    }

    #[test]
    fn a_link_to_a_directory_is_listed_as_neither_a_file_nor_a_directory() {
        let inside = workspace();
        fs::write(inside.path().join("a.txt"), "x").expect("the file is written");
        fs::create_dir(inside.path().join("sub")).expect("the directory is made");
        symlink(inside.path().join("sub"), inside.path().join("zlink")).expect("a link");

        let value = reading(inside.path())
            .dir_list(".")
            .expect("the listing answers");
        let entries = value["entries"].as_array().expect("entries");
        let kinds = entries
            .iter()
            .map(|entry| entry["kind"].as_str().expect("a kind"))
            .collect::<Vec<_>>();
        assert_eq!(kinds, vec!["file", "directory", "symlink"]);
        assert_eq!(entries[2]["bytes"], Value::Null);
    }

    #[test]
    fn a_declared_program_is_started_without_this_process_s_environment() {
        // Cargo puts `CARGO_MANIFEST_DIR` in every test process, so it stands in here for every
        // credential a real run holds while the model chooses a child's arguments.
        let env = Path::new("/usr/bin/env");
        if !env.exists() {
            return;
        }
        let inside = workspace();
        let operations =
            LocalOperations::unconfined(inside.path(), vec!["/usr/bin/env".to_owned()])
                .expect("the workspace opens");

        let value = operations
            .run(&["/usr/bin/env".to_owned()])
            .expect("the program runs");
        let stdout = value["stdout"].as_str().expect("stdout");
        assert!(!stdout.contains("CARGO_MANIFEST_DIR="), "{stdout}");
        assert!(stdout.contains("PATH="), "{stdout}");
    }

    #[test]
    fn a_program_is_bounded_by_what_the_run_has_left_and_the_result_says_so() {
        let sleep = Path::new("/bin/sleep");
        if !sleep.exists() {
            return;
        }
        let inside = workspace();
        let operations = LocalOperations::unconfined(inside.path(), vec!["/bin/sleep".to_owned()])
            .expect("the workspace opens");

        let started = Instant::now();
        let value = operations
            .run_within(
                &["/bin/sleep".to_owned(), "30".to_owned()],
                Some(Duration::from_millis(200)),
            )
            .expect("the program starts");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the program was killed at the bound, not at its own end"
        );
        assert_eq!(value["timed_out"], json!(true));
        assert_eq!(value["timeout_ms"], json!(200));

        // No deadline is the module's own ceiling, not a bound of nothing.
        let value = operations
            .run(&["/bin/sleep".to_owned(), "0".to_owned()])
            .expect("the program runs");
        assert_eq!(value["timed_out"], json!(false));
        assert_eq!(value["timeout_ms"], json!(MAX_RUN_SECONDS * 1000));
    }

    #[cfg(unix)]
    #[test]
    fn a_timeout_kills_descendants_that_hold_the_output_pipes() {
        let shell = Path::new("/bin/sh");
        if !shell.exists() {
            return;
        }
        let inside = workspace();
        let operations = LocalOperations::unconfined(inside.path(), vec!["/bin/sh".to_owned()])
            .expect("the workspace opens");

        let started = Instant::now();
        let value = operations
            .run_within(
                &[
                    "/bin/sh".to_owned(),
                    "-c".to_owned(),
                    "sleep 30 & wait".to_owned(),
                ],
                Some(Duration::from_millis(200)),
            )
            .expect("the process group is stopped");
        assert_eq!(value["timed_out"], json!(true));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "a descendant kept a pipe open after the direct child was killed"
        );
    }

    #[test]
    fn output_over_the_cap_keeps_both_ends_and_the_marker_names_what_went_missing() {
        // The tail is where a test suite's verdict is. Keeping the head alone handed a model the
        // compiler's progress and dropped `test result: FAILED`.
        let seq = Path::new("/usr/bin/seq");
        if !seq.exists() {
            return;
        }
        let inside = workspace();
        let operations =
            LocalOperations::unconfined(inside.path(), vec!["/usr/bin/seq".to_owned()])
                .expect("the workspace opens");

        let value = operations
            .run(&[
                "/usr/bin/seq".to_owned(),
                "1".to_owned(),
                "100000".to_owned(),
            ])
            .expect("the program runs");
        let stdout = value["stdout"].as_str().expect("stdout");
        assert_eq!(value["truncated"], json!(true));
        assert!(stdout.starts_with("1\n2\n3\n"), "the head survives");
        assert!(
            stdout.ends_with("\n99999\n100000\n"),
            "and so does the tail"
        );
        let omitted = value["omitted_bytes"].as_u64().expect("a byte count");
        assert!(omitted > 0);
        assert!(
            stdout.contains(&format!("… {omitted} bytes omitted here …")),
            "and the marker names the count the result reports"
        );
        // 588,895 bytes in, 64 KiB out, and every byte accounted for in one of the three.
        assert_eq!(
            usize::try_from(omitted).expect("a byte count that fits") + stdout.len()
                - format!("\n… {omitted} bytes omitted here …\n").len(),
            588_895
        );
    }

    #[test]
    fn a_search_can_be_a_regular_expression_and_a_broken_one_is_refused_by_the_regexs_own_words() {
        let inside = workspace();
        fs::write(inside.path().join("a.rs"), "fn alpha() {}\nfn beta() {}\n")
            .expect("the file is written");
        let local = reading(inside.path());

        let value = local
            .search(
                r"fn\s+(alpha|gamma)",
                ".",
                &SearchOptions {
                    regex: true,
                    ..SearchOptions::default()
                },
            )
            .expect("the search answers");
        let matches = value["matches"].as_array().expect("matches");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["line"], json!(1));
        assert_eq!(value["regex"], json!(true));

        // Literal is still the default, so a model that meant `a.b` gets `a.b`.
        let value = local
            .search("fn (alpha", ".", &SearchOptions::default())
            .expect("the search answers");
        assert!(value["matches"].as_array().expect("matches").is_empty());

        let refusal = local
            .search(
                "fn (alpha",
                ".",
                &SearchOptions {
                    regex: true,
                    ..SearchOptions::default()
                },
            )
            .expect_err("refused");
        assert!(refusal.contains("not a regular expression"), "{refusal}");
        assert!(
            refusal.contains("unclosed group"),
            "with the regex's own words: {refusal}"
        );
    }

    #[test]
    fn a_glob_narrows_a_search_to_the_files_it_names_and_context_comes_back_numbered() {
        let inside = workspace();
        fs::create_dir(inside.path().join("src")).expect("the directory is made");
        fs::write(inside.path().join("src/one.rs"), "a\nb\nneedle\nd\ne\n")
            .expect("the file is written");
        fs::write(inside.path().join("notes.md"), "needle\n").expect("the file is written");
        let local = reading(inside.path());

        let value = local
            .search(
                "needle",
                ".",
                &SearchOptions {
                    glob: Some("*.rs".to_owned()),
                    context: Some(1),
                    ..SearchOptions::default()
                },
            )
            .expect("the search answers");
        let matches = value["matches"].as_array().expect("matches");
        assert_eq!(matches.len(), 1, "the markdown file is not a `*.rs`");
        assert_eq!(matches[0]["path"], json!("src/one.rs"));
        assert_eq!(
            matches[0]["before"],
            json!([{"line": 2, "text": "b", "line_truncated": false}])
        );
        assert_eq!(
            matches[0]["after"],
            json!([{"line": 4, "text": "d", "line_truncated": false}])
        );

        // A glob with a separator in it is matched against the whole workspace-relative path.
        let value = local
            .search(
                "needle",
                ".",
                &SearchOptions {
                    glob: Some("src/**".to_owned()),
                    ..SearchOptions::default()
                },
            )
            .expect("the search answers");
        assert_eq!(value["matches"].as_array().expect("matches").len(), 1);
        assert!(
            value["matches"][0].get("before").is_none(),
            "context is answered only where it was asked for"
        );
    }

    #[test]
    fn find_answers_workspace_relative_paths_and_never_the_machines_own_output() {
        let inside = workspace();
        fs::create_dir_all(inside.path().join("crates/x/src")).expect("the directories are made");
        fs::create_dir_all(inside.path().join("target/debug")).expect("the directories are made");
        fs::write(inside.path().join("crates/x/src/lib.rs"), "x").expect("the file is written");
        fs::write(inside.path().join("top.rs"), "x").expect("the file is written");
        fs::write(inside.path().join("target/debug/built.rs"), "x").expect("the file is written");
        let local = reading(inside.path());

        // No separator in the glob, so it is the file's own name, anywhere in the tree — the rule
        // a person means by `*.rs`.
        let value = local.find("*.rs", ".", None).expect("the find answers");
        assert_eq!(
            value["paths"],
            json!(["crates/x/src/lib.rs", "top.rs"]),
            "and `target` is machine output, never an answer"
        );
        assert_eq!(value["truncated"], json!(false));

        // A separator makes it the whole path, and `*` does not cross one.
        let value = local
            .find("crates/**/*.rs", ".", None)
            .expect("the find answers");
        assert_eq!(value["paths"], json!(["crates/x/src/lib.rs"]));

        let value = local.find("*.rs", ".", Some(1)).expect("the find answers");
        assert_eq!(value["paths"].as_array().expect("paths").len(), 1);
        assert_eq!(
            value["truncated"],
            json!(true),
            "a cut list says it was cut"
        );

        let refusal = local.find("crates/[", ".", None).expect_err("refused");
        assert!(refusal.contains("not a glob"), "{refusal}");
    }

    #[test]
    fn a_walk_stopped_by_the_depth_bound_says_so_and_one_that_reached_the_bottom_says_it_did_not() {
        // `find` says it lists every match under the workspace and `search` that it reads every
        // file. Both stop descending at `MAX_GREP_DEPTH`, and both used to answer
        // `truncated: false` over the part of the tree they had seen.
        let deep = workspace();
        let mut path = deep.path().to_path_buf();
        for level in 0..14 {
            path = path.join(format!("d{level}"));
        }
        fs::create_dir_all(&path).expect("the directories are made");
        fs::write(path.join("buried.txt"), "needle\n").expect("the file is written");
        let local = reading(deep.path());

        let value = local.find("*.txt", ".", None).expect("the find answers");
        assert_eq!(value["depth_bound_reached"], json!(true));
        assert_eq!(
            value["paths"],
            json!([]),
            "the file is past the bound, and the flag is the only thing that says so"
        );

        let value = local
            .search("needle", ".", &SearchOptions::default())
            .expect("the search answers");
        assert_eq!(value["depth_bound_reached"], json!(true));

        let shallow = workspace();
        fs::create_dir_all(shallow.path().join("a/b")).expect("the directories are made");
        fs::write(shallow.path().join("a/b/near.txt"), "needle\n").expect("the file is written");
        let local = reading(shallow.path());

        let value = local.find("*.txt", ".", None).expect("the find answers");
        assert_eq!(value["depth_bound_reached"], json!(false));
        assert_eq!(value["paths"], json!(["a/b/near.txt"]));
        assert_eq!(
            local
                .search("needle", ".", &SearchOptions::default())
                .expect("the search answers")["depth_bound_reached"],
            json!(false)
        );
    }

    #[test]
    fn counting_lines_stops_at_a_bound_and_the_reply_says_where_rather_than_answering_a_part_count()
    {
        // Reading to the end to count lines is a full scan of a multi-gigabyte artefact on every
        // read. Past the bound the count is not known, so it is `null` — never the part of it this
        // read happened to reach, which would be read as the whole file's.
        let inside = workspace();
        let line = format!("{}\n", "x".repeat(1_023));
        fs::write(inside.path().join("huge.txt"), line.repeat(17 * 1_024))
            .expect("the file is written");
        let local = reading(inside.path());

        let value = local
            .file_read("huge.txt", ReadWindow::lines(1, 2))
            .expect("the read answers");
        assert_eq!(value["lines"]["total"], Value::Null, "absence, not a count");
        assert_eq!(
            value["lines_counted_to"],
            json!(MAX_LINE_COUNT_BYTES / 1_024),
            "the last line the scan reached"
        );
        assert_eq!(value["truncated"], json!(true));
        assert_eq!(
            value["bytes"],
            json!(17 * 1_024 * 1_024),
            "the size is metadata's and is known whatever the scan did"
        );
        assert!(
            value["note"]
                .as_str()
                .expect("a note")
                .contains("not known"),
            "{}",
            value["note"]
        );

        // And a window past where the scan reached is refused by that, not by a line count nobody
        // finished.
        let refusal = local
            .file_read("huge.txt", ReadWindow::lines(20_000, 5))
            .expect_err("refused");
        assert!(refusal.contains("stops counting lines"), "{refusal}");
        assert!(refusal.contains("line 20000"), "{refusal}");
    }

    #[test]
    fn a_link_out_of_the_workspace_is_not_a_way_for_find_or_search_to_read_what_is_outside() {
        // The containment re-check inside `walk` is the boundary these two stand on: `is_dir` and
        // `read_dir` both follow links, so a link to a directory outside would be walked and its
        // files answered under workspace-relative names.
        let outside = workspace();
        fs::create_dir(outside.path().join("private")).expect("the directory is made");
        fs::write(outside.path().join("private/secret.txt"), "needle\n")
            .expect("the file is written");
        fs::write(outside.path().join("loose.txt"), "needle\n").expect("the file is written");

        let inside = workspace();
        fs::write(inside.path().join("own.txt"), "needle\n").expect("the file is written");
        symlink(outside.path(), inside.path().join("out")).expect("a link to a directory");
        symlink(
            outside.path().join("loose.txt"),
            inside.path().join("out.txt"),
        )
        .expect("a link to a file");
        let local = reading(inside.path());

        let value = local.find("*.txt", ".", None).expect("the find answers");
        assert_eq!(
            value["paths"],
            json!(["own.txt"]),
            "neither the linked directory's files nor the linked file itself"
        );

        let value = local
            .search("needle", ".", &SearchOptions::default())
            .expect("the search answers");
        let paths: Vec<&str> = value["matches"]
            .as_array()
            .expect("matches")
            .iter()
            .map(|hit| hit["path"].as_str().expect("a path"))
            .collect();
        assert_eq!(paths, vec!["own.txt"], "and nothing outside was read");
    }

    #[test]
    fn the_two_ends_kept_of_an_oversized_stream_are_whole_characters_and_the_marker_counts_exactly()
    {
        // On generated bytes, so it checks the arithmetic on every machine. The one test that had
        // this drove `/usr/bin/seq` and returned early where there is none, which is a check that
        // is not a check.
        let body = "€".repeat(50_000);
        let bytes = body.as_bytes();
        let half = 1_000;
        let cut = keep_both_ends(
            &bytes[..half],
            &bytes[bytes.len() - half..],
            bytes.len() as u64,
        );

        assert!(
            !cut.text.contains('\u{fffd}'),
            "a cut through a character would read as damage to the stream"
        );
        assert!(cut.text.starts_with(&"€".repeat(333)), "the head survives");
        assert!(cut.text.ends_with(&"€".repeat(333)), "and so does the tail");
        assert_eq!(
            cut.omitted,
            150_000 - 999 - 999,
            "the bytes dropped for a character boundary are counted too"
        );
        let marker = format!("\n… {} bytes omitted here …\n", cut.omitted);
        assert!(cut.text.contains(marker.trim()), "{}", cut.text);
        assert_eq!(
            cut.omitted + cut.text.len() as u64 - marker.len() as u64,
            150_000,
            "every byte is in the head, the tail or the marker's count"
        );
    }

    #[test]
    fn a_stream_whose_two_ends_are_the_whole_of_it_keeps_its_last_line_and_writes_no_marker() {
        let body = format!("{}test result: FAILED\n", "filler\n".repeat(100));
        let bytes = body.as_bytes();
        let cut = keep_both_ends(&bytes[..300], &bytes[300..], bytes.len() as u64);

        assert_eq!(cut.omitted, 0);
        assert_eq!(cut.text, body, "nothing was dropped, so nothing is marked");
        assert!(cut.text.ends_with("test result: FAILED\n"));
    }

    #[test]
    fn a_regular_expression_too_large_for_this_builds_limit_is_refused_by_the_crates_own_words() {
        // The pattern comes from the model, and the crate's default ceiling is ten megabytes of
        // this process's memory for one `search`.
        let inside = workspace();
        fs::write(inside.path().join("a.txt"), "aaa\n").expect("the file is written");

        let refusal = reading(inside.path())
            .search(
                "(a{1000}){1000}",
                ".",
                &SearchOptions {
                    regex: true,
                    ..SearchOptions::default()
                },
            )
            .expect_err("refused");
        assert!(refusal.contains("not a regular expression"), "{refusal}");
        assert!(
            refusal.contains("size limit"),
            "with the crate's own words: {refusal}"
        );
    }

    #[test]
    fn a_crlf_file_reads_here_exactly_as_it_reads_through_a_confined_workspace() {
        // The confined provider splits with `str::lines`, which drops the `\r`. Keeping it here
        // made one file read differently on the two providers, and a `\r` quoted back to
        // `file_edit` matched nothing in a file the model had just been handed.
        let inside = workspace();
        fs::write(inside.path().join("dos.txt"), "alpha\r\nbeta\r\n").expect("the file is written");

        let value = reading(inside.path())
            .file_read("dos.txt", ReadWindow::whole())
            .expect("the read answers");
        assert_eq!(value["text"], json!("     1\talpha\n     2\tbeta\n"));
        assert_eq!(
            value["truncated_lines"],
            json!([]),
            "a dropped `\\r` is not a cut line"
        );
        assert_eq!(value["truncated"], json!(false));
        assert_eq!(value["lines"], json!({"from": 1, "to": 2, "total": 2}));
    }

    #[test]
    fn an_empty_glob_is_refused_by_name_and_a_capped_context_is_echoed_not_applied_silently() {
        let inside = workspace();
        fs::write(
            inside.path().join("a.txt"),
            "1\n2\n3\n4\n5\n6\n7\n8\nneedle\n",
        )
        .expect("the file is written");
        let local = reading(inside.path());

        // An empty glob matched nothing and answered an empty list, which reads as *there are no
        // such files* rather than as *you named no pattern*.
        let refusal = local.find("", ".", None).expect_err("refused");
        assert!(refusal.contains("`glob` is required"), "{refusal}");

        let value = local
            .search(
                "needle",
                ".",
                &SearchOptions {
                    context: Some(40),
                    ..SearchOptions::default()
                },
            )
            .expect("the search answers");
        assert_eq!(value["context"], json!(MAX_SEARCH_CONTEXT));
        assert!(
            value["note"].as_str().expect("a note").contains("40"),
            "{}",
            value["note"]
        );
        assert_eq!(
            value["matches"][0]["before"]
                .as_array()
                .expect("before")
                .len(),
            usize::try_from(MAX_SEARCH_CONTEXT).expect("five fits"),
            "and the matches carry the figure the reply echoed"
        );

        // Asking for what the cap allows is not a cap, so nothing is echoed.
        let value = local
            .search(
                "needle",
                ".",
                &SearchOptions {
                    context: Some(2),
                    ..SearchOptions::default()
                },
            )
            .expect("the search answers");
        assert!(value.get("context").is_none(), "{value}");
        assert!(value.get("note").is_none(), "{value}");
    }

    #[test]
    fn a_caller_can_name_more_of_the_environment_and_only_what_it_named_crosses() {
        // The same stand-in: cargo sets it in every test process, so letting it through by name
        // is observable without setting anything in this process.
        let env = Path::new("/usr/bin/env");
        if !env.exists() {
            return;
        }
        let inside = workspace();
        let operations =
            LocalOperations::unconfined(inside.path(), vec!["/usr/bin/env".to_owned()])
                .expect("the workspace opens")
                .inheriting(["CARGO_MANIFEST_DIR"]);

        let value = operations
            .run(&["/usr/bin/env".to_owned()])
            .expect("the program runs");
        let stdout = value["stdout"].as_str().expect("stdout");
        assert!(stdout.contains("CARGO_MANIFEST_DIR="), "{stdout}");
        assert!(!stdout.contains("CARGO_PKG_NAME="), "{stdout}");
    }
}
