//! HTTP/1.1 over an owner-permissioned Unix socket, by hand.
//!
//! The refusal to take an HTTP crate is argued in the crate root. What is here is the smallest
//! thing that speaks four routes: one request per connection, `Content-Length` bodies, and no
//! streaming, redirects, compression or TLS — none of which a Unix socket to a local daemon
//! serving bounded JSON has any use for.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;

use crate::{Backend, Facts, SubstrateError, base64};

/// How long to wait on a local daemon that is not answering.
///
/// A local socket either answers promptly or is wedged; a caller blocked on one has no way to tell
/// which, and a probe at startup must not be able to hang a launch.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Where the bytes go. Swapped in tests for a socket the test itself serves.
pub trait Transport {
    /// Sends one request and answers the status and body.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError::Unreachable`] when the socket cannot be reached or read.
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, String), SubstrateError>;
}

/// The real transport: a Unix socket the operator's daemon listens on.
#[derive(Debug, Clone)]
pub struct UnixTransport {
    socket: PathBuf,
}

impl UnixTransport {
    pub fn new(socket: impl AsRef<Path>) -> Self {
        Self {
            socket: socket.as_ref().to_path_buf(),
        }
    }
}

impl Transport for UnixTransport {
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<(u16, String), SubstrateError> {
        let unreachable = |source: std::io::Error| SubstrateError::Unreachable {
            path: self.socket.display().to_string(),
            source,
        };

        let mut stream = UnixStream::connect(&self.socket).map_err(unreachable)?;
        stream
            .set_read_timeout(Some(TIMEOUT))
            .map_err(unreachable)?;
        stream
            .set_write_timeout(Some(TIMEOUT))
            .map_err(unreachable)?;

        let payload = body.map(ToString::to_string).unwrap_or_default();
        // `Host` is required by HTTP/1.1 and means nothing over a Unix socket; `localhost` is the
        // conventional placeholder. `Connection: close` is what makes one request per connection a
        // statement rather than an accident.
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\n\
             Connection: close\r\nContent-Length: {}\r\n",
            payload.len()
        );
        if body.is_some() {
            request.push_str("Content-Type: application/json\r\n");
        }
        request.push_str("\r\n");
        request.push_str(&payload);

        stream.write_all(request.as_bytes()).map_err(unreachable)?;
        stream.flush().map_err(unreachable)?;

        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader.read_line(&mut status_line).map_err(unreachable)?;
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse::<u16>().ok())
            .ok_or_else(|| SubstrateError::Unreadable {
                reason: format!("the first line is not a status line: {status_line:?}"),
            })?;

        let mut length: Option<usize> = None;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).map_err(unreachable)?;
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some((name, value)) = header.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse::<usize>().ok();
            }
        }

        let mut text = String::new();
        match length {
            // A declared length is read exactly, so a daemon that keeps the socket open past the
            // body does not leave a caller waiting on an EOF that is not coming.
            Some(length) => {
                let mut bytes = vec![0_u8; length];
                reader.read_exact(&mut bytes).map_err(unreachable)?;
                text = String::from_utf8_lossy(&bytes).into_owned();
            }
            None => {
                reader.read_to_string(&mut text).map_err(unreachable)?;
            }
        }
        Ok((status, text))
    }
}

/// A client of one substrate daemon.
///
/// Holds no connection: every call opens one. A daemon restart between two calls is therefore
/// invisible rather than fatal, which is the right posture for something a probe talks to once at
/// startup and a tool talks to per call.
pub struct Client {
    transport: Box<dyn Transport + Send + Sync>,
    /// The capability snapshot the first `GET /v1/machine` answered, held for the client's life.
    ///
    /// An exec has to name the snapshot it was admitted against, and the daemon states one
    /// snapshot for its own lifetime — so it is asked for once and kept, not fetched before every
    /// exec. The one event that changes it is a daemon restart, and a per-exec probe would not
    /// close that either: the restart can land between the probe and the start, and the daemon
    /// refuses the stale name then exactly as it does now. What the per-exec probe *did* do was
    /// let publication and admission read two different documents.
    snapshot: OnceLock<String>,
}

/// The body every mutating route takes: the operation's id beside its input, and nothing else.
///
/// The daemon's decoder reads `op` before it reads anything — a body without it is refused
/// `request.schema-invalid` before the input is looked at, and a body with a third key is refused
/// the same way. Every body this client posted until 2026-08-29 had only `input`.
fn mutation(op: &str, input: &Value) -> Value {
    serde_json::json!({"op": op, "input": input})
}

impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Client").finish_non_exhaustive()
    }
}

impl Client {
    /// A client over the operator's socket.
    pub fn at(socket: impl AsRef<Path>) -> Self {
        Self::with(UnixTransport::new(socket))
    }

    /// A client over any transport.
    pub fn with(transport: impl Transport + Send + Sync + 'static) -> Self {
        Self {
            transport: Box::new(transport),
            snapshot: OnceLock::new(),
        }
    }

