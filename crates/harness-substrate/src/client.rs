//! HTTP/1.1 over an owner-permissioned Unix socket, by hand.
//!
//! The refusal to take an HTTP crate is argued in the crate root. What is here is the smallest
//! thing that speaks four routes: one request per connection, `Content-Length` bodies, and no
//! streaming, redirects, compression or TLS — none of which a Unix socket to a local daemon
//! serving bounded JSON has any use for.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::{Facts, SubstrateError};

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
        stream.set_read_timeout(Some(TIMEOUT)).map_err(unreachable)?;
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
            if let Some((name, value)) = header.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse::<usize>().ok();
                }
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
        serde_json::from_str(&body).map_err(|error| SubstrateError::Unreadable {
            reason: error.to_string(),
        })
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
}
