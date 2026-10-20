//! Adversary cases for `story:llm-port-is-a-public-crate`, written against the public surface
//! alone: everything here names `harness_llm` and `harness_wire`, as a consumer does.
//!
//! Every secret and every endpoint is synthetic and loopback (`AGENTS.md` invariant 17).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_llm::llm_credentials::{
    ResolvedSecret, Secret, SecretError, SecretRef, SecretResolver, SecretVersion,
};
use harness_llm::{BoxFuture, Presentation, Protocol, Source, Target, client, renew_codex_login};
use harness_wire::{
    Cancel, Item, Sampling, ToolChoice, TurnRequest, VecSink, WireError, WireErrorCode,
};

/// One short Messages turn.
const TURN_EVENTS: [(&str, &str); 6] = [
    (
        "message_start",
        r#"{"type":"message_start","message":{"id":"msg_b10x_001","type":"message","role":"assistant","model":"m","content":[],"stop_reason":null,"usage":{"input_tokens":3,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":1}}}"#,
    ),
    (
        "content_block_start",
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
    ),
    (
        "content_block_delta",
        r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"done"}}"#,
    ),
    (
        "content_block_stop",
        r#"{"type":"content_block_stop","index":0}"#,
    ),
    (
        "message_delta",
        r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}"#,
    ),
    ("message_stop", r#"{"type":"message_stop"}"#),
];

/// What the loopback endpoint answers one request with.
#[derive(Clone, Copy)]
enum Reply {
    /// A complete Messages turn ([`TURN_EVENTS`]).
    Turn,
    /// This status, with a body that echoes the credential the request carried.
    Status(u16),
    /// A Messages stream whose first event is an `error` that echoes the credential.
    StreamError,
}

/// A loopback endpoint that answers request `n` with `script[n]` (a full turn once the script
/// runs out) and keeps the credential header each request carried, in memory only.
struct Endpoint {
    base_url: String,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Endpoint {
    fn start(script: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let base_url = format!("http://{}/v1", listener.local_addr().expect("bound"));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&seen);
        std::thread::spawn(move || {
            for (number, stream) in listener.incoming().enumerate() {
                let Ok(stream) = stream else { return };
                let reply = script.get(number).copied().unwrap_or(Reply::Turn);
                answer(stream, &kept, reply);
            }
        });
        Self { base_url, seen }
    }

    fn target(&self, protocol: Protocol) -> Target<'_> {
        Target {
            protocol,
            base_url: &self.base_url,
            model: "m",
            context_window: 1000,
            max_output_tokens: None,
        }
    }

    fn seen(&self) -> Vec<String> {
        self.seen.lock().expect("unpoisoned").clone()
    }
}

fn answer(stream: TcpStream, seen: &Mutex<Vec<String>>, reply: Reply) {
    let mut reader = BufReader::new(stream);
    let (mut length, mut credential) = (0, String::new());
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse().expect("a length"),
                "x-api-key" | "authorization" => value.trim().clone_into(&mut credential),
                _ => {}
            }
        }
    }
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    seen.lock().expect("unpoisoned").push(credential.clone());
    let mut stream = reader.into_inner();
    let response = match reply {
        Reply::Turn => TURN_EVENTS.iter().fold(
            String::from("HTTP/1.0 200 OK\r\ncontent-type: text/event-stream\r\n\r\n"),
            |response, (name, data)| format!("{response}event: {name}\ndata: {data}\n\n"),
        ),
        Reply::Status(status) => {
            let body = format!(
                r#"{{"type":"error","error":{{"type":"authentication_error","message":"Incorrect API key provided: {credential}"}}}}"#
            );
            format!(
                "HTTP/1.0 {status} Refused\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            )
        }
        Reply::StreamError => {
            let data = format!(
                r#"{{"type":"error","error":{{"type":"authentication_error","message":"invalid x-api-key {credential}"}}}}"#
            );
            format!(
                "HTTP/1.0 200 OK\r\ncontent-type: text/event-stream\r\n\r\nevent: error\ndata: {data}\n\n"
            )
        }
    };
    // A client that hung up early must not end the listener for the requests after it.
    let _ = stream.write_all(response.as_bytes());
}

