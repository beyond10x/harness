//! One plain `POST` that is not a turn: a JSON body out, a JSON document back.
//!
//! # Why this is beside the streaming transport rather than inside it
//!
//! [`crate::HttpTransport`] exists for a *turn*: it frames server-sent events, it retries while
//! the far side has not answered, and it needs a [`crate::Framing`] to know what ends a stream.
//! A bounded document exchange has none of that shape — one small JSON body, one small JSON
//! answer, no stream to frame — and giving it a framing it does not use would be a parameter that
//! means nothing at the call sites that supply it.
//!
//! # It does not retry, and that is the point
//!
//! A caller may be sending a non-idempotent document. The transport cannot prove that repeating it
//! is safe, so it makes one attempt and reports the status the far side gave.
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
/// Documents using this exchange are expected to be small. Reading an unbounded proxy error page
/// or unrelated response into memory to discover that it is the wrong shape helps nobody.
pub const MAX_EXCHANGE_BODY_BYTES: usize = 64 * 1024;

/// Whether a failed response's body may be quoted in the resulting error.
///
/// The transport cannot know whether a response contains sensitive material, so the caller makes
/// the disclosure decision explicitly on every exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureBody {
    /// Omit the response body and report only the peer and status.
    Omit,
    /// Include the body after enforcing [`MAX_EXCHANGE_BODY_BYTES`].
    IncludeBounded,
}

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
    /// The transport treats it as opaque: it is held only long enough to build the request and is
    /// never logged, echoed into an error, or retried.
    pub body: &'a Value,
    /// Whether a failing response body is safe to disclose.
    pub failure_body: FailureBody,
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
            // Shorter than a streamed turn's, deliberately: this request produces one bounded
            // document rather than a long-lived stream.
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
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
        if !status.is_success() && post.failure_body == FailureBody::Omit {
            return Err(not_retriable(status_error(post.who, status, "")));
        }
        let mut body = Vec::new();
        let read = response
            .take(MAX_EXCHANGE_BODY_BYTES as u64 + 1)
            .read_to_end(&mut body);
        if body.len() > MAX_EXCHANGE_BODY_BYTES {
            return Err(not_retriable(WireError::too_large(format!(
                "{}'s answer passed the {MAX_EXCHANGE_BODY_BYTES} byte bound",
                post.who
            ))));
        }
        if !status.is_success() {
            let body = String::from_utf8_lossy(&body);
            return Err(not_retriable(status_error(post.who, status, body.trim())));
        }
        read.map_err(|error| {
            WireError::transport(format!("reading {}'s answer: {error}", post.who))
        })
        .map_err(not_retriable)?;
        serde_json::from_slice(&body).map_err(|error| {
            // The body is not quoted here. Its sensitivity is the caller's to know, not ours.
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

    fn exchange_body(body: Vec<u8>) -> Result<Value, WireError> {
        use std::io::{Read as _, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!("http://{}/document", listener.local_addr().expect("addr"));
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("a request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("request bytes");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .expect("headers");
            stream.write_all(&body).expect("body");
        });
        let exchange = JsonExchange::new().expect("the client builds");
        let request = json!({});
        let result = exchange.post(&JsonPost {
            who: "the document endpoint",
            url: &url,
            body: &request,
            failure_body: FailureBody::IncludeBounded,
        });
        server.join().expect("server finished");
        result
    }

    #[test]
    fn a_transport_failure_is_never_offered_for_retry() {
        // Nothing answers on port 1. What is under test is the `retriable` flag, not the failure:
        // the generic exchange cannot prove that repeating an opaque document is safe.
        let exchange = JsonExchange::new().expect("the client builds");
        let body = json!({});
        let error = exchange
            .post(&JsonPost {
                who: "the document endpoint",
                url: "http://127.0.0.1:1/document",
                body: &body,
                failure_body: FailureBody::IncludeBounded,
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
        // Request bodies are opaque to this crate and never enter its errors.
        let exchange = JsonExchange::new().expect("the client builds");
        let body = json!({"opaque": "synthetic-private-value"});
        let error = exchange
            .post(&JsonPost {
                who: "the document endpoint",
                url: "http://127.0.0.1:1/document",
                body: &body,
                failure_body: FailureBody::IncludeBounded,
            })
            .expect_err("nothing is listening");
        assert!(error.message.contains("127.0.0.1:1"), "{error}");
        assert!(
            !error.message.contains("synthetic-private-value"),
            "the request body reached an error message: {error}"
        );
    }

    #[test]
    fn a_redirect_is_a_response_and_never_a_second_request() {
        use std::io::{Read as _, Write as _};

        let target = std::net::TcpListener::bind("127.0.0.1:0").expect("target port");
        target.set_nonblocking(true).expect("nonblocking target");
        let target_url = format!(
            "http://{}/elsewhere",
            target.local_addr().expect("target addr")
        );
        let source = std::net::TcpListener::bind("127.0.0.1:0").expect("source port");
        let source_url = format!(
            "http://{}/document",
            source.local_addr().expect("source addr")
        );
        let server = std::thread::spawn(move || {
            let (mut stream, _) = source.accept().expect("source request");
            let mut bytes = [0_u8; 4096];
            let _ = stream.read(&mut bytes).expect("request bytes");
            write!(
                stream,
                "HTTP/1.1 307 Temporary Redirect\r\nLocation: {target_url}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .expect("redirect");
        });

        let exchange = JsonExchange::new().expect("the client builds");
        let body = json!({"opaque": "must-not-cross-origins"});
        let error = exchange
            .post(&JsonPost {
                who: "the document endpoint",
                url: &source_url,
                body: &body,
                failure_body: FailureBody::Omit,
            })
            .expect_err("redirects are refused");
        server.join().expect("source finished");
        assert_eq!(error.code, WireErrorCode::Refused, "{error}");
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            matches!(target.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "a redirect reached the second origin"
        );
    }

    #[test]
    fn an_answer_one_byte_over_the_limit_is_refused() {
        let error = exchange_body(vec![b'x'; MAX_EXCHANGE_BODY_BYTES + 1])
            .expect_err("limit plus one refuses");
        assert_eq!(error.code, WireErrorCode::TooLarge, "{error}");
        assert!(error.message.contains("65536"), "{error}");
    }

    #[test]
    fn valid_json_at_both_edges_and_with_multibyte_text_is_accepted() {
        for total in [MAX_EXCHANGE_BODY_BYTES - 1, MAX_EXCHANGE_BODY_BYTES] {
            let body = format!("\"{}\"", "x".repeat(total - 2)).into_bytes();
            assert_eq!(body.len(), total);
            let value = exchange_body(body).expect("the byte bound is inclusive");
            assert_eq!(value.as_str().expect("string").len(), total - 2);
        }
        let body = format!(
            "\"{}é\"",
            "x".repeat(MAX_EXCHANGE_BODY_BYTES - 2 - 'é'.len_utf8())
        )
        .into_bytes();
        assert_eq!(body.len(), MAX_EXCHANGE_BODY_BYTES);
        let value = exchange_body(body).expect("whole multibyte JSON at the bound");
        let text = value.as_str().expect("string");
        assert!(text.ends_with('é'));
        assert!(!text.contains('\u{fffd}'));
    }
}
