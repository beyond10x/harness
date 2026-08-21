//! JSON-lines framing, and the reader thread that makes cancellation mean something.
//!
//! Reading on a background thread is not an optimisation. The loop spends most of a turn blocked on
//! an HTTP read; a server that only looks at its input between messages would see `turn/interrupt`
//! minutes after it was sent, which is indistinguishable from ignoring it. The reader sets the
//! cancel flags the moment the frame arrives, so the in-flight turn actually stops.

use std::io::{BufRead, Read, Write};
use std::sync::mpsc::{Receiver, RecvError, RecvTimeoutError, Sender, TryRecvError, channel};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};

use crate::inventory::MAX_FRAME_BYTES;

/// One decoded frame from the client.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Response {
        id: Value,
        result: Option<Value>,
        error: Option<Value>,
    },
    /// A frame that is not addressable JSON-RPC. Kept rather than dropped so the server can refuse
    /// it out loud instead of appearing to hang.
    Malformed {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("the client closed its input")]
    Closed,
    #[error("writing to the client: {0}")]
    Write(String),
    #[error("{0}")]
    Protocol(String),
    #[error("the client did not answer within {0:?}")]
    TimedOut(Duration),
}

/// Standard JSON-RPC error codes, plus the one this server uses for a pinned method it refuses.
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// Notifies the moment an interrupt frame is seen, before it is queued.
pub trait InterruptWatch: Send {
    fn interrupted(&self);
}

pub struct Reader {
    incoming: Receiver<Incoming>,
    handle: Option<JoinHandle<()>>,
}

impl Reader {
    /// Starts reading `source` on its own thread.
    ///
    /// `watch` fires on `turn/interrupt` before the frame is queued, so a turn blocked on the model
    /// stops without waiting for the main thread to come back to the queue.
    pub fn spawn<R>(mut source: R, watch: Box<dyn InterruptWatch>) -> Self
    where
        R: BufRead + Send + 'static,
    {
        let (sender, incoming) = channel();
        let handle = thread::spawn(move || pump(&mut source, &sender, watch.as_ref()));
        Self {
            incoming,
            handle: Some(handle),
        }
    }

    /// Blocks until the next frame arrives.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::Closed`] once the client's input ends.
    pub fn next(&self) -> Result<Incoming, TransportError> {
        self.incoming
            .recv()
            .map_err(|RecvError| TransportError::Closed)
    }

    /// Blocks for the next frame, but not forever.
    ///
    /// A client that never answers a request it was asked to serve would otherwise hold this
    /// process open with a turn that can never end and nobody able to see why.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::TimedOut`] when nothing arrives in time, or
    /// [`TransportError::Closed`] once the client's input ends.
    pub fn next_within(&self, timeout: Duration) -> Result<Incoming, TransportError> {
        self.incoming
            .recv_timeout(timeout)
            .map_err(|error| match error {
                RecvTimeoutError::Timeout => TransportError::TimedOut(timeout),
                RecvTimeoutError::Disconnected => TransportError::Closed,
            })
    }

    /// Returns a frame only if one is already waiting.
    ///
    /// This is what lets a running turn answer an interrupt: the main thread is inside the loop,
    /// not at the wire, so the only chance to notice a control frame is between the events it is
    /// already emitting.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::Closed`] once the client's input ends.
    pub fn try_next(&self) -> Result<Option<Incoming>, TransportError> {
        match self.incoming.try_recv() {
            Ok(message) => Ok(Some(message)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(TransportError::Closed),
        }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // The thread ends when its source reaches EOF. Joining is best effort: a client that keeps
        // its pipe open must not stop this process from exiting.
        if let Some(handle) = self.handle.take()
            && handle.is_finished()
        {
            let _ = handle.join();
        }
    }
}

fn pump<R: BufRead>(source: &mut R, sender: &Sender<Incoming>, watch: &dyn InterruptWatch) {
    loop {
        let mut line = String::new();
        // The cap is applied to the read itself. Reading first and measuring afterwards lets a
        // peer that never sends a newline choose how much memory this process allocates.
        match source
            .by_ref()
            .take(MAX_FRAME_BYTES as u64 + 1)
            .read_line(&mut line)
        {
            Ok(0) => return,
            Ok(_) => {}
            Err(error) => {
                let _ = sender.send(Incoming::Malformed {
                    reason: format!("reading the client: {error}"),
                });
                return;
            }
        }
        let trimmed = line.trim();
        // `take` already capped the allocation; this only decides what to say about it. A final
        // frame with no trailing newline is legal and must still be served.
        if trimmed.len() > MAX_FRAME_BYTES {
            let _ = sender.send(Incoming::Malformed {
                reason: format!("a frame passed the {MAX_FRAME_BYTES} byte bound"),
            });
            return;
        }
        if trimmed.is_empty() {
            continue;
        }
        let message = decode(trimmed);
        if matches!(&message, Incoming::Request { method, .. } if method == "turn/interrupt") {
            watch.interrupted();
        }
        if sender.send(message).is_err() {
            return;
        }
    }
}

fn decode(line: &str) -> Incoming {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Incoming::Malformed {
            reason: "a frame was not JSON".to_owned(),
        };
    };
    let id = value.get("id").cloned();
    if let Some(method) = value.get("method").and_then(Value::as_str) {
        let params = value.get("params").cloned().unwrap_or(json!({}));
        return match id {
            Some(id) => Incoming::Request {
                id,
                method: method.to_owned(),
                params,
            },
            None => Incoming::Notification {
                method: method.to_owned(),
                params,
            },
        };
    }
    match id {
        Some(id) => Incoming::Response {
            id,
            result: value.get("result").cloned(),
            error: value.get("error").cloned(),
        },
        None => Incoming::Malformed {
            reason: "a frame carried neither a method nor an id".to_owned(),
        },
    }
}

