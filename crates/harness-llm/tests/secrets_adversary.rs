//! Adversary cases for `story:api-key-from-the-secrets-store`, written against the public surface
//! alone: `Source::secret_in`, `client` and the storage port `harness_llm::llm_credentials`
//! re-exports, as an embedder holds them.
//!
//! Every store is `keyring_core`'s in-memory mock, laid out as `secretsctl put <name> --namespace
//! <namespace>` lays one out, and never the platform's keychain, Secret Service or D-Bus. Every
//! value and every endpoint is synthetic and loopback (`AGENTS.md` invariant 17).

use std::future::Future;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use harness_llm::llm_credentials::secrets::authorize::Authorized;
use harness_llm::llm_credentials::secrets::keychain::KeychainBackend;
use harness_llm::llm_credentials::secrets::storage::{
    Address, Capability, Revealed, ScopeName, SecretStorage, SecretValue, StorageError, Target,
};
use harness_llm::{Protocol, Source, client};
use harness_wire::{Cancel, Item, Sampling, ToolChoice, TurnRequest, VecSink, WireErrorCode};

/// A synthetic key, long enough that a refusal quoting any of it is caught.
const STORED: &str = "synthetic-adversary-stored-3b8e1f";
/// What the same entry holds after it is rewritten between two requests.
const ROTATED: &str = "synthetic-adversary-rotated-9d02c4";

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

    fn target(&self, protocol: Protocol) -> harness_llm::Target<'_> {
        harness_llm::Target {
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
    };
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

/// A fresh in-memory keychain no other case sees, written as `secretsctl put` writes one.
struct Keychain {
    store: Arc<keyring_core::mock::Store>,
}

impl Keychain {
    fn new() -> Self {
        Self {
            store: keyring_core::mock::Store::new().expect("an in-memory store"),
        }
    }

    fn backend(&self) -> KeychainBackend {
        KeychainBackend::new(self.store.clone() as Arc<keyring_core::CredentialStore>)
    }

    /// Writes `value` at `tenant/namespace/user/name` straight through the backend, unauthorized.
    fn put_at(&self, tenant: &str, namespace: &str, user: &str, name: &str, value: &[u8]) {
        let address = Address::parse(tenant, namespace, user, name).expect("a secrets address");
        let backend = self.backend();
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a runtime")
            .block_on(backend.write(
                &Target::unbound(address),
                SecretValue::new(value.to_vec()).expect("a value"),
            ))
            .expect("the fixture store takes the write");
    }

    /// Writes `value` at `namespace`/`name` in the local scope, as `secretsctl put` does.
    fn put(&self, namespace: &str, name: &str, value: &[u8]) {
        self.put_at(
            ScopeName::DEFAULT,
            namespace,
            ScopeName::DEFAULT,
            name,
            value,
        );
    }

    /// What `SecretsResolver::keychain` builds over the native store, over this mock instead.
    fn storage(&self) -> Arc<dyn SecretStorage> {
        Arc::new(Authorized::local(self.backend()))
    }
}

type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A storage that counts every read and answers each with `refusal`, or forwards to `inner`.
struct Probe {
    inner: Option<Arc<dyn SecretStorage>>,
    refusal: StorageError,
    reads: AtomicUsize,
}

impl Probe {
    fn refusing(refusal: StorageError) -> Arc<Self> {
        Arc::new(Self {
            inner: None,
            refusal,
            reads: AtomicUsize::new(0),
        })
    }
}

impl SecretStorage for Probe {
    fn capabilities(&self) -> &[Capability] {
        &[Capability::Read]
    }

    fn read<'life0, 'life1, 'async_trait>(
        &'life0 self,
        target: &'life1 Target,
    ) -> Boxed<'async_trait, Result<Revealed, StorageError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            match &self.inner {
                Some(inner) => inner.read(target).await,
                None => Err(self.refusal),
            }
        })
    }
}

/// References that are not `<namespace>/<name>` under the secrets grammar, each with why.
fn malformed() -> Vec<(String, &'static str)> {
    vec![
        (
            "llm/team/".to_owned(),
            "a trailing `/` leaves an empty last segment",
        ),
        ("llm/..".to_owned(), "`..` as the name"),
        ("llm/a/../b".to_owned(), "`..` inside the name"),
        ("llm/./b".to_owned(), "`.` inside the name"),
        ("../llm/key".to_owned(), "`..` as the namespace"),
        ("./key".to_owned(), "`.` as the namespace"),
        ("llm/schlüssel".to_owned(), "a non-ASCII name"),
        ("ллм/key".to_owned(), "a non-ASCII namespace"),
        ("llm/\u{202e}yek".to_owned(), "a bidi override"),
        ("llm/key\u{7}".to_owned(), "a C0 control"),
        ("llm/key\u{1b}[2J".to_owned(), "a terminal escape"),
        ("llm/key\n".to_owned(), "a trailing newline"),
        ("llm/key\u{85}".to_owned(), "a C1 control"),
        ("llm/key ".to_owned(), "a trailing space"),
        (" llm/key".to_owned(), "a leading space"),
        ("llm\\key".to_owned(), "a backslash, not a slash"),
        ("llm/ke y".to_owned(), "a space inside the name"),
        ("llm/key%2f".to_owned(), "a percent escape"),
        (format!("{}/key", "n".repeat(65)), "a 65-byte namespace"),
        (format!("llm/{}", "k".repeat(65)), "a 65-byte name segment"),
        (
            format!("llm/{}/{}", "a".repeat(64), "b".repeat(64)),
            "a 129-byte name",
        ),
        ("llm".to_owned(), "no `/`"),
        ("/".to_owned(), "only a `/`"),
    ]
}

