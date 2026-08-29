//! One plain `POST` that is not a turn: a JSON body out, a JSON document back.
//!
//! # Why this is beside the streaming transport rather than inside it
//!
//! [`crate::HttpTransport`] exists for a *turn*: it frames server-sent events, it retries while
//! the far side has not answered, and it needs a [`crate::Framing`] to know what ends a stream.
//! A credential exchange has none of that shape — one small JSON body, one small JSON answer, no
//! stream to frame — and giving it a framing it does not use would be a parameter that means
//! nothing at the only call site that supplies it.
//!
//! # It does not retry, and that is the point
//!
//! An authorization server that rotates a refresh token has already spent the old one by the time
//! it answers. A second attempt with the same body therefore cannot succeed, and a first attempt
//! whose *answer* was lost has left the caller holding a credential the server no longer honours —
//! so a blind retry turns one recoverable failure into two. One attempt, and the failure is
//! reported with the status the far side gave.
//!
//! Like everything else here it names no vendor: the caller supplies the URL, the body and a word
//! for who is being spoken to.

use std::io::Read as _;
use std::time::Duration;

use harness_wire::{WireError, WireErrorCode};
use serde_json::Value;

use crate::status::status_error;

/// How much of an answer is read before it is refused as implausible for this shape of request.
///
/// A token response is a few hundred bytes. A megabyte of it is a proxy's error page, a captive
/// portal or something else that is not the authorization server, and reading it all into memory
/// to find that out helps nobody.
pub const MAX_EXCHANGE_BODY_BYTES: usize = 64 * 1024;

/// One request/response exchange, as its caller needs it.
pub struct JsonPost<'a> {
    /// Who is being spoken to, for the message a failing status produces.
    ///
    /// A name a person recognises — never a credential, and never a value read out of one.
    pub who: &'a str,
    /// The absolute URL to post to.
    pub url: &'a str,
    /// The request body, sent as JSON.
    ///
    /// **This may carry a secret** — a refresh token is one. It is held for as long as it takes to
    /// become a request and is never logged, never echoed into an error, and never retried.
    pub body: &'a Value,
}

/// A blocking client for a single request and its answer.
#[derive(Debug)]
pub struct JsonExchange {
    http: reqwest::blocking::Client,
}

impl JsonExchange {
    /// Builds the client one exchange will use.
    ///
    /// # Errors
    ///
    /// Returns [`harness_wire::WireErrorCode::Transport`] when the HTTP client cannot be built.
    pub fn new() -> Result<Self, WireError> {
        let http = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            // Shorter than a turn's, deliberately: this request produces a few hundred bytes and
            // an authorization server that has not answered in thirty seconds is not going to.
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| WireError::transport(format!("building the HTTP client: {error}")))?;
        Ok(Self { http })
    }

    /// Posts once and reads the answer as JSON.
    ///
    /// # Errors
    ///
    /// The typed mapping of a failing status ([`crate::status_error`]), a transport failure, or a
    /// refusal when the answer is not JSON — with `retriable` cleared in every case, because this
    /// request is not one anything may send twice.
    pub fn post(&self, post: &JsonPost<'_>) -> Result<Value, WireError> {
        let response = self
            .http
            .post(post.url)
            .json(post.body)
            .send()
            .map_err(|error| {
                // The URL and not the body: the body is where the secret is.
                WireError::transport(format!("posting to {}: {error}", post.url))
            })
            .map_err(not_retriable)?;
        let status = response.status();
        let mut body = String::new();
        let read = response
            .take(MAX_EXCHANGE_BODY_BYTES as u64)
            .read_to_string(&mut body);
        if !status.is_success() {
            return Err(not_retriable(status_error(post.who, status, body.trim())));
        }
        read.map_err(|error| {
            WireError::transport(format!("reading {}'s answer: {error}", post.who))
        })
        .map_err(not_retriable)?;
        serde_json::from_str(&body).map_err(|error| {
            // The body is *not* quoted here. A successful token response is entirely secret.
            WireError::new(
                WireErrorCode::Refused,
                format!(
                    "{} answered {status} with {} byte(s) that are not JSON: {error}",
                    post.who,
                    body.len()
                ),
                false,
            )
        })
    }
}

/// Clears `retriable`, whatever the mapping said.
///
/// The status table is written for a turn, where a 503 means *send it again*. Here it does not:
/// see the module's own note on rotation. The code and the message survive; only the invitation to
/// try again is withdrawn.
fn not_retriable(mut error: WireError) -> WireError {
    error.retriable = false;
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_transport_failure_is_never_offered_for_retry() {
        // Nothing answers on port 1. What is under test is the `retriable` flag, not the failure:
        // a retried refresh presents a token the server may already have rotated away.
        let exchange = JsonExchange::new().expect("the client builds");
        let body = json!({});
        let error = exchange
            .post(&JsonPost {
                who: "the authorization server",
                url: "http://127.0.0.1:1/oauth/token",
                body: &body,
            })
            .expect_err("nothing is listening");
        assert_eq!(error.code, WireErrorCode::Transport);
        assert!(
            !error.retriable,
            "a credential exchange is not a turn: {error}"
        );
    }

    #[test]
    fn the_url_is_named_and_the_body_is_not() {
        // The body of this request is a refresh token. An error message carrying it would put a
        // live credential in a log, a terminal and a session file at once.
        let exchange = JsonExchange::new().expect("the client builds");
        let body = json!({"refresh_token": "synthetic-not-a-real-token"});
        let error = exchange
            .post(&JsonPost {
                who: "the authorization server",
                url: "http://127.0.0.1:1/oauth/token",
                body: &body,
            })
            .expect_err("nothing is listening");
        assert!(error.message.contains("127.0.0.1:1"), "{error}");
        assert!(
            !error.message.contains("synthetic-not-a-real-token"),
            "the request body reached an error message: {error}"
        );
    }
}
