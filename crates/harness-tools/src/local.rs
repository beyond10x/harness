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
//! holds three entries: read, list, search.
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

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::Operations;

const MAX_LIST_ENTRIES: usize = 500;
const MAX_READ_BYTES: u64 = 64 * 1024;
const MAX_READ_BYTES_CEILING: u64 = 256 * 1024;
const MAX_GREP_RESULTS: usize = 200;
const MAX_GREP_FILE_BYTES: u64 = 1024 * 1024;
const MAX_GREP_DEPTH: usize = 12;
const MAX_RUN_OUTPUT_BYTES: usize = 64 * 1024;
/// How long an unconfined `run` may hold the turn open before it is killed.
///
/// A bound rather than none: `wait` on a child that never exits stops the run with no record of
/// why. Ten minutes is longer than any suite this has been pointed at and shorter than a night.
const MAX_RUN_SECONDS: u64 = 600;
/// How much of a matched line `search` reports. One minified file on a single line would
/// otherwise bury every other result, so the line is cut — and the match says that it was.
const MAX_MATCH_CHARS: usize = 400;

/// The only environment variables a declared program is started with.
///
/// An allow list rather than a deny list, because the parent process's environment is where this
/// run's credentials live and the child's arguments came from the model. Naming what may cross
/// keeps a token nobody thought about out of a program somebody else chose.
const INHERITED_ENV: &[&str] = &["PATH", "HOME", "LANG", "LC_ALL", "TERM", "TMPDIR"];

/// Directories skipped while walking. Each is either machine output or another tool's private
/// state, and including them buries the answer the person asked for.
const SKIPPED: &[&str] = &[".git", "target", "node_modules", ".venv", "__pycache__"];

#[derive(Debug, Clone)]
pub struct LocalOperations {
    root: PathBuf,
    /// `None` is read-only. `Some` is the declared program set, which may itself be empty — a
    /// provider that writes files and starts nothing.
    programs: Option<Vec<String>>,
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
        })
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

    fn read(&self, relative: &str, max_bytes: Option<u64>) -> Result<Value, String> {
        let limit = max_bytes
            .unwrap_or(MAX_READ_BYTES)
            .min(MAX_READ_BYTES_CEILING);
        let target = self.resolve(relative)?;
        if !target.is_file() {
            return Err(format!("`{relative}` is not a file"));
        }
        // The size comes from the metadata and only `limit` bytes are ever read. Reading the file
        // whole and then cutting the reply means a multi-gigabyte artefact in the workspace is
        // pulled into this process's memory to answer with 64 KiB of it.
        let total = fs::metadata(&target)
            .map_err(|error| format!("`{relative}`: {error}"))?
            .len();
        let mut head = Vec::new();
        fs::File::open(&target)
            .map_err(|error| format!("`{relative}`: {error}"))?
            .take(limit)
            .read_to_end(&mut head)
            .map_err(|error| format!("`{relative}`: {error}"))?;
        let truncated = total > limit;
        // On a character boundary. A cut through a multi-byte character becomes U+FFFD, which reads
        // as damage to the file rather than to the reply, so the last partial character is dropped
        // instead. `error_len() == None` is exactly "the bytes ran out mid-character": anything
        // else is the file's own encoding and is reported lossily, as before.
        let text = match std::str::from_utf8(&head) {
            Ok(text) => text.to_owned(),
            Err(error) if truncated && error.error_len().is_none() => {
                String::from_utf8_lossy(&head[..error.valid_up_to()]).into_owned()
            }
            Err(_) => String::from_utf8_lossy(&head).into_owned(),
        };
        Ok(json!({
            "path": self.display(&target),
            "bytes": total,
            "truncated": truncated,
            "text": text,
        }))
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
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("`{relative}`: {error}"))?;
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

    fn exec(&self, argv: &[String], programs: &[String]) -> Result<Value, String> {
        let Some(program) = argv.first() else {
            return Err("`argv` must name a program".to_owned());
        };
        if !programs.iter().any(|allowed| allowed == program) {
            return Err(format!(
                "`{program}` is not a program this run may start. Declared: {}.",
                if programs.is_empty() {
                    "none".to_owned()
                } else {
                    programs.join(", ")
                }
            ));
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
        for name in INHERITED_ENV {
            if let Ok(value) = std::env::var(name) {
                command.env(name, value);
            }
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("`{program}`: {error}"))?;

        // Drained on threads rather than after `wait`: a child that fills a pipe buffer blocks
        // until somebody reads it, and a `wait` that has not read is the deadlock.
        let out = child.stdout.take().expect("stdout was piped");
        let err = child.stderr.take().expect("stderr was piped");
        let out = std::thread::spawn(move || drain(out));
        let err = std::thread::spawn(move || drain(err));

        let deadline = Instant::now() + Duration::from_secs(MAX_RUN_SECONDS);
        let mut killed = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {}
                Err(error) => return Err(format!("`{program}`: {error}")),
            }
            if Instant::now() >= deadline {
                killed = true;
                let _ = child.kill();
                break child
                    .wait()
                    .map_err(|error| format!("`{program}`: {error}"))?;
            }
            std::thread::sleep(Duration::from_millis(25));
        };

        let (stdout, stdout_truncated) = out.join().unwrap_or_default();
        let (stderr, stderr_truncated) = err.join().unwrap_or_default();
        Ok(json!({
            "argv": argv,
            "exit": status.code(),
            "stdout": stdout,
            "stderr": stderr,
            "truncated": stdout_truncated || stderr_truncated,
            // Named, because a killed process's exit code says nothing about the task.
            "timed_out": killed,
        }))
    }

    fn grep(
        &self,
        pattern: &str,
        relative: &str,
        max_results: Option<usize>,
    ) -> Result<Value, String> {
        if pattern.is_empty() {
            return Err("`pattern` is required and must not be empty".to_owned());
        }
        let limit = max_results
            .unwrap_or(MAX_GREP_RESULTS)
            .min(MAX_GREP_RESULTS);
        let root = self.resolve(relative)?;

        let mut matches = Vec::new();
        let mut truncated = false;
        let boundary = self.root.clone();
        walk(&root, &boundary, 0, &mut |file| {
            if matches.len() >= limit {
                truncated = true;
                return false;
            }
            let Ok(metadata) = file.metadata() else {
                return true;
            };
            if metadata.len() > MAX_GREP_FILE_BYTES {
                return true;
            }
            let Ok(text) = fs::read_to_string(file) else {
                // Binary or unreadable. Skipping is right; reporting each one is noise.
                return true;
            };
            for (index, line) in text.lines().enumerate() {
                if matches.len() >= limit {
                    truncated = true;
                    return false;
                }
                if line.contains(pattern) {
                    matches.push(json!({
                        "path": self.display(file),
                        "line": index + 1,
                        "text": line.chars().take(MAX_MATCH_CHARS).collect::<String>(),
                        // Always present, so the shape is stable and a reader who checks the flag
                        // never has to wonder whether its absence means "whole" or "old reply".
                        "line_truncated": line.chars().nth(MAX_MATCH_CHARS).is_some(),
                    }));
                }
            }
            true
        });

        Ok(json!({
            "pattern": pattern,
            "matches": matches,
            "truncated": truncated,
        }))
    }
}

