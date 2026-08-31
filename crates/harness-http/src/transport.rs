//! One streaming `POST`, attempted under the rule that keeps a transcript honest.
//!
//! # Where this came from
//!
//! the first wire's `attempt_turn`/`send` pair, which the second copied with the
//! header list swapped and nothing else changed. What is left here is what neither wire had to
//! change: the blocking client and its two timeouts, the cancellation check before a byte goes
//! out, the bounded read of a failed response's body, and the attempt loop. What went back to the
//! wires is what they *did* change — the URL, the headers and the decoder.

use std::io::{BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

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

/// Response headers exposed to the wire's server-delay decoder.
///
/// Names and values are owned because the asynchronous response is dropped independently of the
/// synchronous decoder. The transport does not interpret or retain them.
pub type ResponseHeaders = Vec<(String, String)>;

/// Decodes a server-requested retry delay from response headers against a supplied clock.
pub type ServerDelayDecoder = dyn Fn(&[(String, String)], SystemTime) -> Option<Duration>;

/// The stream a wire wants read, once the projection has decided what to send.
///
/// The reader is [`SseReader`] over the response body; a wire's decoder is generic over
/// [`std::io::BufRead`] so the same code reads a live turn and a pinned fixture.
type Stream = SseReader<BufReader<ResponseBody>>;

/// What a wire asks of the transport.
///
/// Compared field by field between the two wires by
/// the cross-wire transport test: everything here is expected to be identical
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
    /// Largest server-requested retry delay the transport will honour.
    pub max_server_delay: Duration,
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
            max_server_delay: Duration::from_secs(30),
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
    /// Decodes a standard server-requested delay from response headers.
    ///
    /// Header spelling belongs to the wire. The transport supplies its clock and applies
    /// [`Settings::max_server_delay`] before sleeping.
    pub server_delay: &'a ServerDelayDecoder,
}

/// A blocking HTTP client that reads one streamed response, and retries when that is honest.
#[derive(Debug)]
pub struct HttpTransport {
    http: reqwest::Client,
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
        let http = reqwest::Client::builder()
            .connect_timeout(settings.connect_timeout)
            .read_timeout(settings.read_timeout)
            .redirect(reqwest::redirect::Policy::none())
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
                decode(reader, &mut witnessed).map_err(AttemptFailure::without_delay)
            });
            let emitted = witnessed.emitted();
            let failure = match outcome {
                Ok(outcome) => return Ok(outcome),
                Err(error) => error,
            };
            attempt += 1;
            let again = failure.error.retriable
                && attempt < policy.max_attempts
                && !emitted
                && !self.cancel.is_cancelled();
            if !again {
                return Err(policy.exhausted(failure.error, attempt));
            }
            let local_delay = policy.backoff(attempt);
            let delay = combined_delay(
                local_delay,
                failure.server_delay,
                self.settings.max_server_delay,
            );
            // Said out loud. A run that quietly took four times as long as it looks would be a run
            // whose latency numbers mean nothing.
            sink.emit(StreamEvent::Warning {
                code: "turn-retried".to_owned(),
                message: format!(
                    "attempt {attempt} of {} failed before answering and is being retried after \
                     {} ms: {}",
                    policy.max_attempts,
                    delay.as_millis(),
                    failure.error.message
                ),
            });
            pause(delay, &self.cancel);
        }
    }

    fn send(&self, post: &StreamingPost<'_>) -> Result<ResponseBody, AttemptFailure> {
        if self.cancel.is_cancelled() {
            return Err(AttemptFailure::without_delay(WireError::cancelled()));
        }
        let headers = (post.headers)().map_err(AttemptFailure::without_delay)?;
        let encoded = encode_json_body(post.body).map_err(AttemptFailure::without_delay)?;
        let (head, body) = ResponseBody::spawn(
            self.http.clone(),
            post.url.to_owned(),
            encoded,
            headers,
            self.cancel.clone(),
        )
        .map_err(AttemptFailure::without_delay)?;
        if head.status.is_success() {
            return Ok(body);
        }
        let server_delay = (post.server_delay)(&head.headers, SystemTime::now());
        let response_body = self
            .read_bounded_body(body)
            .map_err(AttemptFailure::without_delay)?;
        Err(AttemptFailure {
            error: status_error(post.wire, head.status, &response_body),
            server_delay,
        })
    }

    fn read_bounded_body(&self, response: ResponseBody) -> Result<String, WireError> {
        let mut body = Vec::new();
        response
            .take(self.settings.max_error_body_bytes as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|error| {
                if self.cancel.is_cancelled() {
                    WireError::cancelled()
                } else {
                    WireError::transport(format!("reading a failed response: {error}"))
                }
            })?;
        if body.len() > self.settings.max_error_body_bytes {
            return Err(WireError::too_large(format!(
                "a failed response body passed the {} byte bound",
                self.settings.max_error_body_bytes
            )));
        }
        Ok(String::from_utf8_lossy(&body).trim().to_owned())
    }
}