    /// `GET /v1/machine` — what this machine can confine.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError`] when the daemon cannot be reached, refuses, or answers something
    /// this build cannot read.
    pub fn machine(&self) -> Result<Facts, SubstrateError> {
        let (status, body) = self.transport.request("GET", "/v1/machine", None)?;
        if !(200..300).contains(&status) {
            return Err(SubstrateError::Refused { status, body });
        }
        let value: Value =
            serde_json::from_str(&body).map_err(|error| SubstrateError::Unreadable {
                reason: error.to_string(),
            })?;
        // **The envelope, and the first live probe is why it is here.** Every route answers
        // `{api_version, request_id, result: {...}}` — the capability document is the `result`, not
        // the body. This crate read the body until 2026-08-23, when the first real daemon it ever
        // spoke to answered 200 with every fact present and this build saw none of them, because a
        // document with an unexpected shape deserialises into a `Facts` whose map is simply empty.
        // Which would have published no tools and blamed the machine.
        //
        // Both shapes are accepted: a bare document is what every test fixture and every hand-made
        // example is, and refusing one would be refusing the thing the contract's own schemas show.
        let document = value.get("result").unwrap_or(&value);
        let facts: Facts = serde_json::from_value(document.clone()).map_err(|error| {
            SubstrateError::Unreadable {
                reason: error.to_string(),
            }
        })?;
        if let Some(Value::String(snapshot)) = &facts.snapshot {
            let _ = self.snapshot.set(snapshot.clone());
        }
        Ok(facts)
    }

    /// The snapshot an exec is admitted against: the one already held, or the one the daemon
    /// states when asked now.
    ///
    /// `status: 0` on the refusals is not an HTTP status — there was no request. The same
    /// convention `embedded.rs::refused` uses, and for the same reason: the refusal happened on
    /// this side of the wire, so quoting a status would name a daemon that never answered.
    fn admitted_snapshot(&self) -> Result<String, SubstrateError> {
        if let Some(snapshot) = self.snapshot.get() {
            return Ok(snapshot.clone());
        }
        let facts = self.machine()?;
        match facts.snapshot {
            None => Err(SubstrateError::Refused {
                status: 0,
                body: "the substrate daemon's machine document carries no capability snapshot. An \
                       exec has to name the snapshot it was admitted against, so one without it \
                       cannot be admitted confined - and nothing was started."
                    .to_owned(),
            }),
            Some(Value::String(snapshot)) => Ok(snapshot),
            Some(other) => Err(SubstrateError::Refused {
                status: 0,
                body: format!(
                    "the substrate daemon's capability snapshot is not the shape this build \
                     reads: {other}. Nothing was started."
                ),
            }),
        }
    }

    /// What this machine can confine, or nothing at all.
    ///
    /// **Unreachable is not an error here.** A harness with no substrate daemon is a harness whose
    /// confined tools do not exist, which is how this component has run since it was written and a
    /// legitimate way to run now. A probe that failed the launch would make the read-only harness
    /// unlaunchable on a machine that never wanted the other tools.
    ///
    /// A daemon that *is* there and answers something unreadable is a different case and stays an
    /// error: that is a broken deployment, not an absent one.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError`] only when a daemon answered and could not be understood.
    pub fn probe(&self) -> Result<Facts, SubstrateError> {
        match self.machine() {
            Ok(facts) => Ok(facts),
            Err(SubstrateError::Unreachable { .. }) => Ok(Facts::none()),
            Err(other) => Err(other),
        }
    }

    /// `PUT /v1/workspaces/{workspace}/files/{path}` — write one file whole.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError`] when the daemon cannot be reached or refuses. A path that leaves
    /// the workspace is refused **by the daemon**, not here: this process is a client of a boundary
    /// and re-implementing the check would make two answers to one question.
    pub fn file_write(
        &self,
        workspace: &str,
        path: &str,
        text: &str,
    ) -> Result<Value, SubstrateError> {
        let route = format!("/v1/workspaces/{workspace}/files/{path}");
        // **Base64, because the wire says so.** `workspace.file-write` takes
        // `{"encoding": "base64", "data": …}`; this crate sent the text directly until the
        // contract was read against a live daemon. A file's bytes are not a JSON string - a
        // wire that carried them as one could not carry a file with a byte that is not UTF-8.
        let (status, body) = self.transport.request(
            "PUT",
            &route,
            Some(&mutation(
                "workspace.file.write",
                &serde_json::json!({
                    "content": {"encoding": "base64", "data": base64::encode(text.as_bytes())}
                }),
            )),
        )?;
        Self::decode(status, body)
    }

    /// `GET /v1/workspaces/{workspace}/files/{path}` — read one file.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError`] when the daemon cannot be reached, refuses, or answers a document
    /// with no text in it.
    pub fn file_read(&self, workspace: &str, path: &str) -> Result<String, SubstrateError> {
        let route = format!("/v1/workspaces/{workspace}/files/{path}");
        let (status, body) = self.transport.request("GET", &route, None)?;
        let value = Self::decode(status, body)?;
        let data = value
            .pointer("/result/content/data")
            .or_else(|| value.pointer("/content/data"))
            .and_then(Value::as_str)
            .ok_or_else(|| SubstrateError::Unreadable {
                reason: format!("no file content in {value}"),
            })?;
        let bytes = base64::decode(data).map_err(|reason| SubstrateError::Unreadable { reason })?;
        String::from_utf8(bytes).map_err(|error| SubstrateError::Unreadable {
            reason: format!("the file is not text: {error}"),
        })
    }