/// Walks files under `directory`, calling `visit` until it returns `false`.
///
/// Every entry is re-checked against `boundary`. Checking only the path the caller supplied is not
/// enough: `is_dir` and `read_dir` both follow symlinks, so a link inside the workspace pointing
/// anywhere else would be walked and its contents returned under a workspace-relative name.
///
/// Depth is bounded so a deep or cyclic tree cannot hold the run open.
fn walk(directory: &Path, boundary: &Path, depth: usize, visit: &mut dyn FnMut(&Path) -> bool) {
    if depth > MAX_GREP_DEPTH {
        return;
    }
    if !contained(directory, boundary) {
        return;
    }
    if directory.is_file() {
        visit(directory);
        return;
    }
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(std::fs::DirEntry::file_name);
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
            walk(&path, boundary, depth + 1, visit);
        } else if !visit(&path) {
            return;
        }
    }
}

/// Reads a child's stream to the end, keeping the first [`MAX_RUN_OUTPUT_BYTES`].
///
/// It keeps reading after the cap rather than stopping: a reader that walked away would block the
/// child on a full pipe, and the point of the cap is the size of the answer, not the child's fate.
fn drain(mut stream: impl std::io::Read) -> (String, bool) {
    let mut kept: Vec<u8> = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    while let Ok(read) = stream.read(&mut buffer) {
        if read == 0 {
            break;
        }
        let room = MAX_RUN_OUTPUT_BYTES.saturating_sub(kept.len());
        kept.extend_from_slice(&buffer[..read.min(room)]);
        truncated |= read > room;
    }
    (String::from_utf8_lossy(&kept).into_owned(), truncated)
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
    fn file_read(&self, path: &str, max_bytes: Option<u64>) -> Result<Value, String> {
        self.read(path, max_bytes)
    }

    fn dir_list(&self, path: &str) -> Result<Value, String> {
        self.list(path)
    }

    fn search(
        &self,
        pattern: &str,
        path: &str,
        max_results: Option<usize>,
    ) -> Result<Value, String> {
        self.grep(pattern, path, max_results)
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

    fn run(&self, argv: &[String]) -> Result<Value, String> {
        let Some(programs) = self.programs.as_deref() else {
            return Err(Self::unavailable("run"));
        };
        self.exec(argv, programs)
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
    fn a_new_file_under_directories_that_do_not_exist_yet_still_writes() {
        let inside = workspace();
        writing(inside.path())
            .file_write("new/dir/file.txt", "hello")
            .expect("the write lands");
        assert_eq!(
            fs::read_to_string(inside.path().join("new/dir/file.txt")).expect("the file is read"),
            "hello"
        );
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
        fs::write(inside.path().join("big.txt"), "x".repeat(200 * 1024))
            .expect("the file is written");

        let value = reading(inside.path())
            .file_read("big.txt", Some(1024))
            .expect("the read answers");
        assert_eq!(value["bytes"], json!(204_800));
        assert_eq!(value["truncated"], json!(true));
        assert!(value["text"].as_str().expect("text").len() <= 1024);
    }

    #[test]
    fn truncation_lands_on_a_character_boundary_rather_than_through_one() {
        let inside = workspace();
        fs::write(inside.path().join("accents.txt"), "é".repeat(16)).expect("the file is written");

        let value = reading(inside.path())
            .file_read("accents.txt", Some(3))
            .expect("the read answers");
        assert_eq!(value["truncated"], json!(true));
        assert_eq!(value["text"], json!("é"));
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
            .search("needle", ".", None)
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
}
