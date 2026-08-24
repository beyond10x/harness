//! The six things a tool can be, performed by somebody else.
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

use serde_json::Value;

/// What performs a catalogue entry.
///
/// Every method answers `Result<Value, String>`: the `Err` is a sentence the model reads, because a
/// tool that failed has to say so in words the next turn can act on. A typed error here would have
/// to be rendered into one anyway, and the two would drift.
pub trait Operations {
    /// Read one text file.
    ///
    /// # Errors
    ///
    /// The path could not be read, is not a file, or is outside the workspace.
    fn file_read(&self, path: &str, max_bytes: Option<u64>) -> Result<Value, String>;

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

    /// Find a literal substring in the tree's text files.
    ///
    /// # Errors
    ///
    /// The pattern is empty, or the path is outside the workspace.
    fn search(
        &self,
        pattern: &str,
        path: &str,
        max_results: Option<usize>,
    ) -> Result<Value, String>;

    /// Run one program and answer what it did.
    ///
    /// **An argv, never a command line.** Nothing here builds a string a shell would take apart,
    /// and an implementation that has no way to confine a process answers
    /// [`Unavailable`](Self::unavailable) rather than shelling out unconfined.
    ///
    /// # Errors
    ///
    /// The program is not one this run may start, or it could not be launched.
    fn run(&self, argv: &[String]) -> Result<Value, String>;

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
    fn file_read(&self, path: &str, max_bytes: Option<u64>) -> Result<Value, String> {
        self.reads.file_read(path, max_bytes)
    }

    fn dir_list(&self, path: &str) -> Result<Value, String> {
        self.reads.dir_list(path)
    }

    fn search(
        &self,
        pattern: &str,
        path: &str,
        max_results: Option<usize>,
    ) -> Result<Value, String> {
        self.reads.search(pattern, path, max_results)
    }

    fn file_write(&self, path: &str, text: &str) -> Result<Value, String> {
        self.effects.file_write(path, text)
    }

    fn file_edit(&self, path: &str, old: &str, new: &str) -> Result<Value, String> {
        self.effects.file_edit(path, old, new)
    }

    fn run(&self, argv: &[String]) -> Result<Value, String> {
        self.effects.run(argv)
    }

    fn programs(&self) -> &[String] {
        self.effects.programs()
    }

    fn writes(&self) -> bool {
        self.effects.writes()
    }
}