fn request() -> TurnRequest {
    TurnRequest {
        model: "m".to_owned(),
        instructions: "Answer.".to_owned(),
        items: vec![Item::UserText {
            text: "hello".to_owned(),
        }],
        tools: Vec::new(),
        max_output_tokens: None,
        sampling: Sampling::default(),
        tool_choice: ToolChoice::Auto,
    }
}

fn done() -> Vec<Item> {
    vec![Item::AssistantText {
        text: "done".to_owned(),
    }]
}

/// A consumer's resolver: answers `template` with `{n}` replaced by the resolve's ordinal, and
/// counts every resolve.
struct Counting {
    template: &'static str,
    calls: AtomicUsize,
}

impl Counting {
    fn new(template: &'static str) -> Arc<Self> {
        Arc::new(Self {
            template,
            calls: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl SecretResolver for Counting {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let value = self.template.replace("{n}", &call.to_string());
        Box::pin(async move {
            Ok(ResolvedSecret {
                secret: Secret::new(value.into_bytes())?,
                version: SecretVersion::new(format!("v{call}"))?,
            })
        })
    }
}

/// A consumer's resolver that refuses with `0`, counting every resolve.
struct Refusing(SecretError, AtomicUsize);

impl SecretResolver for Refusing {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        self.1.fetch_add(1, Ordering::SeqCst);
        let refusal = self.0;
        Box::pin(async move { Err(refusal) })
    }
}

/// A consumer's resolver whose lease service never answers, counting every resolve.
#[derive(Default)]
struct NeverAnswers(AtomicUsize);

impl SecretResolver for NeverAnswers {
    fn resolve<'a>(
        &'a self,
        _reference: &'a SecretRef,
    ) -> BoxFuture<'a, Result<ResolvedSecret, SecretError>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(std::future::pending::<Result<ResolvedSecret, SecretError>>())
    }
}

#[test]
fn a_resolver_is_asked_once_per_request_on_the_responses_wire_as_a_bearer() {
    // The story's per-request lease is not Messages-only: on Responses the same resolver, under
    // a subscription presentation, is asked once per request and its answer is the bearer.
    let endpoint = Endpoint::start(vec![Reply::Status(500); 3]);
    let lease = Counting::new("lease-{n}");
    let source = Source::resolver("tenant-lease", lease.clone(), Presentation::Subscription)
        .expect("a source");
    let mut port = client(
        &endpoint.target(Protocol::Responses),
        Some(source),
        &Cancel::new(),
    )
    .expect("a client");
    for turn in 1..=3 {
        let error = port
            .turn(&request(), &mut VecSink::new())
            .expect_err("the endpoint answers 500");
        assert_eq!(error.code, WireErrorCode::Transport, "{error:?}");
        assert_eq!(
            lease.calls(),
            turn,
            "asked once per request, through {turn}"
        );
    }
    assert_eq!(
        endpoint.seen(),
        ["Bearer lease-1", "Bearer lease-2", "Bearer lease-3"]
    );
}

#[test]
fn a_request_retried_after_a_failure_asks_again_and_once() {
    // The loop's retry is a second `turn`, so a second request and a second lease; never a
    // second resolve inside one request.
    let endpoint = Endpoint::start(vec![Reply::Status(500), Reply::Turn]);
    let lease = Counting::new("lease-{n}");
    let source =
        Source::resolver("tenant-lease", lease.clone(), Presentation::ApiKey).expect("a source");
    let mut port = client(
        &endpoint.target(Protocol::Messages),
        Some(source),
        &Cancel::new(),
    )
    .expect("a client");
    let error = port
        .turn(&request(), &mut VecSink::new())
        .expect_err("the endpoint answers 500 first");
    assert!(error.retriable, "{error:?}");
    assert_eq!(lease.calls(), 1);
    let outcome = port
        .turn(&request(), &mut VecSink::new())
        .expect("the retry completes");
    assert_eq!(outcome.items, done());
    assert_eq!(lease.calls(), 2);
    assert_eq!(endpoint.seen(), ["lease-1", "lease-2"]);
}

