//! Security review cases for `story:api-key-from-the-secrets-store`, written against the public
//! surface alone (`harness_llm::Source::secret_in`, `harness_llm::client`), as an embedder does.
//!
//! Every store is `keyring_core`'s in-memory mock, laid out as the `secrets` keychain backend lays
//! one out; never the platform's keychain, Secret Service or D-Bus. Every value and endpoint is
//! synthetic and loopback (`AGENTS.md` invariant 17).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use harness_llm::llm_credentials::secrets::authorize::Authorized;
use harness_llm::llm_credentials::secrets::keychain::{DEFAULT_SERVICE, KeychainBackend};
use harness_llm::llm_credentials::secrets::storage::{
    Address, ScopeName, SecretStorage, SecretValue, Target as At,
};
use harness_llm::{Protocol, Source, Target, client};
use harness_wire::{
    Cancel, Item, Sampling, ToolChoice, TurnRequest, VecSink, WireError, WireErrorCode,
};
use keyring_core::api::CredentialStoreApi as _;

/// A synthetic key long enough that any quoted fragment of it is caught.
const STORED: &str = "synthetic-review-key-9d41c07e";
/// The same entry after it was rewritten.
const ROTATED: &str = "synthetic-review-rotated-3a6f52b1";

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

/// A fresh in-memory keychain no other case sees.
#[derive(Clone)]
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

    /// Writes `value` at tenant `default`, `namespace`, `user`, `name`, as the backend writes.
    fn put_as(&self, namespace: &str, user: &str, name: &str, value: &[u8]) {
        let address =
            Address::parse(ScopeName::DEFAULT, namespace, user, name).expect("a secrets address");
        let backend = self.backend();
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a runtime")
            .block_on(backend.write(
                &At::unbound(address),
                SecretValue::new(value.to_vec()).expect("a value"),
            ))
            .expect("the fixture store takes the write");
    }

    fn put(&self, namespace: &str, name: &str, value: &str) {
        self.put_as(namespace, ScopeName::DEFAULT, name, value.as_bytes());
    }

    /// An entry at the exact keychain place the backend reads, holding bytes that are not the
    /// backend's layout (no format byte, no version): what another keyring client would leave.
    fn put_raw(&self, namespace: &str, name: &str, raw: &[u8]) {
        let user = format!("default/{namespace}/default/{name}");
        self.store
            .build(DEFAULT_SERVICE, &user, None)
            .expect("an entry")
            .set_secret(raw)
            .expect("the raw write");
    }

    fn storage(&self) -> Arc<dyn SecretStorage> {
        Arc::new(Authorized::local(self.backend()))
    }
}

#[derive(Clone, Copy)]
enum Reply {
    Turn,
    /// A 401 whose body echoes the credential the request carried.
    Unauthorized,
}

/// A loopback Messages endpoint: request `n` gets `script[n]` (a turn once it runs out), and
/// `before_reply(n)` runs before the reply is written. Keeps each request's `x-api-key`.
struct Endpoint {
    base_url: String,
    keys: Arc<Mutex<Vec<String>>>,
}

impl Endpoint {
    fn start(script: Vec<Reply>, before_reply: impl Fn(usize) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let base_url = format!("http://{}/v1", listener.local_addr().expect("bound"));
        let keys = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&keys);
        std::thread::spawn(move || {
            for (number, stream) in listener.incoming().enumerate() {
                let Ok(stream) = stream else { return };
                let reply = script.get(number).copied().unwrap_or(Reply::Turn);
                answer(stream, &seen, reply, || before_reply(number));
            }
        });
        Self { base_url, keys }
    }

    fn target(&self) -> Target<'_> {
        Target {
            protocol: Protocol::Messages,
            base_url: &self.base_url,
            model: "m",
            context_window: 1000,
            max_output_tokens: None,
        }
    }

    fn keys(&self) -> Vec<String> {
        self.keys.lock().expect("unpoisoned").clone()
    }
}