#[test]
fn every_reference_the_secrets_grammar_refuses_is_refused_by_name_and_reads_nothing() {
    let probe = Probe::refusing(StorageError::NotFound);
    for (reference, why) in malformed() {
        let error = Source::secret_in(&reference, probe.clone()).expect_err(&format!(
            "{why} ({reference:?}) is not `<namespace>/<name>`"
        ));
        assert!(
            error.contains(&format!("`{}`", reference.escape_debug())),
            "{why}: the refusal names the reference as typed: {error}"
        );
    }
    assert_eq!(
        probe.reads.load(Ordering::SeqCst),
        0,
        "a refused reference reads nothing"
    );
}

#[test]
fn a_refused_reference_reaches_the_terminal_without_a_raw_control_character() {
    // The refusal is printed to the operator's stderr. A reference that carries a terminal
    // escape or a bidi override is echoed byte for byte today, so the line that says *what was
    // refused* can be rewritten by what was refused. `harness-cli/src/approve.rs` already
    // neutralises controls in an argv it shows a person; this refusal does not.
    let probe = Probe::refusing(StorageError::NotFound);
    for (reference, why) in malformed() {
        let error = Source::secret_in(&reference, probe.clone()).expect_err("refused");
        let raw: Vec<char> = error
            .chars()
            .filter(|c| {
                c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
            .collect();
        assert!(
            raw.is_empty(),
            "{why}: the refusal carries {raw:?} raw: {error:?}"
        );
    }
}

#[test]
fn the_longest_reference_the_grammar_admits_is_accepted_and_its_value_sent() {
    // 64-byte namespace, 128-byte name: the bounds of `ScopeName` and `SecretName`. The typed
    // `SecretRef` the source carries must not be narrower than the grammar it is checked against.
    let namespace = "n".repeat(64);
    let name = format!("{}/{}", "a".repeat(64), "b".repeat(63));
    assert_eq!(name.len(), 128);
    let keychain = Keychain::new();
    keychain.put(&namespace, &name, STORED.as_bytes());
    let endpoint = Endpoint::start(Vec::new());
    let source = Source::secret_in(&format!("{namespace}/{name}"), keychain.storage())
        .expect("the longest well-formed reference");
    let mut port = client(
        &endpoint.target(Protocol::Messages),
        Some(source),
        &Cancel::new(),
    )
    .expect("a client");
    port.turn(&request(), &mut VecSink::new())
        .expect("the turn completes");
    assert_eq!(endpoint.seen(), [STORED]);
}

#[test]
fn a_401_is_not_resent_and_the_next_turn_reads_the_store_again() {
    let keychain = Keychain::new();
    keychain.put("llm", "fixture-api-key", STORED.as_bytes());
    let endpoint = Endpoint::start(vec![Reply::Status(401)]);
    let source = Source::secret_in("llm/fixture-api-key", keychain.storage()).expect("a source");
    let mut port = client(
        &endpoint.target(Protocol::Messages),
        Some(source),
        &Cancel::new(),
    )
    .expect("a client");

    let mut sink = VecSink::new();
    let error = port
        .turn(&request(), &mut sink)
        .expect_err("the endpoint refuses the key");
    assert_eq!(error.code, WireErrorCode::Unauthorized, "{error:?}");
    let rendered = format!("{error} {error:?} {sink:?}");
    assert!(
        !rendered.contains(STORED),
        "the echoed key never comes back: {rendered}"
    );
    assert_eq!(
        endpoint.seen(),
        [STORED],
        "one request: a refused key is neither refreshed into a loop nor resent"
    );

    keychain.put("llm", "fixture-api-key", ROTATED.as_bytes());
    port.turn(&request(), &mut VecSink::new())
        .expect("the rotated key is accepted");
    assert_eq!(
        endpoint.seen(),
        [STORED, ROTATED],
        "the turn after a 401 reads the store again and sends what it holds now, not the stale key"
    );
}

#[test]
fn a_store_that_refuses_is_named_by_the_reference_and_never_by_a_value() {
    for refusal in [
        StorageError::Unavailable,
        StorageError::Denied,
        StorageError::Unsupported,
        StorageError::Conflict,
        StorageError::TooLarge,
    ] {
        let probe = Probe::refusing(refusal);
        let endpoint = Endpoint::start(Vec::new());
        let source = Source::secret_in("llm/fixture-api-key", probe.clone()).expect("a source");
        let mut port = client(
            &endpoint.target(Protocol::Messages),
            Some(source),
            &Cancel::new(),
        )
        .expect("a client");
        let error = port
            .turn(&request(), &mut VecSink::new())
            .expect_err("a refusing store refuses the request");
        assert!(
            error.message.contains("llm/fixture-api-key"),
            "{refusal:?}: the refusal names the reference: {error:?}"
        );
        assert_eq!(
            probe.reads.load(Ordering::SeqCst),
            1,
            "{refusal:?}: one read"
        );
        assert_eq!(
            endpoint.seen(),
            Vec::<String>::new(),
            "{refusal:?}: nothing sent"
        );
    }
}

#[test]
fn a_held_value_the_transport_will_not_carry_is_refused_without_quoting_it() {
    let keychain = Keychain::new();
    let spaced = "synthetic adversary key with spaces 77e1";
    keychain.put("llm", "fixture-api-key", spaced.as_bytes());
    let endpoint = Endpoint::start(Vec::new());
    let source = Source::secret_in("llm/fixture-api-key", keychain.storage()).expect("a source");
    let shown = format!("{source:?}");
    let mut port = client(
        &endpoint.target(Protocol::Messages),
        Some(source),
        &Cancel::new(),
    )
    .expect("a client");
    let error = port
        .turn(&request(), &mut VecSink::new())
        .expect_err("a value with spaces is not a header value");
    let rendered = format!("{error} {error:?} {shown}");
    assert!(!rendered.contains("adversary key"), "{rendered}");
    assert_eq!(endpoint.seen(), Vec::<String>::new(), "nothing sent");
}

#[test]
fn a_value_stored_with_one_line_terminator_is_sent_without_it() {
    // `secretsctl put --raw` keeps whatever the pipe carried; `--api-key-file` trims. The store
    // must not send a key the file source would have sent differently.
    let keychain = Keychain::new();
    keychain.put("llm", "fixture-api-key", format!("{STORED}\n").as_bytes());
    let endpoint = Endpoint::start(Vec::new());
    let source = Source::secret_in("llm/fixture-api-key", keychain.storage()).expect("a source");
    let mut port = client(
        &endpoint.target(Protocol::Messages),
        Some(source),
        &Cancel::new(),
    )
    .expect("a client");
    port.turn(&request(), &mut VecSink::new())
        .expect("the turn completes");
    assert_eq!(endpoint.seen(), [STORED]);
}

#[test]
fn on_the_responses_wire_the_stored_key_is_the_bearer() {
    let keychain = Keychain::new();
    keychain.put("llm", "fixture-api-key", STORED.as_bytes());
    let endpoint = Endpoint::start(Vec::new());
    let source = Source::secret_in("llm/fixture-api-key", keychain.storage()).expect("a source");
    let mut port = client(
        &endpoint.target(Protocol::Responses),
        Some(source),
        &Cancel::new(),
    )
    .expect("a client");
    // The endpoint speaks Messages; only the header the request carried is asserted.
    let _ = port.turn(&request(), &mut VecSink::new());
    assert_eq!(endpoint.seen(), [format!("Bearer {STORED}")]);
}

#[test]
fn a_secret_held_for_another_tenant_or_user_never_answers_for_the_local_scope() {
    let keychain = Keychain::new();
    keychain.put_at(
        "other",
        "llm",
        ScopeName::DEFAULT,
        "fixture-api-key",
        STORED.as_bytes(),
    );
    keychain.put_at(
        ScopeName::DEFAULT,
        "llm",
        "other",
        "fixture-api-key",
        STORED.as_bytes(),
    );
    // And the same name nested one level down, and a longer name it is a prefix of.
    keychain.put("llm", "team/fixture-api-key", STORED.as_bytes());
    keychain.put("llm", "fixture-api-key-2", STORED.as_bytes());
    keychain.put("llm-2", "fixture-api-key", STORED.as_bytes());
    let endpoint = Endpoint::start(Vec::new());
    let source = Source::secret_in("llm/fixture-api-key", keychain.storage()).expect("a source");
    let mut port = client(
        &endpoint.target(Protocol::Messages),
        Some(source),
        &Cancel::new(),
    )
    .expect("a client");
    let error = port
        .turn(&request(), &mut VecSink::new())
        .expect_err("nothing is held at default/llm/default/fixture-api-key");
    assert_eq!(error.code, WireErrorCode::Unauthorized, "{error:?}");
    assert!(!format!("{error} {error:?}").contains(STORED));
    assert_eq!(endpoint.seen(), Vec::<String>::new(), "nothing sent");
}
