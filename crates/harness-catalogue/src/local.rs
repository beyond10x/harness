//! A bounded view of one directory, with no confinement under it, over the `tools` crates.
//!
//! # The provider with no boundary, and why it still exists
//!
//! Reading a tree is [`tool_fs`]'s, searching it [`tool_search`]'s, writing one
//! [`tool_code_editor`]'s and running a declared program in it [`tool_process`]'s: every path check
//! is that crate's own, canonicalising before a containment check, and the declared set, the
//! cleared environment and the bound on a run are `tool_process`'s. That is real and it is not
//! confinement — nothing stops the *process*, only those functions, and a bug in them is the whole
//! boundary.
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

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use tool_code_editor::{EditInput, Editor, WriteInput};
use tool_fs::{Fs, GlobInput, ListInput, ReadInput};
use tool_process::{Process, ProcessError, RunInput};
use tool_search::{GrepInput, Search};

use crate::{Operations, ReadWindow, Refusal, Refused, SearchOptions};

/// The **declared** harness-name↔tools-id correspondence: `(harness, tools)`.
///
/// Not the router — each `Operations` method below calls its tools operation directly, by name, in
/// code. This table is what a consumer and the two-way test read instead: [`tools_operation`] and
/// [`harness_operation`] answer it in each direction, and `every_mapped_id_is_an_operation_the_tools_crates_expose`
/// is what keeps it honest against the tools crates, so the declaration cannot drift from what the
/// adapter actually calls.
///
/// Every harness operation is here, `run` included since it moved onto `b10x-tool-process`. The
/// recorded operation name (`file.read`, `shell`) is a third thing and is not this table;
/// metaharness reads that back, and renaming it is not renaming this.
pub(crate) const MAPPING: &[(&str, &str)] = &[
    ("file_read", "fs.read"),
    ("dir_list", "fs.list"),
    ("search", "search.grep"),
    ("find", "fs.glob"),
    ("file_write", "code.write"),
    ("file_edit", "code.edit"),
    ("run", "process.run"),
];

/// The `tools` operation id a harness operation is performed by, or `None` for a name that is not
/// a harness operation.
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

/// A bounded view of one directory, read through [`tool_fs`], searched through [`tool_search`],
/// written through [`tool_code_editor`] and run in through [`tool_process`].
#[derive(Debug, Clone)]
pub struct LocalOperations {
    root: PathBuf,
    /// Reads: `fs.read`, `fs.list`, `fs.glob`.
    fs: Fs,
    /// Search: `search.grep`.
    search: Search,
    /// Writes: `code.write`, `code.edit`. Held whether or not writing is offered; whether it is
    /// reachable is `process`, exactly as the catalogue's gate reads it, so holding it publishes
    /// nothing.
    editor: Editor,
    /// Runs: `process.run`. `None` is read-only. `Some` holds the declared program set, which may
    /// itself be empty — a provider that writes files and starts nothing.
    process: Option<Process>,
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

