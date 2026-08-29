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

use std::time::Duration;

use harness_tools::Operations;
use serde_json::{Value, json};

use crate::{Backend, Facts};

/// What a confined workspace can do on this machine.
pub struct ConfinedOperations {
    backend: Box<dyn Backend>,
    workspace: String,
    programs: Vec<String>,
    writes: bool,
}

impl std::fmt::Debug for ConfinedOperations {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfinedOperations")
            .field("workspace", &self.workspace)
            .field("programs", &self.programs)
            .field("writes", &self.writes)
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
    pub fn new(
        backend: impl Backend + 'static,
        facts: &Facts,
        workspace: impl Into<String>,
        programs: Vec<String>,
    ) -> Self {
        let confines_execution = facts.confines_execution();
        Self {
            backend: Box::new(backend),
            workspace: workspace.into(),
            // A machine that cannot confine a process offers none, whatever was declared.
            programs: if confines_execution {
                programs
            } else {
                Vec::new()
            },
            writes: facts.holds_workspaces(),
        }
    }
}

/// How much of a file one read answers with unless the caller asks for less.
///
/// The same figure the unconfined provider uses, so a run's replies do not change shape when it is
/// confined.
const MAX_READ_BYTES: u64 = 64 * 1024;

/// The most a caller may ask for in one read, however large a number it names.
const MAX_READ_BYTES_CEILING: u64 = 256 * 1024;

impl Operations for ConfinedOperations {
    fn file_read(&self, path: &str, max_bytes: Option<u64>) -> Result<Value, String> {
        // Bounded here, and reported. The earlier note said a truncation this side "could not be
        // reported as one" — that was wrong: the whole text is in hand, so the exact total and the
        // fact of truncation are both known, which is all a reply needs to keep a partial read from
        // looking whole.
        //
        // It is bounded because a result is replayed on **every** later turn. A live run on
        // 2026-08-24 read three files in one turn and the next turn's replay grew by 24,630 tokens,
        // which then pushed the conversation past its bound and bought a prefix rewrite.
        let limit = max_bytes
            .unwrap_or(MAX_READ_BYTES)
            .min(MAX_READ_BYTES_CEILING);
        let text = self
            .backend
            .file_read(&self.workspace, path)
            .map_err(|error| error.to_string())?;
        let total = text.len() as u64;
        let truncated = total > limit;
        // On a character boundary, so the reply is still a string the model can read.
        let head = if truncated {
            let mut end = usize::try_from(limit).unwrap_or(text.len()).min(text.len());
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            &text[..end]
        } else {
            text.as_str()
        };
        Ok(json!({
            "path": path,
            "bytes": total,
            "truncated": truncated,
            "text": head,
        }))
    }

    fn file_write(&self, path: &str, text: &str) -> Result<Value, String> {
        self.backend
            .file_write(&self.workspace, path, text)
            .map_err(|error| error.to_string())
    }

    fn file_edit(&self, path: &str, old: &str, new: &str) -> Result<Value, String> {
        let current = self
            .backend
            .file_read(&self.workspace, path)
            .map_err(|error| error.to_string())?;
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

    fn search(&self, _p: &str, _path: &str, _max: Option<usize>) -> Result<Value, String> {
        Err(Self::unavailable("search through a confined workspace"))
    }

    fn run(&self, argv: &[String]) -> Result<Value, String> {
        self.run_within(argv, None)
    }

    fn run_within(&self, argv: &[String], remaining: Option<Duration>) -> Result<Value, String> {
        // The catalogue refuses an empty argv before it gets here, but this is a public trait
        // method and an embedder can call it directly; a refusal by name is what the unconfined
        // provider answers, and a panic mid-turn is not.
        let Some(program) = argv.first() else {
            return Err("`argv` must name a program".to_owned());
        };
        if !self.programs.iter().any(|allowed| allowed == program) {
            return Err(format!(
                "`{program}` is not a program this run may start. Declared: {}.",
                self.programs.join(", ")
            ));
        }
        self.backend
            .exec(&self.workspace, argv, remaining)
            .map_err(|error| error.to_string())
    }

    fn programs(&self) -> &[String] {
        &self.programs
    }

    fn writes(&self) -> bool {
        self.writes
    }
}
