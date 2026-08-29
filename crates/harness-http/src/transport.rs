//! One streaming `POST`, attempted under the rule that keeps a transcript honest.
//!
//! # Where this came from
//!
//! `harness-responses`'s `attempt_turn`/`send` pair, which `harness-messages` copied with the
//! header list swapped and nothing else changed. What is left here is what neither wire had to
//! change: the blocking client and its two timeouts, the cancellation check before a byte goes
//! out, the bounded read of a failed response's body, and the attempt loop. What went back to the
//! wires is what they *did* change — the URL, the headers and the decoder.

use std::io::{BufReader, Read};
use std::time::Duration;

use harness_wire::{Cancel, StreamEvent, StreamSink, WireError};
use serde_json::Value;

use crate::retry::{RetryPolicy, pause};
use crate::sse::{Framing, SseReader};
use crate::status::status_error;
use crate::witness::WitnessedSink;

/// The headers one attempt carries, in the order they are set.
///
/// Built by the wire, per attempt, and consumed here. **The values may include a credential**:
/// this is a place a secret can escape, and it is held only for as long as it takes to become a
/// header on a request that has not been sent yet.
pub type Headers = Vec<(&'static str, String)>;

/// The stream a wire wants read, once the projection has decided what to send.
///
/// The reader is [`SseReader`] over the response body; a wire's decoder is generic over
/// [`std::io::BufRead`] so the same code reads a live turn and a pinned fixture.
type Stream = SseReader<BufReader<reqwest::blocking::Response>>;

/// What a wire asks of the transport.
///
/// Compared field by field between the two wires by
/// `crates/harness-messages/tests/transport.rs`: everything here is expected to be identical
/// except [`Settings::framing`], which is a real difference between the two routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// How long a connection may take to establish.
    pub connect_timeout: Duration,
    /// Applies to each individual read, not to the turn.
    ///
    /// A streamed turn may legitimately run for minutes; what must not happen is a peer that
    /// accepts the connection and then says nothing, which would hold the loop open with no way
    /// out.
    pub read_timeout: Duration,
    /// How much of a failed response's body reaches the error message.
    pub max_error_body_bytes: usize,
    /// How many attempts a turn gets, and the pauses between them.
    pub retry: RetryPolicy,
    /// Whether `data: [DONE]` ends the stream. See [`Framing`].
    pub framing: Framing,
}

impl Settings {
    /// The settings both wires arrived at, with the one thing they disagree about named.
    ///
    /// There is no `Default`: a wire that inherited a framing would be speaking whichever route
    /// was written first.
    pub const fn streaming(framing: Framing) -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            read_timeout: Duration::from_secs(180),
            max_error_body_bytes: 2048,
            retry: RetryPolicy::DEFAULT,
            framing,
        }
    }
}

/// One request, as the transport needs it.
///
/// `headers` is a function rather than a list because it is called **once per attempt**: a
/// credential is fetched at call time and dropped when the request is built, and a wire that
/// counts its requests gives each attempt its own identifier.
pub struct StreamingPost<'a> {
    /// The wire's own name for itself, for the message a failing status produces.
    pub wire: &'a str,
    /// The absolute URL to post to.
    pub url: &'a str,
    /// The request body, sent as JSON.
    pub body: &'a Value,
    /// Builds this attempt's headers, credential included.
    pub headers: &'a dyn Fn() -> Result<Headers, WireError>,
}

/// A blocking HTTP client that reads one streamed response, and retries when that is honest.
#[derive(Debug)]
pub struct HttpTransport {
    http: reqwest::blocking::Client,
    settings: Settings,
    cancel: Cancel,
}

