#![forbid(unsafe_code)]

//! Credential sources that read exactly what a caller pointed them at, and nothing else.
//!
//! # Why this is its own crate and not part of a wire
//!
//! `harness-wire` defines the credential *types* and performs no I/O (AGENTS.md invariant 3), so a
//! source that reads a file cannot live there. The obvious second home — beside the wire that
//! needed one first — is wrong for a different reason: **nothing here is vendor-shaped.** Reading a
//! token out of a named file or a named environment variable, optionally at a named JSON pointer,
//! is the same operation for every subscription route there is, and the two this harness cares
//! about hang off *different* wires. Putting it in one of them would make the other depend on it to
//! reuse it, which is a coupling between two wire adapters that have nothing to say to each other.
//!
//! What is vendor-shaped is how the fetched credential is *presented* — which header carries it,
//! and what else has to travel with it. That stays in the wire crate, keyed off
//! [`harness_wire::CredentialKind`], which is what a source here declares.

mod oauth;
mod renewal;

pub use oauth::{NamedSource, SubscriptionToken};
pub use renewal::{AuthDocument, Renewed, TokenEndpoint, expiry_of, is_stale, renew_if_stale};
