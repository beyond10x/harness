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

impl Operations for ConfinedOperations {
    fn file_read(&self, path: &str, _max_bytes: Option<u64>) -> Result<Value, String> {
        // The daemon's own read ceiling governs; a caller's smaller one is not forwarded, because a
        // truncation this side could not be reported as one and a partial read that looked whole is
        // the failure the read tool's own reply exists to prevent.
        self.backend
            .file_read(&self.workspace, path)
            .map(
                |text| json!({"path": path, "bytes": text.len(), "truncated": false, "text": text}),
            )
            .map_err(|error| error.to_string())
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
        let program = &argv[0];
        if !self.programs.iter().any(|allowed| allowed == program) {
            return Err(format!(
                "`{program}` is not a program this run may start. Declared: {}.",
                self.programs.join(", ")
            ));
        }
        self.backend
            .exec(&self.workspace, argv)
            .map_err(|error| error.to_string())
    }

    fn programs(&self) -> &[String] {
        &self.programs
    }

    fn writes(&self) -> bool {
        self.writes
    }
}