impl HttpTransport {
    /// Builds the client one wire will use for the life of a run.
    ///
    /// # Errors
    ///
    /// Returns [`harness_wire::WireErrorCode::Transport`] when the HTTP client cannot be
    /// constructed.
    pub fn new(settings: Settings) -> Result<Self, WireError> {
        let http = reqwest::blocking::Client::builder()
            .connect_timeout(settings.connect_timeout)
            .timeout(settings.read_timeout)
            .build()
            .map_err(|error| WireError::transport(format!("building the HTTP client: {error}")))?;
        Ok(Self {
            http,
            settings,
            cancel: Cancel::new(),
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    /// Shares this transport's cancellation token.
    ///
    /// Cancelling it drops the response body being read, so a completion that arrives afterwards
    /// cannot win. A harness whose cancel is advisory is one a person cannot actually stop.
    pub fn cancel_handle(&self) -> Cancel {
        self.cancel.clone()
    }

    /// Adopts a token shared with the rest of a turn, rather than owning a private one.
    pub fn adopt_cancel(&mut self, cancel: Cancel) {
        self.cancel = cancel;
    }

    /// One turn, retried while the far side has not actually answered.
    ///
    /// `decode` is the wire's own projection, handed the frames of one attempt and a sink that
    /// remembers whether it wrote anything. See [`WitnessedSink`] for the rule that decides what
    /// may be retried; a cancelled run stops at once, because waiting out a back-off after
    /// somebody pressed Ctrl-C is the harness ignoring them.
    ///
    /// # Errors
    ///
    /// Whatever `decode` refuses with, the typed mapping of a failing status
    /// ([`crate::status_error`]), or a transport failure — with `retriable` cleared once the
    /// attempts are spent.
    pub fn stream_turn<T, F>(
        &self,
        post: &StreamingPost<'_>,
        sink: &mut dyn StreamSink,
        mut decode: F,
    ) -> Result<T, WireError>
    where
        F: FnMut(Stream, &mut dyn StreamSink) -> Result<T, WireError>,
    {
        let policy = self.settings.retry;
        let mut attempt = 0;
        loop {
            let mut witnessed = WitnessedSink::new(&mut *sink);
            let outcome = self.send(post).and_then(|response| {
                let reader = SseReader::new(BufReader::new(response), self.settings.framing)
                    .with_cancel(self.cancel.clone());
                decode(reader, &mut witnessed)
            });
            let emitted = witnessed.emitted();
            let error = match outcome {
                Ok(outcome) => return Ok(outcome),
                Err(error) => error,
            };
            attempt += 1;
            let again = error.retriable
                && attempt < policy.max_attempts
                && !emitted
                && !self.cancel.is_cancelled();
            if !again {
                return Err(policy.exhausted(error, attempt));
            }
            // Said out loud. A run that quietly took four times as long as it looks would be a run
            // whose latency numbers mean nothing.
            sink.emit(StreamEvent::Warning {
                code: "turn-retried".to_owned(),
                message: format!(
                    "attempt {attempt} of {} failed before answering and is being retried: {}",
                    policy.max_attempts, error.message
                ),
            });
            pause(policy.backoff(attempt), &self.cancel);
        }
    }

    fn send(&self, post: &StreamingPost<'_>) -> Result<reqwest::blocking::Response, WireError> {
        if self.cancel.is_cancelled() {
            return Err(WireError::cancelled());
        }
        let mut request = self.http.post(post.url);
        // Held only for as long as it takes to become a header, and dropped before the send that
        // can block.
        for (name, value) in (post.headers)()? {
            request = request.header(name, value);
        }
        let response = request
            .json(post.body)
            .send()
            .map_err(|error| WireError::transport(format!("posting to {}: {error}", post.url)))?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        Err(status_error(
            post.wire,
            status,
            &self.read_bounded_body(response),
        ))
    }

    fn read_bounded_body(&self, response: reqwest::blocking::Response) -> String {
        let mut body = String::new();
        let _ = response
            .take(self.settings.max_error_body_bytes as u64)
            .read_to_string(&mut body);
        body.trim().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_wire::{VecSink, WireErrorCode};
    use serde_json::json;

    fn transport() -> HttpTransport {
        HttpTransport::new(Settings::streaming(Framing::DoneSentinel)).expect("the client builds")
    }

    #[test]
    fn a_cancelled_transport_never_sends_and_never_pauses() {
        // The address is one nothing answers on, so a request that escaped would fail slowly and
        // with a different code. What is under test is that the check happens first.
        let transport = transport();
        transport.cancel_handle().cancel();
        let body = json!({});
        let headers = || -> Result<Headers, WireError> { Ok(Vec::new()) };
        let post = StreamingPost {
            wire: "test-wire",
            url: "http://127.0.0.1:1/v1/turns",
            body: &body,
            headers: &headers,
        };
        let mut sink = VecSink::new();
        let started = std::time::Instant::now();
        let error = transport
            .stream_turn(&post, &mut sink, |_, _| Ok(()))
            .expect_err("a cancelled transport refuses");
        assert_eq!(error.code, WireErrorCode::Cancelled);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a cancelled run waited {:?} out",
            started.elapsed()
        );
        assert!(
            sink.events().is_empty(),
            "nothing was announced: {:?}",
            sink.events()
        );
    }

    #[test]
    fn a_header_source_that_refuses_stops_the_attempt_with_its_own_error() {
        // A credential source that cannot answer is not a transport failure and must not be
        // dressed as one: the wire's error goes up as it was written.
        let transport = transport();
        let body = json!({});
        let headers = || -> Result<Headers, WireError> {
            Err(WireError::unauthorized("the source answered with nothing"))
        };
        let post = StreamingPost {
            wire: "test-wire",
            url: "http://127.0.0.1:1/v1/turns",
            body: &body,
            headers: &headers,
        };
        let mut sink = VecSink::new();
        let error = transport
            .stream_turn(&post, &mut sink, |_, _| Ok(()))
            .expect_err("an unbuildable header list refuses");
        assert_eq!(error.code, WireErrorCode::Unauthorized);
        assert!(!error.retriable, "a rejected credential is not retried");
    }

    #[test]
    fn the_shared_settings_are_the_ones_both_wires_were_written_with() {
        let settings = Settings::streaming(Framing::PayloadsOnly);
        assert_eq!(settings.connect_timeout, Duration::from_secs(15));
        assert_eq!(settings.read_timeout, Duration::from_secs(180));
        assert_eq!(settings.max_error_body_bytes, 2048);
        assert_eq!(settings.retry, RetryPolicy::DEFAULT);
        assert_eq!(settings.framing, Framing::PayloadsOnly);
    }
}
