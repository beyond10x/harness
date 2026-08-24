//! The five operations a confined workspace needs, from wherever they come.
//!
//! # Two backends, one toolset
//!
//! [`Client`](crate::Client) reaches a daemon over an owner-permissioned Unix socket.
//! [`Embedded`](crate::Embedded) holds substrate's own driver in this process. The tools cannot
//! tell them apart, and that is the point: which one a run uses is a deployment decision, not a
//! different set of things the model may do.
//!
//! What genuinely differs is stated where it lives — the socket carries an authenticated subject
//! derived from kernel peer credentials, and an embedded driver has no such boundary because there
//! is no peer. See [`Embedded`](crate::Embedded).

use serde_json::Value;

use crate::{Facts, SubstrateError};

/// Where a confined workspace's operations actually happen.
pub trait Backend {
    /// What this machine can confine.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError`] when the answer cannot be obtained or read.
    fn machine(&self) -> Result<Facts, SubstrateError>;

    /// Open a confined workspace and answer its id.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError`] when the workspace cannot be opened.
    fn workspace_create(&self, lease_ttl_ms: u64) -> Result<String, SubstrateError>;

    /// Write one file, whole.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError`] when the write is refused. A path that leaves the workspace is
    /// refused by substrate and never by this crate: re-implementing containment here would make
    /// two answers to one question, and the wrong one would be the one nobody was looking at.
    fn file_write(&self, workspace: &str, path: &str, text: &str) -> Result<Value, SubstrateError>;

    /// Read one file back as text.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError`] when the file cannot be read or is not text.
    fn file_read(&self, workspace: &str, path: &str) -> Result<String, SubstrateError>;

    /// Run one argv and answer what it did.
    ///
    /// **An argv, never a command line.** Substrate's own `exec.start` predicate is
    /// `exec.argv-only`; nothing here builds a string a shell would then take apart.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError`] when the run could not be started. A program that exits non-zero
    /// is **not** an error: it is a result, and the caller needs to see it.
    fn exec(&self, workspace: &str, argv: &[String]) -> Result<Value, SubstrateError>;
}
