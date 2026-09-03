//! A caller-owned source of context and tool inventory refreshed before every model turn.
//!
//! The loop owns neither a workspace nor a service client. Hosted callers fetch their current
//! actor view outside this crate and hand the resulting, already-authorized snapshot through this
//! port. The loop only enforces that a snapshot can narrow the attached [`ToolPort`], never widen
//! it, and records the revisions that actually shaped a turn.

use harness_wire::ToolSpec;

use crate::ContextPackage;

/// The turn about to be assembled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnEnvironmentRequest {
    /// One-based turn number of the request that will consume the snapshot.
    pub turn: u64,
    /// Declared model context window, when the embedder supplied one.
    pub context_window: Option<u64>,
}

/// Renderer-neutral inputs that may change while a run is alive.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnEnvironment {
    /// Turn-scoped context appended after the run's standing instructions.
    pub context: ContextPackage,
    /// Exact current subset of the attached port's published specifications.
    pub tools: Vec<ToolSpec>,
    /// Opaque durable revision of the actor context used for this snapshot.
    pub context_revision: String,
    /// Opaque durable revision of the actor's intent inventory.
    pub inventory_revision: String,
}

/// Why a current environment could not safely shape a turn.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnvironmentError {
    #[error("the turn environment is unavailable: {0}")]
    Unavailable(String),
    #[error("the turn environment is invalid: {0}")]
    Invalid(String),
}

/// Fetches the current actor context and exact tool inventory.
///
/// Implementations may perform service I/O. A failure aborts the run before the model request;
/// stale context is never silently replayed as current context.
pub trait TurnEnvironmentProvider {
    /// # Errors
    ///
    /// Returns [`EnvironmentError`] when the authoritative view cannot be obtained or normalized.
    /// The loop fails closed and sends no model request for that turn.
    fn refresh(
        &mut self,
        request: TurnEnvironmentRequest,
    ) -> Result<TurnEnvironment, EnvironmentError>;
}
