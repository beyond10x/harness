//! Bounded server-sent-event reading.
//!
//! The stream is untrusted input from a network peer, so every read is bounded twice: one event
//! cannot grow without limit, and neither can the stream as a whole. Without the second bound a
//! peer that never closes holds the loop forever while the process grows.

use std::io::{BufRead, Read};

#[cfg(test)]
use harness_wire::WireErrorCode;
use harness_wire::{Cancel, WireError};
use serde_json::Value;

/// Largest single event payload.
pub const MAX_EVENT_BYTES: usize = 1024 * 1024;

/// Largest total stream length for one turn.
pub const MAX_STREAM_BYTES: usize = 32 * 1024 * 1024;

/// One decoded event: the terminating `[DONE]` sentinel, or a JSON payload.
#[derive(Debug, Clone, PartialEq)]
pub enum SseEvent {
    Done,
    Payload(Value),
}

pub struct SseReader<R: BufRead> {
    reader: R,
    consumed: usize,
    cancel: Option<Cancel>,
}

impl<R: BufRead> SseReader<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            consumed: 0,
            cancel: None,
        }
    }

    #[must_use]
    pub fn with_cancel(mut self, cancel: Cancel) -> Self {
        self.cancel = Some(cancel);
        self
    }

    fn cancelled(&self) -> bool {
        self.cancel.as_ref().is_some_and(Cancel::is_cancelled)
    }

    /// Reads the next event, or `None` at end of stream.
    ///
    /// # Errors
    ///
    /// Returns [`harness_wire::WireErrorCode::Cancelled`] when the caller cancelled, [`harness_wire::WireErrorCode::TooLarge`]
    /// when a declared bound is passed, [`harness_wire::WireErrorCode::Transport`] on a read failure, and
    /// [`harness_wire::WireErrorCode::Protocol`] when a `data:` payload is not JSON.
    pub fn next_event(&mut self) -> Result<Option<SseEvent>, WireError> {
        let mut data = String::new();
        let mut saw_data = false;
        loop {
            if self.cancelled() {
                return Err(WireError::cancelled());
            }
            let mut line = String::new();
            // Capped at the read, not after it. A peer that never sends a newline would otherwise
            // decide how much memory this process allocates, which is the whole point of the
            // bounds this module declares.
            let read = self
                .reader
                .by_ref()
                .take(MAX_EVENT_BYTES as u64 + 1)
                .read_line(&mut line)
                .map_err(|error| {
                    WireError::transport(format!("reading the event stream: {error}"))
                })?;
            if line.len() > MAX_EVENT_BYTES {
                return Err(WireError::too_large(format!(
                    "one line passed the {MAX_EVENT_BYTES} byte bound"
                )));
            }
            if read == 0 {
                // End of stream. A half-collected event is a truncation, not a completion.
                if saw_data {
                    return Err(WireError::protocol(
                        "the event stream ended inside an event",
                    ));
                }
                return Ok(None);
            }
            self.consumed = self.consumed.saturating_add(read);
            if self.consumed > MAX_STREAM_BYTES {
                return Err(WireError::too_large(format!(
                    "the event stream passed the {MAX_STREAM_BYTES} byte bound"
                )));
            }

            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                if !saw_data {
                    continue;
                }
                return Self::decode(&data).map(Some);
            }
            if line.starts_with(':') {
                continue;
            }
            let Some((field, value)) = line.split_once(':') else {
                // A bare field name with no value carries nothing this wire uses.
                continue;
            };
            if field != "data" {
                continue;
            }
            if saw_data {
                data.push('\n');
            }
            data.push_str(value.strip_prefix(' ').unwrap_or(value));
            saw_data = true;
            if data.len() > MAX_EVENT_BYTES {
                return Err(WireError::too_large(format!(
                    "one event passed the {MAX_EVENT_BYTES} byte bound"
                )));
            }
        }
    }

    fn decode(data: &str) -> Result<SseEvent, WireError> {
        if data.trim() == "[DONE]" {
            return Ok(SseEvent::Done);
        }
        serde_json::from_str(data)
            .map(SseEvent::Payload)
            .map_err(|error| WireError::protocol(format!("event payload is not JSON: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn read_all(body: &str) -> Result<Vec<SseEvent>, WireError> {
        let mut reader = SseReader::new(body.as_bytes());
        let mut events = Vec::new();
        while let Some(event) = reader.next_event()? {
            events.push(event);
        }
        Ok(events)
    }

    #[test]
    fn reads_events_and_ignores_comments_and_other_fields() {
        let body =
            ": keep-alive\nevent: response.created\ndata: {\"type\":\"a\"}\n\ndata: [DONE]\n\n";
        assert_eq!(
            read_all(body).expect("well formed stream"),
            vec![SseEvent::Payload(json!({"type": "a"})), SseEvent::Done]
        );
    }

    #[test]
    fn joins_multi_line_data() {
        let body = "data: {\"type\":\ndata: \"a\"}\n\n";
        assert_eq!(
            read_all(body).expect("multi-line data"),
            vec![SseEvent::Payload(json!({"type": "a"}))]
        );
    }

    #[test]
    fn malformed_json_refuses_as_protocol() {
        let error = read_all("data: {not json}\n\n").expect_err("malformed payload refuses");
        assert_eq!(error.code, WireErrorCode::Protocol);
    }

    #[test]
    fn a_truncated_event_is_not_a_completion() {
        let error = read_all("data: {\"type\":\"a\"}").expect_err("truncation refuses");
        assert_eq!(error.code, WireErrorCode::Protocol);
        assert!(
            error.message.contains("inside an event"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_clean_end_of_stream_is_not_an_error() {
        assert_eq!(read_all("").expect("empty stream"), Vec::new());
    }

    #[test]
    fn an_oversized_event_refuses() {
        let body = format!("data: {{\"blob\":\"{}\"}}\n\n", "x".repeat(MAX_EVENT_BYTES));
        assert_eq!(
            read_all(&body).expect_err("oversized event").code,
            WireErrorCode::TooLarge
        );
    }

    #[test]
    fn cancellation_wins_before_the_next_event() {
        let cancel = Cancel::new();
        cancel.cancel();
        let mut reader =
            SseReader::new("data: {\"type\":\"a\"}\n\n".as_bytes()).with_cancel(cancel);
        assert_eq!(
            reader.next_event().expect_err("cancelled").code,
            WireErrorCode::Cancelled
        );
    }
}

#[cfg(test)]
mod bounds {
    use super::*;

    /// A reader that never produces a newline, like a peer that opened a stream and went quiet.
    struct Endless;

    impl std::io::Read for Endless {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            buffer.fill(b'x');
            Ok(buffer.len())
        }
    }

    impl BufRead for Endless {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
            Ok(&[b'x'; 4096])
        }

        fn consume(&mut self, _: usize) {}
    }

    #[test]
    fn a_peer_that_never_sends_a_newline_cannot_choose_the_allocation() {
        let mut reader = SseReader::new(Endless);
        let error = reader
            .next_event()
            .expect_err("an unterminated line refuses");
        assert_eq!(error.code, WireErrorCode::TooLarge);
        // The bound named is the per-line one, reached long before the whole-stream one. If this
        // ever reports the stream bound again, the cap moved back to after the allocation.
        assert!(error.message.contains("one line"), "{}", error.message);
    }
}
