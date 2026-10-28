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
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;
use substrate_wire::OutputStream;

use crate::{Backend, Facts, ProcessWorkspaceAccess, SubstrateError, base64};

/// The most one read asks the daemon for when the daemon has not said what it admits.
///
/// The same figure the confined provider caps a reply at; a daemon that states a lower ceiling
/// is asked for that instead.
const READ_LIMIT_BYTES: u64 = 256 * 1024;

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

    /// [`request`](Self::request), waiting up to `read_timeout` for the answer.
    ///
    /// An exec started with `wait: true` holds the connection open until the program exits, so
    /// the ten seconds a probe is given would cut a build off mid-way and report the daemon
    /// unreachable. Defaulted to [`request`](Self::request) for a transport that does not wait.
    ///
    /// # Errors
    ///
    /// As [`request`](Self::request).
    fn request_within(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        read_timeout: Duration,
    ) -> Result<(u16, String), SubstrateError> {
        let _ = read_timeout;
        self.request(method, path, body)
    }
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
        self.request_within(method, path, body, TIMEOUT)
    }

    fn request_within(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        read_timeout: Duration,
    ) -> Result<(u16, String), SubstrateError> {
        let unreachable = |source: std::io::Error| SubstrateError::Unreachable {
            path: self.socket.display().to_string(),
            source,
        };

        let mut stream = UnixStream::connect(&self.socket).map_err(unreachable)?;
        stream
            .set_read_timeout(Some(read_timeout))
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
    /// The machine document the first `GET /v1/machine` answered, held for the client's life.
    ///
    /// An exec has to name the snapshot it was admitted against, and the daemon states one
    /// snapshot for its own lifetime — so it is asked for once and kept, not fetched before every
    /// exec. The one event that changes it is a daemon restart, and a per-exec probe would not
    /// close that either: the restart can land between the probe and the start, and the daemon
    /// refuses the stale name then exactly as it does now. What the per-exec probe *did* do was
    /// let publication and admission read two different documents. A read takes its byte ceiling
    /// from the same document.
    machine: OnceLock<Facts>,
    /// The next operation id's sequence number, per client.
    next_operation: AtomicU64,
}

/// One mutation's identity, in the shape substrate admits: `common.json#/$defs/operation-id`,
/// 16 to 128 of `[A-Za-z0-9_-]`, **minted by the caller**.
///
/// `op` is not the operation's name. It is an idempotency key the daemon reserves against the
/// request's hash: the same id with the same body answers the same result, and the same id with
/// a different body is refused. This client sent `"workspace.create"` there for one afternoon and
/// the daemon refused it `request.schema-invalid` at `input` — the `.` is outside the charset, and
/// the refusal names the value it was given, which is how it read as the daemon's own operation
/// name. Verified against a daemon built from the pinned revision on 2026-08-29.
///
/// Time, process and a sequence, so two clients in one process, two processes in one second and
/// two calls on one client all mint different ids — and a daemon whose state outlives this process
/// cannot see an id it reserved before. Its own function so the shape is testable without a
/// daemon, for the reason [`crate::embedded::exec_identity`] is one.
pub(crate) fn operation_identity(nanos: u128, process: u32, sequence: u64) -> String {
    format!("op_{nanos}_{process}_{sequence}")
}