    /// `POST /v1/workspaces` — open a confined workspace and answer its id.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError`] when the daemon cannot be reached, refuses, or answers a document
    /// with no workspace id in it.
    pub fn workspace_create(&self, lease_ttl_ms: u64) -> Result<String, SubstrateError> {
        // **`labels` is required, and the first live call is how that was learned.** The operation
        // schema lists it beside `source` in `required`, and the daemon answers
        // `request.schema-invalid` to a body without it - a closed schema, so an omission is a
        // refusal rather than a default. An empty map is the honest value: this client labels
        // nothing, and inventing a label would put a word in an operator's mouth.
        let (status, body) = self.transport.request(
            "POST",
            "/v1/workspaces",
            Some(&mutation(
                "workspace.create",
                &serde_json::json!({"source": "empty", "labels": {}, "lease_ttl_ms": lease_ttl_ms}),
            )),
        )?;
        let value = Self::decode(status, body)?;
        value
            .pointer("/result/id")
            .or_else(|| value.pointer("/id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| SubstrateError::Unreadable {
                reason: format!("no workspace id in {value}"),
            })
    }

    /// `POST /v1/execs` then `GET /v1/execs/{id}/output` — run one argv and answer what it did.
    ///
    /// **An argv, never a command line.** The wire takes a list and substrate's own `exec.start`
    /// predicate is `exec.argv-only`; nothing here builds a string a shell would then take apart.
    ///
    /// **And confinement is asked for by name.** This posted `{workspace_id, argv}` until 2026-08-29
    /// — no `sandbox`, so no `require`, no snapshot, no limits. Whether a daemon then ran that
    /// unconfined or refused it was the daemon's choice, and a harness whose whole argument is *a
    /// tool this machine cannot confine does not exist* cannot leave that decision elsewhere. The
    /// request is built by [`crate::confined_exec_input`], the same function the embedded driver
    /// calls, so the two paths cannot ask for different things.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError`] when the daemon cannot be reached or refuses, and
    /// [`SubstrateError::Refused`] before sending anything when the machine document carries no
    /// capability snapshot to name. A program that exits non-zero is **not** an error: it is a
    /// result, and the caller needs to see it.
    pub fn exec(&self, workspace: &str, argv: &[String]) -> Result<Value, SubstrateError> {
        let snapshot = self.admitted_snapshot()?;
        let input = crate::confined_exec_input(
            workspace,
            argv,
            snapshot,
            // Nothing inherited and nothing set: an exec that saw this process's environment would
            // carry a credential into a confined workspace. A toolchain is the embedded driver's
            // to declare; this path has no root to mount one from.
            substrate_wire::ExecEnvironment {
                allow: Vec::new(),
                set: BTreeMap::new(),
            },
            Vec::new(),
        );
        // Serialised by the wire crate's own type, never hand-written: which field is `require` and
        // which is `required` is substrate's to say, and a body assembled here is a second opinion
        // about it.
        let input = serde_json::to_value(&input).map_err(|error| SubstrateError::Unreadable {
            reason: error.to_string(),
        })?;
        let (status, body) =
            self.transport
                .request("POST", "/v1/execs", Some(&mutation("exec.start", &input)))?;
        let started = Self::decode(status, body)?;
        let Some(id) = started
            .pointer("/result/exec_id")
            .or_else(|| started.pointer("/exec_id"))
            .and_then(Value::as_str)
        else {
            return Ok(started);
        };
        let (status, body) =
            self.transport
                .request("GET", &format!("/v1/execs/{id}/output"), None)?;
        Self::decode(status, body)
    }

    fn decode(status: u16, body: String) -> Result<Value, SubstrateError> {
        if !(200..300).contains(&status) {
            return Err(SubstrateError::Refused { status, body });
        }
        serde_json::from_str(&body).map_err(|error| SubstrateError::Unreadable {
            reason: error.to_string(),
        })
    }
}

impl Backend for Client {
    fn machine(&self) -> Result<Facts, SubstrateError> {
        Client::machine(self)
    }

    fn workspace_create(&self, lease_ttl_ms: u64) -> Result<String, SubstrateError> {
        Client::workspace_create(self, lease_ttl_ms)
    }

    fn file_write(&self, workspace: &str, path: &str, text: &str) -> Result<Value, SubstrateError> {
        Client::file_write(self, workspace, path, text)
    }

    fn file_read(&self, workspace: &str, path: &str) -> Result<String, SubstrateError> {
        Client::file_read(self, workspace, path)
    }

    fn exec(&self, workspace: &str, argv: &[String]) -> Result<Value, SubstrateError> {
        Client::exec(self, workspace, argv)
    }
}
