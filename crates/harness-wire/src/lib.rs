#![forbid(unsafe_code)]

//! Neutral values and ports shared by the b10x agent loop and its model wires.
//!
//! This crate performs no I/O, reads no clock, holds no credential, and names no vendor field. A
//! wire adapter projects one documented model API into these values; the loop consumes only these
//! values. That is the whole reason a second wire costs a projection instead of a second loop.

mod bearer;
mod bound;
mod cancel;
mod envelope;
mod id;
mod item;
mod port;
mod turn;

pub use bearer::{Bearer, BearerSource, CredentialKind, StaticBearer};
pub use bound::{
    MAX_INSTRUCTION_BYTES, MAX_TOOL_ARGUMENT_BYTES, MAX_TOOL_DESCRIPTION_BYTES,
    MAX_TOOL_RESULT_BYTES, MAX_TOOLS, encoded_len, exceeds,
};
pub use cancel::Cancel;
pub use envelope::{AccessKind, Effect, Envelope, Idempotency, Risk, Subject};
pub use id::{CallId, InvalidId, ToolName, WireId};
pub use item::{Item, ToolCall, ToolOutcome};
pub use port::{ModelPort, StreamEvent, StreamSink, ToolPort, VecSink};
pub use turn::{Approval, Sampling, StopReason, ToolSpec, TurnOutcome, TurnRequest, Usage};

/// Why a model wire refused, in terms the loop can decide on without knowing the vendor.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum WireErrorCode {
    /// The request never reached a model: connect, TLS, or read failure.
    Transport,
    /// A response arrived but did not match the pinned wire subset.
    Protocol,
    /// The credential was missing, rejected, or lacked authority.
    Unauthorized,
    /// The provider asked for less traffic.
    RateLimited,
    /// The provider refused this request on its own terms.
    Refused,
    /// A bound this crate declares was exceeded.
    TooLarge,
    /// The request needs something this wire does not implement.
    Unsupported,
    /// The caller cancelled. No later completion may win.
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[error("{code:?}: {message}")]
#[serde(deny_unknown_fields)]
pub struct WireError {
    pub code: WireErrorCode,
    pub message: String,
    pub retriable: bool,
}

impl WireError {
    pub fn new(code: WireErrorCode, message: impl Into<String>, retriable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retriable,
        }
    }

    pub fn transport(message: impl Into<String>) -> Self {
        Self::new(WireErrorCode::Transport, message, true)
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        Self::new(WireErrorCode::Protocol, message, false)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(WireErrorCode::Unauthorized, message, false)
    }

    pub fn too_large(message: impl Into<String>) -> Self {
        Self::new(WireErrorCode::TooLarge, message, false)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(WireErrorCode::Unsupported, message, false)
    }

    pub fn cancelled() -> Self {
        Self::new(WireErrorCode::Cancelled, "the caller cancelled", false)
    }
}