#[test]
fn a_forked_port_asks_the_shared_resolver_once_per_its_own_request() {
    // Delegates run on forks; a fork shares the resolver, and each request still asks once.
    let endpoint = Endpoint::start(Vec::new());
    let lease = Counting::new("lease-{n}");
    let source =
        Source::resolver("tenant-lease", lease.clone(), Presentation::ApiKey).expect("a source");
    let mut port = client(
        &endpoint.target(Protocol::Messages),
        Some(source),
        &Cancel::new(),
    )
    .expect("a client");
    {
        let mut child = port.fork().expect("an HTTP port forks");
        let outcome = child
            .turn(&request(), &mut VecSink::new())
            .expect("the child's turn completes");
        assert_eq!(outcome.items, done());
    }
    assert_eq!(lease.calls(), 1);
    port.turn(&request(), &mut VecSink::new())
        .expect("the parent's turn completes");
    assert_eq!(lease.calls(), 2);
    assert_eq!(endpoint.seen(), ["lease-1", "lease-2"]);
}

#[test]
fn a_turn_cancelled_before_it_starts_asks_the_resolver_nothing() {
    let endpoint = Endpoint::start(Vec::new());
    let lease = Counting::new("lease-{n}");
    let source =
        Source::resolver("tenant-lease", lease.clone(), Presentation::ApiKey).expect("a source");
    let cancel = Cancel::new();
    let mut port =
        client(&endpoint.target(Protocol::Messages), Some(source), &cancel).expect("a client");
    cancel.cancel();
    let error = port
        .turn(&request(), &mut VecSink::new())
        .expect_err("a cancelled turn");
    assert_eq!(error.code, WireErrorCode::Cancelled, "{error:?}");
    assert_eq!(
        lease.calls(),
        0,
        "no lease is taken for a turn that never runs"
    );
    assert!(endpoint.seen().is_empty());
}

#[test]
fn a_resolver_that_never_answers_ends_on_the_callers_cancel() {
    // A lease service that hangs must not hang the turn past the caller's cancel.
    let endpoint = Endpoint::start(Vec::new());
    let lease = Arc::new(NeverAnswers::default());
    let source =
        Source::resolver("tenant-lease", lease.clone(), Presentation::ApiKey).expect("a source");
    let cancel = Cancel::new();
    let mut port =
        client(&endpoint.target(Protocol::Messages), Some(source), &cancel).expect("a client");
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(port.turn(&request(), &mut VecSink::new()));
    });
    std::thread::sleep(Duration::from_millis(200));
    cancel.cancel();
    let result = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("the turn ends within 10 s of the cancel");
    let error = result.expect_err("a cancelled turn");
    assert_eq!(error.code, WireErrorCode::Cancelled, "{error:?}");
    assert_eq!(lease.0.load(Ordering::SeqCst), 1);
    assert!(endpoint.seen().is_empty());
}

/// A synthetic secret no output may carry.
const SENTINEL: &str = "sk-adversary-sentinel-7f3a91";

fn assert_never_named(what: &str, error: &WireError, sink: &VecSink, source: &str) {
    for (part, text) in [
        ("Debug", format!("{error:?}")),
        ("Display", error.to_string()),
        ("events", format!("{:?}", sink.events())),
        ("source", source.to_owned()),
    ] {
        assert!(
            !text.contains(SENTINEL),
            "{what}: the {part} names the secret: {text}"
        );
    }
}