#[derive(Debug)]
struct AttemptFailure {
    error: WireError,
    server_delay: Option<Duration>,
}

impl AttemptFailure {
    fn without_delay(error: WireError) -> Self {
        Self {
            error,
            server_delay: None,
        }
    }
}

#[derive(Debug)]
struct ResponseHead {
    status: reqwest::StatusCode,
    headers: ResponseHeaders,
}

#[derive(Debug)]
enum WorkerMessage {
    Head(Result<ResponseHead, WireError>),
    Chunk(Vec<u8>),
    Error(String),
    End,
}

/// A blocking reader over a cancellable asynchronous response body.
#[derive(Debug)]
pub struct ResponseBody {
    receiver: Receiver<WorkerMessage>,
    buffered: std::io::Cursor<Vec<u8>>,
    cancel: Cancel,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl ResponseBody {
    fn spawn(
        client: reqwest::Client,
        url: String,
        body: Vec<u8>,
        headers: Headers,
        cancel: Cancel,
    ) -> Result<(ResponseHead, Self), WireError> {
        let (sender, receiver) = mpsc::sync_channel(8);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_cancel = cancel.clone();
        let worker = std::thread::Builder::new()
            .name("harness-http-response".to_owned())
            .spawn(move || {
                response_worker(
                    client,
                    url,
                    body,
                    headers,
                    worker_cancel,
                    worker_stop,
                    sender,
                );
            })
            .map_err(|error| WireError::transport(format!("starting the HTTP worker: {error}")))?;
        let response = Self {
            receiver,
            buffered: std::io::Cursor::new(Vec::new()),
            cancel,
            stop,
            worker: Some(worker),
        };
        let head = loop {
            if response.cancel.is_cancelled() {
                return Err(WireError::cancelled());
            }
            match response.receiver.recv_timeout(WORKER_POLL) {
                Ok(WorkerMessage::Head(head)) => break head?,
                Ok(WorkerMessage::Error(error)) => return Err(WireError::transport(error)),
                Ok(WorkerMessage::End) => {
                    return Err(WireError::transport(
                        "the HTTP worker ended before response headers",
                    ));
                }
                Ok(WorkerMessage::Chunk(_)) => {
                    return Err(WireError::transport(
                        "the HTTP worker sent body bytes before response headers",
                    ));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    if response.cancel.is_cancelled() {
                        return Err(WireError::cancelled());
                    }
                    return Err(WireError::transport(
                        "the HTTP worker stopped before response headers",
                    ));
                }
            }
        };
        Ok((head, response))
    }

    fn stop_worker(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Read for ResponseBody {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        loop {
            let read = self.buffered.read(output)?;
            if read != 0 {
                return Ok(read);
            }
            if self.cancel.is_cancelled() {
                // `BufRead::read_line` retries `Interrupted` internally, which would turn a
                // cancelled stream into a live-lock. `Other` crosses that boundary once; the SSE
                // layer sees the shared token and restores the typed cancellation outcome.
                return Err(std::io::Error::other("the caller cancelled"));
            }
            match self.receiver.recv_timeout(WORKER_POLL) {
                Ok(WorkerMessage::Chunk(bytes)) => {
                    self.buffered = std::io::Cursor::new(bytes);
                }
                Ok(WorkerMessage::Error(error)) => {
                    return Err(std::io::Error::other(error));
                }
                Ok(WorkerMessage::End) => return Ok(0),
                Ok(WorkerMessage::Head(_)) => {
                    return Err(std::io::Error::other(
                        "the HTTP worker sent response headers twice",
                    ));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    if self.cancel.is_cancelled() {
                        return Err(std::io::Error::other("the caller cancelled"));
                    }
                    return Ok(0);
                }
            }
        }
    }
}

impl Drop for ResponseBody {
    fn drop(&mut self) {
        self.stop_worker();
    }
}

const WORKER_POLL: Duration = Duration::from_millis(20);

fn combined_delay(
    local: Duration,
    server: Option<Duration>,
    maximum_server_delay: Duration,
) -> Duration {
    server.map_or(local, |requested| {
        requested.min(maximum_server_delay).max(local)
    })
}

fn response_worker(
    client: reqwest::Client,
    url: String,
    body: Vec<u8>,
    headers: Headers,
    cancel: Cancel,
    stop: Arc<AtomicBool>,
    sender: SyncSender<WorkerMessage>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = sender.send(WorkerMessage::Head(Err(WireError::transport(format!(
                "building the HTTP runtime: {error}"
            )))));
            return;
        }
    };
    runtime.block_on(async move {
        let mut request = client.post(&url).body(body);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let mut response = tokio::select! {
            result = request.send() => match result {
                Ok(response) => response,
                Err(error) => {
                    let _ = sender.send(WorkerMessage::Head(Err(WireError::transport(
                        format!("posting to {url}: {error}")
                    ))));
                    return;
                }
            },
            () = wait_until_stopped(&cancel, &stop) => {
                if cancel.is_cancelled() {
                    let _ = sender.send(WorkerMessage::Head(Err(WireError::cancelled())));
                }
                return;
            }
        };
        let head = ResponseHead {
            status: response.status(),
            headers: response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (name.as_str().to_owned(), value.to_owned()))
                })
                .collect(),
        };
        if !send_message(&sender, WorkerMessage::Head(Ok(head)), &cancel, &stop).await {
            return;
        }
        loop {
            let chunk = tokio::select! {
                result = response.chunk() => result,
                () = wait_until_stopped(&cancel, &stop) => return,
            };
            match chunk {
                Ok(Some(bytes)) => {
                    if !send_message(
                        &sender,
                        WorkerMessage::Chunk(bytes.to_vec()),
                        &cancel,
                        &stop,
                    )
                    .await
                    {
                        return;
                    }
                }
                Ok(None) => {
                    let _ = send_message(&sender, WorkerMessage::End, &cancel, &stop).await;
                    return;
                }
                Err(error) => {
                    let _ = send_message(
                        &sender,
                        WorkerMessage::Error(format!("reading the HTTP response: {error}")),
                        &cancel,
                        &stop,
                    )
                    .await;
                    return;
                }
            }
        }
    });
}