fn answer(stream: TcpStream, seen: &Mutex<Vec<String>>, reply: Reply, before: impl FnOnce()) {
    let mut reader = BufReader::new(stream);
    let (mut length, mut key) = (0, String::new());
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
                "x-api-key" => value.trim().clone_into(&mut key),
                _ => {}
            }
        }
    }
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    seen.lock().expect("unpoisoned").push(key.clone());
    before();
    let response = match reply {
        Reply::Turn => TURN_EVENTS.iter().fold(
            String::from("HTTP/1.0 200 OK\r\ncontent-type: text/event-stream\r\n\r\n"),
            |response, (name, data)| format!("{response}event: {name}\ndata: {data}\n\n"),
        ),
        Reply::Unauthorized => {
            let body = format!(
                r#"{{"type":"error","error":{{"type":"authentication_error","message":"invalid x-api-key {key}"}}}}"#
            );
            format!(
                "HTTP/1.0 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            )
        }
    };
    let mut stream = reader.into_inner();
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

fn rendered(error: &WireError, sink: &VecSink) -> String {
    format!("{error} {error:?} {:?}", sink.events())
}

/// A refusal of the far side that echoes the stored key names the reference and never the value,
/// and the store path asks nothing more than once: a 401 is not a licence to re-read and resend.
#[test]
fn a_401_echoing_the_stored_key_is_refused_once_without_the_value() {
    let keychain = Keychain::new();
    keychain.put("llm", "fixture-api-key", STORED);
    let endpoint = Endpoint::start(vec![Reply::Unauthorized], |_| {});
    let source = Source::secret_in("llm/fixture-api-key", keychain.storage()).expect("a source");
    let mut port = client(&endpoint.target(), Some(source), &Cancel::new()).expect("a client");
    let mut sink = VecSink::new();
    let error = port
        .turn(&request(), &mut sink)
        .expect_err("the endpoint refuses");
    assert_eq!(
        endpoint.keys(),
        [STORED],
        "the stored key was sent exactly once"
    );
    let shown = rendered(&error, &sink);
    assert!(!shown.contains(STORED), "never the value: {shown}");
}

/// A key rewritten in the store while a turn is in flight is the one the **next** request
/// carries, and the client keeps no copy of the earlier one (the 401 above, then a turn).
#[test]
fn a_key_rewritten_after_a_401_is_the_one_the_next_turn_sends() {
    let keychain = Keychain::new();
    keychain.put("llm", "fixture-api-key", STORED);
    let rotating = keychain.clone();
    let endpoint = Endpoint::start(vec![Reply::Unauthorized], move |number| {
        if number == 0 {
            rotating.put("llm", "fixture-api-key", ROTATED);
        }
    });
    let source = Source::secret_in("llm/fixture-api-key", keychain.storage()).expect("a source");
    let mut port = client(&endpoint.target(), Some(source), &Cancel::new()).expect("a client");
    port.turn(&request(), &mut VecSink::new())
        .expect_err("the first request is refused");
    port.turn(&request(), &mut VecSink::new())
        .expect("the second turn completes with the rewritten key");
    assert_eq!(endpoint.keys(), [STORED, ROTATED]);
}

/// A stored value the transport will not carry (a space, an inner line break, a non-ASCII byte)
/// is refused before dispatch, and the refusal quotes none of it.
#[test]
fn a_stored_value_that_is_not_an_http_token_is_refused_unsent_and_unquoted() {
    for (what, value) in [
        ("a space", &b"synthetic-review-key has-a-space"[..]),
        (
            "an inner line break",
            &b"synthetic-review-key\nsecond-line"[..],
        ),
        ("a non-ASCII byte", &b"synthetic-review-key-\xc3\xa9"[..]),
        ("nothing", &b""[..]),
    ] {
        let keychain = Keychain::new();
        keychain.put_as("llm", ScopeName::DEFAULT, "fixture-api-key", value);
        let endpoint = Endpoint::start(Vec::new(), |_| {});
        let source =
            Source::secret_in("llm/fixture-api-key", keychain.storage()).expect("a source");
        let mut port = client(&endpoint.target(), Some(source), &Cancel::new()).expect("a client");
        let mut sink = VecSink::new();
        let error = port
            .turn(&request(), &mut sink)
            .expect_err("not an HTTP token");
        assert_eq!(error.code, WireErrorCode::Unauthorized, "{what}: {error:?}");
        assert!(endpoint.keys().is_empty(), "{what}: nothing was sent");
        let shown = rendered(&error, &sink);
        assert!(
            !shown.contains("synthetic-review-key"),
            "{what}: never the value: {shown}"
        );
    }
}

/// An entry at the backend's exact keychain place that is not in the backend's layout (written by
/// another keyring client) is never sent verbatim: the store path refuses it.
#[test]
fn an_entry_not_in_the_secrets_layout_is_refused_and_never_sent() {
    let keychain = Keychain::new();
    keychain.put_raw("llm", "fixture-api-key", STORED.as_bytes());
    let endpoint = Endpoint::start(Vec::new(), |_| {});
    let source = Source::secret_in("llm/fixture-api-key", keychain.storage()).expect("a source");
    let mut port = client(&endpoint.target(), Some(source), &Cancel::new()).expect("a client");
    let mut sink = VecSink::new();
    let error = port
        .turn(&request(), &mut sink)
        .expect_err("a foreign entry is not a secret");
    assert!(endpoint.keys().is_empty(), "nothing was sent: {error:?}");
    assert!(
        error.message.contains("llm/fixture-api-key"),
        "named by reference: {error:?}"
    );
    let shown = rendered(&error, &sink);
    assert!(!shown.contains(STORED), "never the value: {shown}");
}

/// The reference selects exactly one entry: tenant and user `default`, the namespace its first
/// segment, the name the rest. Neighbours in another user, another namespace, a prefix or an
/// extension of the name never answer for it.
#[test]
fn a_reference_reads_only_the_entry_it_names_and_never_a_neighbour() {
    let keychain = Keychain::new();
    keychain.put_as("llm", "alice", "team/fixture-api-key", STORED.as_bytes());
    keychain.put("team", "fixture-api-key", STORED);
    keychain.put("llm", "team", STORED);
    keychain.put("llm", "team/fixture-api-key/extra", STORED);
    keychain.put("llm", "team/fixture-api-key-2", STORED);
    let endpoint = Endpoint::start(Vec::new(), |_| {});
    let source =
        Source::secret_in("llm/team/fixture-api-key", keychain.storage()).expect("a nested name");
    let mut port = client(&endpoint.target(), Some(source), &Cancel::new()).expect("a client");
    let error = port
        .turn(&request(), &mut VecSink::new())
        .expect_err("no neighbour answers");
    assert_eq!(error.code, WireErrorCode::Unauthorized, "{error:?}");
    assert!(endpoint.keys().is_empty());

    keychain.put("llm", "team/fixture-api-key", ROTATED);
    port.turn(&request(), &mut VecSink::new())
        .expect("the named entry answers");
    assert_eq!(endpoint.keys(), [ROTATED]);
}

/// A malformed reference is refused at construction through the public constructor too, by the
/// reference as typed, before any storage is asked (the storage here would answer anything).
#[test]
fn a_malformed_reference_is_refused_by_the_public_constructor_before_the_store_is_read() {
    let keychain = Keychain::new();
    keychain.put("llm", "fixture-api-key", STORED);
    for malformed in [
        "",
        "/",
        "llm",
        "llm/",
        "/fixture-api-key",
        "llm/fixture-api-key/",
        "llm/fixture-api-key\n",
        " llm/fixture-api-key",
        "llm/.",
        "default/../llm/fixture-api-key",
    ] {
        match Source::secret_in(malformed, keychain.storage()) {
            Err(error) => {
                assert!(
                    error.contains(&format!("`{}`", malformed.escape_debug())),
                    "{malformed:?}: {error}"
                );
                assert!(!error.contains(STORED), "{error}");
            }
            Ok(source) => {
                // Admitted: it must still never read the entry another reference names.
                let endpoint = Endpoint::start(Vec::new(), |_| {});
                let mut port =
                    client(&endpoint.target(), Some(source), &Cancel::new()).expect("a client");
                let _ = port.turn(&request(), &mut VecSink::new());
                assert!(
                    endpoint.keys().iter().all(|key| key != STORED),
                    "{malformed:?} read the entry `llm/fixture-api-key` names"
                );
            }
        }
    }
}