#[test]
fn a_sentinel_secret_reaches_no_error_no_event_and_no_debug() {
    // Each failure is one where the far side echoes the credential back, or the credential
    // itself is refused; the secret must not come back out of the port on any of them.
    for (what, protocol, presentation, reply) in [
        (
            "Messages 401 echoing the key",
            Protocol::Messages,
            Presentation::ApiKey,
            Reply::Status(401),
        ),
        (
            "Messages stream error echoing the key",
            Protocol::Messages,
            Presentation::ApiKey,
            Reply::StreamError,
        ),
        (
            "Responses 401 echoing the bearer",
            Protocol::Responses,
            Presentation::Subscription,
            Reply::Status(401),
        ),
    ] {
        let endpoint = Endpoint::start(vec![reply]);
        let source = Source::resolver("tenant-lease", Counting::new(SENTINEL), presentation)
            .expect("a source");
        let shown = format!("{source:?}");
        let mut port =
            client(&endpoint.target(protocol), Some(source), &Cancel::new()).expect("a client");
        let mut sink = VecSink::new();
        let error = port
            .turn(&request(), &mut sink)
            .expect_err("the endpoint refuses");
        assert!(
            endpoint.seen().iter().any(|seen| seen.contains(SENTINEL)),
            "{what}: the secret was sent, so its absence below means something"
        );
        assert_never_named(what, &error, &sink, &shown);
    }

    // A credential the transport will not carry is refused before dispatch, unnamed.
    let endpoint = Endpoint::start(Vec::new());
    let source = Source::resolver(
        "tenant-lease",
        Counting::new("sk-adversary-sentinel-7f3a91 has a space"),
        Presentation::ApiKey,
    )
    .expect("a source");
    let shown = format!("{source:?}");
    let mut port = client(
        &endpoint.target(Protocol::Messages),
        Some(source),
        &Cancel::new(),
    )
    .expect("a client");
    let mut sink = VecSink::new();
    let error = port
        .turn(&request(), &mut sink)
        .expect_err("not an HTTP token");
    assert_eq!(error.code, WireErrorCode::Unauthorized, "{error:?}");
    assert!(endpoint.seen().is_empty(), "nothing was sent");
    assert_never_named("an untransportable credential", &error, &sink, &shown);

    // A resolver that refuses: the turn names the reference, which is the caller's own label.
    let refusing = Arc::new(Refusing(SecretError::Unavailable, AtomicUsize::new(0)));
    let source =
        Source::resolver("tenant-lease", refusing.clone(), Presentation::ApiKey).expect("a source");
    let mut port = client(
        &endpoint.target(Protocol::Messages),
        Some(source),
        &Cancel::new(),
    )
    .expect("a client");
    let error = port
        .turn(&request(), &mut VecSink::new())
        .expect_err("the resolver refuses");
    assert_eq!(refusing.1.load(Ordering::SeqCst), 1);
    assert!(error.message.contains("tenant-lease"), "{error:?}");
}

#[test]
fn the_port_runs_from_an_async_consumer_over_spawn_blocking() {
    // uilab's shape: the client is built on a runtime thread and its turn runs under
    // `spawn_blocking`, which needs `Send + 'static`.
    let endpoint = Endpoint::start(Vec::new());
    let target = endpoint.target(Protocol::Messages);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime");
    let outcome = runtime.block_on(async {
        let mut port = client(
            &target,
            Some(Source::key("synthetic-key").expect("a source")),
            &Cancel::new(),
        )
        .expect("a client built inside a runtime");
        tokio::task::spawn_blocking(move || port.turn(&request(), &mut VecSink::new()))
            .await
            .expect("the blocking task ends")
    });
    assert_eq!(outcome.expect("the turn completes").items, done());
}

#[test]
fn renew_codex_login_from_an_async_consumer_returns_its_answer_rather_than_panicking() {
    // `renew_codex_login` is public now and documents only an `Err` for its refusals (`# Errors`,
    // no `# Panics`). The port beside it runs from inside a runtime (the case above); this one
    // builds a runtime of its own and `block_on`s it, which tokio refuses with a panic when the
    // calling thread is already driving one.
    let login = Path::new("/nonexistent/adversary/codex-login.json");
    let renew = || {
        renew_codex_login(
            login,
            "http://127.0.0.1:1/oauth/token",
            "adversary-client",
            Duration::from_mins(15),
        )
    };
    let outside = renew();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime");
    let inside = runtime.block_on(async { std::panic::catch_unwind(renew) });
    assert_eq!(
        inside.map_err(|_| "panicked"),
        Ok(outside),
        "called from a runtime thread, renewal answers as it does outside one"
    );
}