/// Encodes the JSON bytes a streaming request sends.
///
/// Kept as the production encoder's public seam so a wire contract compares the exact body bytes
/// rather than a parsed value that would hide ordering, whitespace, or escaping changes.
///
/// # Errors
///
/// Returns a protocol error when the supplied JSON value cannot be serialized.
pub fn encode_json_body(body: &Value) -> Result<Vec<u8>, WireError> {
    let mut encoded = serde_json::to_vec(body)
        .map_err(|error| WireError::protocol(format!("encoding a request body: {error}")))?;
    encoded.push(b'\n');
    Ok(encoded)
}

async fn wait_until_stopped(cancel: &Cancel, stop: &AtomicBool) {
    while !cancel.is_cancelled() && !stop.load(Ordering::SeqCst) {
        tokio::time::sleep(WORKER_POLL).await;
    }
}

async fn send_message(
    sender: &SyncSender<WorkerMessage>,
    mut message: WorkerMessage,
    cancel: &Cancel,
    stop: &AtomicBool,
) -> bool {
    loop {
        if cancel.is_cancelled() || stop.load(Ordering::SeqCst) {
            return false;
        }
        match sender.try_send(message) {
            Ok(()) => return true,
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(returned)) => {
                message = returned;
                tokio::time::sleep(WORKER_POLL).await;
            }
        }
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

    fn silent_server(
        response_headers: bool,
    ) -> (
        String,
        std::sync::mpsc::Receiver<()>,
        std::thread::JoinHandle<bool>,
    ) {
        use std::io::{Read as _, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let url = format!("http://{}/stream", listener.local_addr().expect("addr"));
        let (ready_sender, ready_receiver) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("a request");
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .expect("read timeout");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("request bytes");
            if response_headers {
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
                )
                .expect("response headers");
                stream.flush().expect("headers flushed");
            }
            ready_sender.send(()).expect("announce ready");
            loop {
                match stream.read(&mut request) {
                    Ok(0) => return true,
                    Ok(_) => {}
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        return false;
                    }
                    Err(_) => return true,
                }
            }
        });
        (url, ready_receiver, server)
    }

    #[test]
    fn a_cancelled_transport_never_sends_and_never_pauses() {
        // The address is one nothing answers on, so a request that escaped would fail slowly and
        // with a different code. What is under test is that the check happens first.
        let transport = transport();
        transport.cancel_handle().cancel();
        let body = json!({});
        let headers = || -> Result<Headers, WireError> { Ok(Vec::new()) };
        let server_delay = |_: &[(String, String)], _: SystemTime| None;
        let post = StreamingPost {
            wire: "test-wire",
            url: "http://127.0.0.1:1/stream",
            body: &body,
            headers: &headers,
            server_delay: &server_delay,
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
        let server_delay = |_: &[(String, String)], _: SystemTime| None;
        let post = StreamingPost {
            wire: "test-wire",
            url: "http://127.0.0.1:1/stream",
            body: &body,
            headers: &headers,
            server_delay: &server_delay,
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
        assert_eq!(settings.max_server_delay, Duration::from_secs(30));
        assert_eq!(settings.framing, Framing::PayloadsOnly);
    }

    #[test]
    fn server_delay_never_shortens_local_backoff_and_is_bounded() {
        let local = Duration::from_secs(2);
        assert_eq!(combined_delay(local, None, Duration::from_secs(30)), local);
        assert_eq!(
            combined_delay(local, Some(Duration::from_secs(1)), Duration::from_secs(30)),
            local,
            "the server cannot ask for less than local policy"
        );
        assert_eq!(
            combined_delay(local, Some(Duration::from_secs(9)), Duration::from_secs(30)),
            Duration::from_secs(9)
        );
        assert_eq!(
            combined_delay(
                local,
                Some(Duration::from_secs(90)),
                Duration::from_secs(30)
            ),
            Duration::from_secs(30),
            "a peer cannot hold a run beyond the declared cap"
        );
    }

    #[test]
    fn cancellation_aborts_a_request_waiting_for_response_headers() {
        let (url, ready, server) = silent_server(false);
        let transport = transport();
        let cancel = transport.cancel_handle();
        let canceller = std::thread::spawn(move || {
            ready.recv().expect("request arrived");
            cancel.cancel();
        });
        let body = json!({});
        let headers = || -> Result<Headers, WireError> { Ok(Vec::new()) };
        let server_delay = |_: &[(String, String)], _: SystemTime| None;
        let post = StreamingPost {
            wire: "test-wire",
            url: &url,
            body: &body,
            headers: &headers,
            server_delay: &server_delay,
        };
        let mut sink = VecSink::new();
        let started = std::time::Instant::now();
        let error = transport
            .stream_turn(&post, &mut sink, |_, _| Ok(()))
            .expect_err("cancelled while waiting for headers");
        canceller.join().expect("canceller finished");
        assert_eq!(error.code, WireErrorCode::Cancelled, "{error}");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            server.join().expect("server finished"),
            "the socket stayed open"
        );
    }

    #[test]
    fn cancellation_aborts_a_silent_event_stream() {
        let (url, ready, server) = silent_server(true);
        let transport = transport();
        let cancel = transport.cancel_handle();
        let canceller = std::thread::spawn(move || {
            ready.recv().expect("response began");
            cancel.cancel();
        });
        let body = json!({});
        let headers = || -> Result<Headers, WireError> { Ok(Vec::new()) };
        let server_delay = |_: &[(String, String)], _: SystemTime| None;
        let post = StreamingPost {
            wire: "test-wire",
            url: &url,
            body: &body,
            headers: &headers,
            server_delay: &server_delay,
        };
        let mut sink = VecSink::new();
        let started = std::time::Instant::now();
        let error = transport
            .stream_turn(&post, &mut sink, |mut reader, _| {
                reader.next_event().map(|_| ())
            })
            .expect_err("cancelled while waiting for an event");
        canceller.join().expect("canceller finished");
        assert_eq!(error.code, WireErrorCode::Cancelled, "{error}");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            server.join().expect("server finished"),
            "the socket stayed open"
        );
    }

    #[test]
    fn a_redirect_never_forwards_wire_built_headers_or_the_body() {
        use std::io::{Read as _, Write as _};

        let target = std::net::TcpListener::bind("127.0.0.1:0").expect("target port");
        target.set_nonblocking(true).expect("nonblocking target");
        let target_url = format!(
            "http://{}/elsewhere",
            target.local_addr().expect("target addr")
        );
        let source = std::net::TcpListener::bind("127.0.0.1:0").expect("source port");
        let source_url = format!(
            "http://{}/stream",
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

        let transport = transport();
        let body = json!({"opaque": "synthetic-body-secret"});
        let headers = || -> Result<Headers, WireError> {
            Ok(vec![("x-private", "synthetic-header-secret".to_owned())])
        };
        let server_delay = |_: &[(String, String)], _: SystemTime| None;
        let post = StreamingPost {
            wire: "test-wire",
            url: &source_url,
            body: &body,
            headers: &headers,
            server_delay: &server_delay,
        };
        let mut sink = VecSink::new();
        let error = transport
            .stream_turn(&post, &mut sink, |_, _| Ok(()))
            .expect_err("redirects refuse");
        server.join().expect("source finished");
        assert_eq!(error.code, WireErrorCode::Refused, "{error}");
        assert!(
            !error.message.contains("synthetic-header-secret"),
            "{error}"
        );
        assert!(!error.message.contains("synthetic-body-secret"), "{error}");
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            matches!(target.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "a redirect reached the second origin"
        );
    }
}