/// The write half. Only the main thread writes, so ordering on the wire is the order of events.
pub struct Writer {
    sink: Box<dyn Write + Send>,
    next_id: i64,
}

impl Writer {
    pub fn new(sink: Box<dyn Write + Send>) -> Self {
        Self { sink, next_id: 1 }
    }

    pub fn take_request_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Writes one frame and flushes it.
    ///
    /// Flushing per frame is required, not tidiness: the client is blocked reading, and a buffered
    /// notification is one it never sees.
    ///
    /// # Errors
    ///
    /// Returns [`TransportError::Write`] when the client's pipe is gone.
    pub fn send(&mut self, frame: &Value) -> Result<(), TransportError> {
        let line = serde_json::to_string(frame)
            .map_err(|error| TransportError::Write(format!("encoding a frame: {error}")))?;
        writeln!(self.sink, "{line}")
            .and_then(|()| self.sink.flush())
            .map_err(|error| TransportError::Write(error.to_string()))
    }

    /// # Errors
    ///
    /// Returns [`TransportError::Write`] when the client's pipe is gone.
    pub fn notify(&mut self, method: &str, params: &Value) -> Result<(), TransportError> {
        self.send(&json!({"method": method, "params": params}))
    }

    /// # Errors
    ///
    /// Returns [`TransportError::Write`] when the client's pipe is gone.
    pub fn respond(&mut self, id: &Value, result: &Value) -> Result<(), TransportError> {
        self.send(&json!({"id": id, "result": result}))
    }

    /// # Errors
    ///
    /// Returns [`TransportError::Write`] when the client's pipe is gone.
    pub fn respond_error(
        &mut self,
        id: &Value,
        code: i64,
        message: impl Into<String>,
    ) -> Result<(), TransportError> {
        self.send(&json!({"id": id, "error": {"code": code, "message": message.into()}}))
    }

    /// # Errors
    ///
    /// Returns [`TransportError::Write`] when the client's pipe is gone.
    pub fn request(&mut self, id: i64, method: &str, params: &Value) -> Result<(), TransportError> {
        self.send(&json!({"id": id, "method": method, "params": params}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Flag(Arc<AtomicBool>);

    impl InterruptWatch for Flag {
        fn interrupted(&self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[test]
    fn requests_notifications_and_responses_are_told_apart() {
        assert_eq!(
            decode(r#"{"id":1,"method":"turn/start","params":{"a":1}}"#),
            Incoming::Request {
                id: json!(1),
                method: "turn/start".to_owned(),
                params: json!({"a": 1}),
            }
        );
        assert_eq!(
            decode(r#"{"method":"initialized","params":{}}"#),
            Incoming::Notification {
                method: "initialized".to_owned(),
                params: json!({}),
            }
        );
        assert_eq!(
            decode(r#"{"id":7,"result":{"success":true}}"#),
            Incoming::Response {
                id: json!(7),
                result: Some(json!({"success": true})),
                error: None,
            }
        );
    }

    #[test]
    fn a_frame_with_no_method_and_no_id_is_malformed_rather_than_ignored() {
        assert!(matches!(decode("{}"), Incoming::Malformed { .. }));
        assert!(matches!(decode("not json"), Incoming::Malformed { .. }));
    }

    #[test]
    fn a_request_without_params_defaults_to_an_empty_object() {
        assert_eq!(
            decode(r#"{"id":1,"method":"initialize"}"#),
            Incoming::Request {
                id: json!(1),
                method: "initialize".to_owned(),
                params: json!({}),
            }
        );
    }

    #[test]
    fn an_interrupt_fires_the_watch_before_it_is_queued() {
        let flag = Arc::new(AtomicBool::new(false));
        let reader = Reader::spawn(
            std::io::Cursor::new(r#"{"id":2,"method":"turn/interrupt","params":{}}"#.to_owned()),
            Box::new(Flag(Arc::clone(&flag))),
        );
        let message = reader.next().expect("the frame arrives");
        assert!(
            flag.load(Ordering::SeqCst),
            "the watch must fire, or a blocked turn never learns it was cancelled"
        );
        assert!(matches!(message, Incoming::Request { method, .. } if method == "turn/interrupt"));
    }

    #[test]
    fn blank_lines_are_skipped_and_end_of_input_closes() {
        let reader = Reader::spawn(
            std::io::Cursor::new("\n\n{\"method\":\"initialized\"}\n".to_owned()),
            Box::new(Flag(Arc::new(AtomicBool::new(false)))),
        );
        assert!(matches!(
            reader.next().expect("the frame arrives"),
            Incoming::Notification { .. }
        ));
        assert_eq!(
            reader.next().expect_err("input ends"),
            TransportError::Closed
        );
    }

    #[test]
    fn request_ids_do_not_repeat() {
        let mut writer = Writer::new(Box::new(Vec::new()));
        assert_eq!(writer.take_request_id(), 1);
        assert_eq!(writer.take_request_id(), 2);
    }
}