    /// Names more of this process's environment that a declared program may see, beyond
    /// [`tool_process::INHERITED_ENV`].
    ///
    /// The default list is what a toolchain needs and nothing that carries a credential. A run
    /// that needs a proxy, or a variable its build reads, names it here — by name, at the call
    /// site, where a reader can see what was let through. A read-only view starts nothing, so on
    /// one this lets nothing through.
    #[must_use]
    pub fn inheriting(mut self, names: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.process = self.process.map(|process| process.inheriting(names));
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
        let process = programs
            .map(|programs| Process::new(&root, programs))
            .transpose()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            root: fs.root().to_path_buf(),
            fs,
            search,
            editor,
            process,
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
        if self.process.is_none() {
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
        if self.process.is_none() {
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

    /// `process.run` through [`tool_process`], its reply field for field.
    ///
    /// The call names no `timeout_ms` — the `run` entry publishes none — so the bound is
    /// `tool_process`'s ceiling capped by `remaining`, what the run has left, exactly as before
    /// `run` moved. The declared-set refusal stays harness's named one (`refused`).
    fn run_within(&self, argv: &[String], remaining: Option<Duration>) -> Result<Value, Refused> {
        let Some(process) = &self.process else {
            return Err(Self::unavailable("run").into());
        };
        let input = RunInput {
            argv: argv.to_vec(),
            timeout_ms: None,
        };
        let output = process.run(&input, remaining).map_err(refused)?;
        Ok(serde_json::to_value(output).expect("a run output serialises"))
    }

    /// The same resolution a write does, answered as the workspace-relative name the write would
    /// report — so the scope judges the file the bytes reach, under the name it actually has.
    fn lands(&self, path: &str) -> Result<String, String> {
        self.editor.lands(path).map_err(|error| error.to_string())
    }

    fn programs(&self) -> &[String] {
        self.process.as_ref().map_or(&[], Process::programs)
    }

    fn writes(&self) -> bool {
        self.process.is_some()
    }
}

/// A `process.run` refusal in harness's terms.
///
/// A program outside the declared set is harness's own named
/// [`Refusal::ProgramNotDeclared`], in its own words, as it was before `run` moved: the name is
/// what a reader of the record counts, and the sentence beside it is harness's to word. Every
/// other refusal is `tool_process`'s sentence, unnamed, as it was.
fn refused(error: ProcessError) -> Refused {
    match error {
        ProcessError::ProgramNotDeclared { program, declared } => {
            Refusal::ProgramNotDeclared { program, declared }.into()
        }
        other => other.to_string().into(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

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
        // `run` is `process.run`, read in both directions: every harness operation is now
        // performed by a tools operation.
        assert_eq!(tools_operation("run"), Some("process.run"));
        assert_eq!(harness_operation("process.run"), Some("run"));
        // The recorded operation name is not the table, and an id no harness operation uses maps
        // to nothing.
        assert_eq!(tools_operation("shell"), None);
        assert_eq!(harness_operation("fs.walk"), None);
    }

    #[test]
    fn every_mapped_id_is_an_operation_the_tools_crates_expose() {
        // The table cannot name an id the tools crates do not answer. The `fs`, `search` and
        // `process` ids are read off the contract each crate carries; `b10x-tool-code-editor`
        // keeps its parsed contract private, so its two writes are named here.
        let mut exposed: Vec<String> = tool_fs::contract()
            .operations
            .iter()
            .chain(&tool_search::contract().operations)
            .chain(&tool_process::contract().operations)
            .map(|operation| operation.name.clone())
            .collect();
        exposed.extend(["code.write".to_owned(), "code.edit".to_owned()]);
        for (_, id) in MAPPING {
            assert!(
                exposed.iter().any(|name| name == id),
                "{id} is not an operation of the fs, search, process or code-editor toolset: \
                 {exposed:?}"
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

    // --- `run` on `process.run` ------------------------------------------------------------------

    fn running(root: &Path) -> LocalOperations {
        LocalOperations::unconfined(root, vec!["/bin/sh".to_owned()]).expect("a workspace")
    }

    fn sh(script: &str) -> Vec<String> {
        vec!["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()]
    }

    /// The bound the acceptance cases run under, and the most a call may be held against it: a
    /// small multiple, room for the kill and for the half second `process.run` waits after it.
    const BOUND: Duration = Duration::from_millis(300);
    const HELD_AT_MOST: Duration = Duration::from_millis(1_500);

    /// The pid a script wrote to `file` in the workspace, and whether that process was still alive
    /// when asked; a live one is killed here, so no case leaves it running.
    #[cfg(unix)]
    fn reap(root: &Path, file: &str) -> (i32, bool) {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;

        let text = std::fs::read_to_string(root.join(file)).expect("the script wrote the pid");
        let pid: i32 = text.trim().parse().expect("a pid");
        let alive = kill(Pid::from_raw(pid), None).is_ok();
        if alive {
            let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
        }
        (pid, alive)
    }

    /// Whether `setsid` is on this process's search path, which is the one a started program gets.
    fn setsid_is_installed() -> bool {
        std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|dir| dir.join("setsid").is_file())
        })
    }

    #[test]
    fn run_answers_process_runs_reply_field_for_field() {
        // The adapter's wiring only: the declared set reaches `Process`, the argv reaches
        // `RunInput`, and the reply comes back with nothing stripped or added. Both sides are
        // `tool_process`; the drift guard against 0.15.1's reply is `tests/run_against_0_15_1.rs`.
        let dir = workspace();
        let argv = sh("printf out; printf err >&2; exit 3");
        let answer = running(dir.path())
            .run(&argv)
            .expect("the declared program runs");
        let direct = tool_process::Process::new(dir.path(), vec!["/bin/sh".to_owned()])
            .expect("a workspace")
            .run(&tool_process::RunInput::argv(argv.clone()), None)
            .expect("the run");
        assert_eq!(
            answer,
            serde_json::to_value(direct).expect("serialises"),
            "{answer}"
        );
    }

    #[test]
    fn what_the_run_has_left_is_the_bound_process_run_is_given() {
        let dir = workspace();
        let operations = running(dir.path());
        let answer = operations
            .run_within(&sh("exit 0"), Some(Duration::from_millis(1_234)))
            .expect("the run");
        assert_eq!(answer["timeout_ms"], serde_json::json!(1_234), "{answer}");
        let answer = operations
            .run_within(&sh("exit 0"), Some(Duration::from_secs(3_600)))
            .expect("the run");
        assert_eq!(
            answer["timeout_ms"],
            serde_json::json!(600_000),
            "capped at the ten-minute ceiling: {answer}"
        );
    }

    #[test]
    fn a_name_given_to_inheriting_reaches_the_program_and_no_other_does() {
        // `cargo test` starts this binary with `CARGO_PKG_NAME` set, and it is off the allow-list.
        assert!(
            std::env::var_os("CARGO_PKG_NAME").is_some(),
            "precondition: the test process holds a variable off the allow-list"
        );
        let dir = workspace();
        let argv = sh("printf '%s' \"${CARGO_PKG_NAME-unset}\"");
        let cleared = running(dir.path()).run(&argv).expect("the run");
        assert_eq!(cleared["stdout"], serde_json::json!("unset"), "{cleared}");
        let named = running(dir.path())
            .inheriting(["CARGO_PKG_NAME"])
            .run(&argv)
            .expect("the run");
        assert_eq!(
            named["stdout"],
            serde_json::json!(env!("CARGO_PKG_NAME")),
            "{named}"
        );
    }

    #[test]
    fn a_workspace_removed_after_it_opened_is_refused_as_the_workspace_and_not_the_program() {
        // A start in a root that is gone fails the same way as a program that is missing; 0.15.1
        // answered "`/bin/sh`: No such file or directory", blaming the program.
        let dir = workspace();
        let root = dir.path().join("tree");
        std::fs::create_dir(&root).expect("the root");
        let canonical = root.canonicalize().expect("the canonical root");
        let operations = running(&root);
        std::fs::remove_dir(&root).expect("the root is removed");
        let refused = operations.run(&sh("exit 0")).expect_err("refused");
        assert_eq!(
            refused.message(),
            format!(
                "workspace `{}`: No such file or directory (os error 2)",
                canonical.display()
            ),
        );
        assert_eq!(
            refused.refusal(),
            None,
            "a failure, not a rule of the run's"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_descendant_holding_stdout_does_not_hold_the_run_past_its_bound() {
        // story:run-moves-onto-tool-process's acceptance, argv for argv: the shell ends at once
        // and its background `sleep` keeps stdout open. harness's own runner stopped watching the
        // clock once the program ended and joined the streams with no deadline.
        let dir = workspace();
        let started = Instant::now();
        let answer = running(dir.path())
            .run_within(&sh("sleep 3 &"), Some(BOUND))
            .expect("the declared program runs");
        let held = started.elapsed();
        assert_eq!(answer["timed_out"], serde_json::json!(true), "{answer}");
        assert_eq!(answer["timeout_ms"], serde_json::json!(300), "{answer}");
        // The shell ended by itself and only its descendant was killed, so `exit` is the shell's.
        assert_eq!(answer["exit"], serde_json::json!(0), "{answer}");
        assert!(
            held < HELD_AT_MOST,
            "held {held:?} against a bound of {BOUND:?}: {answer}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_descendant_holding_stdout_is_killed_with_the_group_at_the_bound() {
        let dir = workspace();
        let started = Instant::now();
        let answer = running(dir.path())
            .run_within(
                &sh("sleep 3 & printf '%s' \"$!\" > descendant.pid"),
                Some(BOUND),
            )
            .expect("the declared program runs");
        let held = started.elapsed();
        let (pid, alive) = reap(dir.path(), "descendant.pid");
        assert_eq!(answer["timed_out"], serde_json::json!(true), "{answer}");
        assert!(
            held < HELD_AT_MOST,
            "held {held:?} against a bound of {BOUND:?}: {answer}"
        );
        assert!(!alive, "descendant {pid} outlived the call: {answer}");
    }

    #[cfg(unix)]
    #[test]
    fn a_setsid_descendant_does_not_hold_the_run_past_its_bound() {
        // story:run-moves-onto-tool-process's acceptance (revision 5): "a `setsid` descendant does
        // not hold the call past its bound". `process.run` kills the program's group only; a
        // process that left it is not killed and may outlive the call, so this case reaps it.
        if !setsid_is_installed() {
            eprintln!(
                "skipped: `setsid` is not on PATH, so no descendant can leave the group here"
            );
            return;
        }
        let dir = workspace();
        let started = Instant::now();
        let answer = running(dir.path())
            .run_within(
                &sh("setsid sleep 3 & printf '%s' \"$!\" > escaped.pid"),
                Some(BOUND),
            )
            .expect("the declared program runs");
        let held = started.elapsed();
        let _ = reap(dir.path(), "escaped.pid");
        assert_eq!(answer["timed_out"], serde_json::json!(true), "{answer}");
        assert!(
            held < HELD_AT_MOST,
            "held {held:?} against a bound of {BOUND:?}: {answer}"
        );
    }
}
