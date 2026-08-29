#![forbid(unsafe_code)]

//! The transport half of a provider wire: one streaming `POST`, and everything beneath the
//! projection.
//!
//! # Why this is a crate rather than a module inside a wire
//!
//! `harness-responses` was written first, and `harness-messages` copied five things out of it
//! **unchanged**: bounded server-sent-event framing, the retry rule, the back-off, the witnessed
//! sink that makes the retry rule safe, and the HTTP status mapping. The copy compiled and passed
//! the second wire's suite without a line altered, which is the evidence that none of it is
//! vendor-shaped. It is *transport*-shaped, and the first wire could not tell the difference while
//! it was the only one (`ROADMAP.md`, phase 3).
//!
//! So the rule this crate is held to: **no vendor name, no vendor field name, no endpoint path and
//! no header name appears anywhere in it.** A wire hands it a URL, a body, a header list the wire
//! built itself and a decoder; it hands back frames and typed refusals. Anything in here that had
//! to name one of the two routes would be a sign the extraction was drawn in the wrong place.
//!
//! One thing came close, and it is a parameter rather than a constant: [`Framing`]. The two
//! streams genuinely differ about whether `data: [DONE]` ends them, so both wires say which they
//! speak — neither inherits a default that happens to suit the other.
//!
//! It holds no credential. The header list arrives already built, and is rebuilt for **every**
//! attempt, so a credential is still fetched at call time by the wire that owns its presentation
//! and is dropped when the request is built.
//!
//! It knows nothing about turns, items, tools or usage: those are the projection's, and the
//! projection stays in the wire that names the fields.

mod exchange;
mod retry;
mod sse;
mod status;
mod transport;
mod witness;

pub use exchange::{JsonExchange, JsonPost, MAX_EXCHANGE_BODY_BYTES};
pub use retry::RetryPolicy;
pub use sse::{Framing, MAX_EVENT_BYTES, MAX_STREAM_BYTES, SseEvent, SseReader};
pub use status::status_error;
pub use transport::{Headers, HttpTransport, Settings, StreamingPost};
pub use witness::WitnessedSink;