/// Whether `id` is a wire `workspace-id`: `^ws_[A-Za-z0-9]+$` (substrate ADR 0035 decision 2).
///
/// Written here because substrate-wire's own check is private. Byte-wise and ASCII-only, so a
/// non-ASCII letter, a `/`, a `?`, whitespace or a control byte is outside it.
pub(crate) fn is_workspace_id(id: &str) -> bool {
    id.strip_prefix("ws_").is_some_and(|rest| {
        !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

/// One stream of one finished exec, as the model will read it.
struct Output {
    text: String,
    truncated: bool,
    eof: bool,
    slice: Value,
}

/// The body every mutating route takes: a fresh operation id beside the input, and nothing else.
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
            machine: OnceLock::new(),
            next_operation: AtomicU64::new(0),
        }
    }

    /// A fresh operation id for one mutation.
    fn operation(&self) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        operation_identity(
            nanos,
            std::process::id(),
            self.next_operation.fetch_add(1, Ordering::Relaxed),
        )
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
        let _ = self.machine.set(facts.clone());
        Ok(facts)
    }

    /// The machine document already held, or the one the daemon states when asked now.
    fn facts(&self) -> Result<Facts, SubstrateError> {
        match self.machine.get() {
            Some(facts) => Ok(facts.clone()),
            None => self.machine(),
        }
    }

    /// The snapshot an exec is admitted against: the one already held, or the one the daemon
    /// states when asked now.
    ///
    /// `status: 0` on the refusals is not an HTTP status — there was no request. The same
    /// convention `embedded.rs::refused` uses, and for the same reason: the refusal happened on
    /// this side of the wire, so quoting a status would name a daemon that never answered.
    fn admitted_snapshot(&self) -> Result<String, SubstrateError> {
        match self.facts()?.snapshot {
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
                &self.operation(),
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
        // **The query is not optional.** `FileReadQuery` in `file` mode needs `offset` and
        // `limit_bytes` both present and nothing else; a bare `GET` is refused
        // `request.schema-invalid` at `query`, which is what every read this client made until
        // 2026-08-29 got. The ceiling is the daemon's own `workspace.read-limit-bytes` where the
        // client has probed it — asking for more is refused by the operation's predicate — and
        // the figure the confined provider bounds its replies at otherwise.
        let limit = self
            .machine
            .get()
            .and_then(|facts| facts.get("workspace.read-limit-bytes"))
            .and_then(Value::as_u64)
            .unwrap_or(READ_LIMIT_BYTES);
        let route = format!(
            "/v1/workspaces/{workspace}/files/{path}?mode=file&offset=0&limit_bytes={limit}"
        );
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
                &self.operation(),
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

    /// `POST /v1/workspaces/adopt` — serve a directory the operator placed under the daemon's
    /// workspace root, and answer the id the daemon minted for it (substrate ADR 0035).
    ///
    /// **The answer is a `ws_…` id, never the root name.** Every later route — files, `exec.start`
    /// — addresses the workspace by that id; the root name only says which directory to adopt.
    ///
    /// A root name outside substrate's set is refused here, before anything is sent, with
    /// `status: 0` as [`Self::admitted_snapshot`] refuses: the daemon would refuse it at
    /// `root_name`, and a request that cannot succeed only spends one of its ledger entries.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError::Refused`] with the daemon's status and body unchanged for every
    /// refusal it answers — `workspace.already-exists` (409) when this subject already adopted the
    /// name, `resource.not-found` (404) when there is no such directory or another subject holds
    /// it, `workspace.path-escape` (422) when the entry is not a directory this daemon owns — and
    /// [`SubstrateError::Unreadable`] when the answer carries no workspace id, an id outside the
    /// published `^ws_[A-Za-z0-9]+$`, or a row for another root name or an origin other than
    /// `adopted`.
    pub fn workspace_adopt(&self, root_name: &str) -> Result<String, SubstrateError> {
        if !substrate_wire::is_root_name(root_name) {
            return Err(SubstrateError::Refused {
                status: 0,
                body: format!(
                    "{root_name:?} is not a substrate root name: one path component of ASCII \
                     alphanumerics, `_` and `-`, 1 to {} bytes, not beginning with `-` - so \
                     nothing was sent",
                    substrate_wire::MAX_ROOT_NAME_BYTES
                ),
            });
        }
        // Serialised by the wire crate's own type, as an exec input is: which members the closed
        // input schema admits is substrate's to say. `labels` is required and empty for the reason
        // `workspace_create` gives.
        let input = serde_json::to_value(substrate_wire::WorkspaceAdoptInput {
            root_name: root_name.into(),
            labels: substrate_wire::Labels::new(),
        })
        .map_err(|error| SubstrateError::Unreadable {
            reason: error.to_string(),
        })?;
        let (status, body) = self.transport.request(
            "POST",
            "/v1/workspaces/adopt",
            Some(&mutation(&self.operation(), &input)),
        )?;
        let value = Self::decode(status, body)?;
        let resource = value.get("result").unwrap_or(&value);
        let Some(id) = resource.get("id").and_then(Value::as_str) else {
            return Err(SubstrateError::Unreadable {
                reason: format!("no workspace id in {value}"),
            });
        };
        // **The id is checked before anything carries it.** It goes verbatim into every later
        // route's path and into the run's stderr notice, so an answer outside the published
        // pattern (ADR 0035 decision 2) is not a workspace this client will act in. Quoted with
        // `{:?}` so a refusal cannot print a line break or a control byte it was handed.
        if !is_workspace_id(id) {
            return Err(SubstrateError::Unreadable {
                reason: format!(
                    "the adopt answered the workspace id {id:?}, which is outside the published \
                     pattern `^ws_[A-Za-z0-9]+$`"
                ),
            });
        }
        // Decision 3: the result is the row for this root name, with `origin: adopted`. Another
        // name or origin is not the directory that was asked for, and acting in it would write
        // somewhere the operator did not name.
        let root = resource.get("root_name").and_then(Value::as_str);
        if root != Some(root_name) {
            return Err(SubstrateError::Unreadable {
                reason: format!(
                    "the adopt of {root_name:?} answered a workspace for root name {root:?}"
                ),
            });
        }
        let origin = resource.get("origin").and_then(Value::as_str);
        if origin != Some("adopted") {
            return Err(SubstrateError::Unreadable {
                reason: format!(
                    "the adopt of {root_name:?} answered a workspace whose origin is {origin:?}, \
                     not \"adopted\""
                ),
            });
        }
        Ok(id.to_owned())
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
    /// request is built by `crate::confined_exec_input`, the same function the embedded driver
    /// calls, so the two paths cannot ask for different things.
    ///
    /// # Errors
    ///
    /// Returns [`SubstrateError`] when the daemon cannot be reached or refuses, and
    /// [`SubstrateError::Refused`] before sending anything when the machine document carries no
    /// capability snapshot to name. A program that exits non-zero is **not** an error: it is a
    /// result, and the caller needs to see it.
    pub fn exec(
        &self,
        workspace: &str,
        argv: &[String],
        workspace_access: &ProcessWorkspaceAccess,
        remaining: Option<Duration>,
    ) -> Result<Value, SubstrateError> {
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
            workspace_access.clone(),
            remaining,
        );
        let timeout_ms = input.limits.timeout_ms;
        let output_bytes = input.limits.output_bytes;
        // Serialised by the wire crate's own type, never hand-written: which field is `require` and
        // which is `required` is substrate's to say, and a body assembled here is a second opinion
        // about it.
        let input = serde_json::to_value(&input).map_err(|error| SubstrateError::Unreadable {
            reason: error.to_string(),
        })?;
        // `wait: true` holds the connection open until the program exits, so the answer is waited
        // for as long as the exec itself may run, plus the slack a probe gets.
        let (status, body) = self.transport.request_within(
            "POST",
            "/v1/execs",
            Some(&mutation(&self.operation(), &input)),
            Duration::from_millis(timeout_ms) + TIMEOUT,
        )?;
        let started = Self::decode(status, body)?;
        // The resource is the `result`, and its `id` is what the output routes take. Read off a
        // live daemon on 2026-08-29: this looked for `exec_id`, and because a miss fell through to
        // answering the start document, the model got an exit code and never the program's output.
        let observed = started
            .get("result")
            .cloned()
            .unwrap_or_else(|| started.clone());
        let Some(id) = observed.get("id").and_then(Value::as_str) else {
            return Err(SubstrateError::Unreadable {
                reason: format!("no exec id in {started}"),
            });
        };
        let stdout = self.output(id, OutputStream::Stdout, output_bytes)?;
        let stderr = self.output(id, OutputStream::Stderr, output_bytes)?;
        // The shape the embedded path answers, so a run's replies do not change when it is
        // confined over a socket instead of in-process — and `stdout_truncated` is part of it: a
        // partial answer that looked whole would be read as the whole answer.
        Ok(serde_json::json!({
            "stdout": stdout.text,
            "stderr": stderr.text,
            "stdout_truncated": stdout.truncated,
            "stderr_truncated": stderr.truncated,
            "output_complete": stdout.eof && stderr.eof,
            "exit": observed,
            "slice": stdout.slice,
        }))
    }

    /// `GET /v1/execs/{id}/output` — one stream of one exec, from its start.
    ///
    /// The query is not optional: `ExecOutputQuery` needs `stream`, `offset` and `limit_bytes`,
    /// and the ceiling is the one the exec was started with, which the daemon's
    /// `exec.output-limit-bytes` predicate already admitted.
    fn output(
        &self,
        id: &str,
        stream: OutputStream,
        limit_bytes: u64,
    ) -> Result<Output, SubstrateError> {
        // The wire crate spells the stream; nothing here guesses at `stdout` versus `Stdout`.
        let stream = serde_json::to_value(stream)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_default();
        let route =
            format!("/v1/execs/{id}/output?stream={stream}&offset=0&limit_bytes={limit_bytes}");
        let (status, body) = self.transport.request("GET", &route, None)?;
        let value = Self::decode(status, body)?;
        let slice = value.get("result").cloned().unwrap_or(value);
        let data = slice
            .pointer("/content/data")
            .and_then(Value::as_str)
            .ok_or_else(|| SubstrateError::Unreadable {
                reason: format!("no output content in {slice}"),
            })?;
        let bytes = base64::decode(data).map_err(|reason| SubstrateError::Unreadable { reason })?;
        let eof = slice.get("eof").and_then(Value::as_bool).unwrap_or(false);
        Ok(Output {
            text: String::from_utf8_lossy(&bytes).into_owned(),
            // Cut by the daemon at its own ceiling, or more to read past this slice: either way
            // the model has not seen all of it.
            truncated: slice
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || !eof,
            eof,
            slice,
        })
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

    fn exec(
        &self,
        workspace: &str,
        argv: &[String],
        workspace_access: &ProcessWorkspaceAccess,
        remaining: Option<Duration>,
    ) -> Result<Value, SubstrateError> {
        Client::exec(self, workspace, argv, workspace_access, remaining)
    }
}
