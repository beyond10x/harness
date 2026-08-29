//! Bounded server-sent-event reading.
//!
//! The stream is untrusted input from a network peer, so every read is bounded twice: one event
//! cannot grow without limit, and neither can the stream as a whole. Without the second bound a
//! peer that never closes holds the loop forever while the process grows.
//!
//! # Where this came from
//!
//! `harness-responses`, and byte-for-byte from `harness-messages`, which copied it when the second
//! wire was built. Framing is not vendor-shaped: `data:`, comments, the blank-line terminator and
//! the two bounds are the same on both routes, and neither reader ever looked inside a payload.
//!
//! # The one thing the two routes do not agree on
//!
//! Whether `data: [DONE]` ends the stream. On the first route it is the terminal frame; on the
//! second there is no sentinel at all — the terminal marker is a *payload* (`message_stop`), and a
//! `[DONE]` line arriving there is a payload that is not JSON, which is a protocol refusal and
//! must stay one. Unifying the two would have silently taught the second route a sentinel it does
//! not speak, so the difference is carried as [`Framing`] and each wire names its own.

use std::io::{BufRead, Read};

use harness_wire::{Cancel, WireError, WireErrorCode};
use serde_json::Value;

/// Largest single event payload.
pub const MAX_EVENT_BYTES: usize = 1024 * 1024;

/// Largest total stream length for one turn.
pub const MAX_STREAM_BYTES: usize = 32 * 1024 * 1024;

/// Whether a `data: [DONE]` line is the end of the stream or just a payload.
///
/// Named by the wire at every construction rather than defaulted, because a default here is a
/// route's framing decided by whichever wire was written first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// `data: [DONE]` ends the stream and is reported as [`SseEvent::Done`].
    DoneSentinel,
    /// There is no sentinel: every frame is a payload, and the terminal marker is inside one.
    ///
    /// A `[DONE]` line under this framing is a payload that is not JSON, and refuses as one.
    PayloadsOnly,
}

/// One decoded event: the terminating sentinel, or a JSON payload.
#[derive(Debug, Clone, PartialEq)]
pub enum SseEvent {
    Done,
    Payload(Value),
}

pub struct SseReader<R: BufRead> {
    reader: R,
    framing: Framing,
    consumed: usize,
    cancel: Option<Cancel>,
}

impl<R: BufRead> SseReader<R> {
    pub fn new(reader: R, framing: Framing) -> Self {
        Self {
            reader,
            framing,
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

    /// Reads the next payload, treating the sentinel as the end of the stream.
    ///
    /// What both wires drain with: a decoder that is handed the sentinel has nothing to do with it
    /// but stop, and under [`Framing::PayloadsOnly`] there is no sentinel to hand it in the first
    /// place. [`Self::next_event`] is the same read with the sentinel still visible.
    ///
    /// # Errors
    ///
    /// The same refusals as [`Self::next_event`].
    pub fn next_payload(&mut self) -> Result<Option<Value>, WireError> {
        match self.next_event()? {
            None | Some(SseEvent::Done) => Ok(None),
            Some(SseEvent::Payload(payload)) => Ok(Some(payload)),
        }
    }

    /// Reads the next event, or `None` at end of stream.
    ///
    /// # Errors
    ///
    /// Returns [`harness_wire::WireErrorCode::Cancelled`] when the caller cancelled,
    /// [`harness_wire::WireErrorCode::TooLarge`] when a declared bound is passed,
    /// [`harness_wire::WireErrorCode::Transport`] on a read failure, and
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
                //
                // **Retriable**, unlike every other framing failure here: nothing malformed
                // arrived, the peer stopped mid-frame and closed, and that is what a connection
                // dropping looks like from in here rather than a peer speaking a different
                // protocol. The identical request very likely answers. The code stays `Protocol`
                // because it names what was observed; `retriable` says what to do about it, and
                // the two are different questions. Whether a retry actually happens is decided
                // above: the transport never resends once anything reached the caller.
                if saw_data {
                    return Err(WireError::new(
                        WireErrorCode::Protocol,
                        "the event stream ended inside an event",
                        true,
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
                return self.decode(&data).map(Some);
            }
            if line.starts_with(':') {
                continue;
            }
            let Some((field, value)) = line.split_once(':') else {
                // A bare field name with no value carries nothing a wire uses.
                continue;
            };
            // `event:` names the frame; on both routes the payload repeats it as its own type
            // field, and that is what the decoder reads. Trusting the frame name over the payload
            // would let a mislabelled frame decide the turn.
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

    /// Decodes one collected `data:` payload.
    ///
    /// A payload that is not JSON is **not** retriable, and that is the line this module draws:
    /// the frame arrived whole and its contents are wrong, so the far side is not speaking the
    /// pinned subset and will not start speaking it on a second attempt. A retry there would spend
    /// the run's budget four times over to be told the same thing.
    fn decode(&self, data: &str) -> Result<SseEvent, WireError> {
        if self.framing == Framing::DoneSentinel && data.trim() == "[DONE]" {
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

    fn read_all(body: &str, framing: Framing) -> Result<Vec<SseEvent>, WireError> {
        let mut reader = SseReader::new(body.as_bytes(), framing);
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
            read_all(body, Framing::DoneSentinel).expect("well formed stream"),
            vec![SseEvent::Payload(json!({"type": "a"})), SseEvent::Done]
        );
    }

    #[test]
    fn the_frame_name_is_ignored_and_the_payload_decides() {
        // The second wire's case: every frame is named twice, once by `event:` and once inside the
        // payload, and only the payload is read.
        let body = ": keep-alive\nevent: message_start\ndata: {\"type\":\"message_start\"}\n\n\
                    event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n";
        assert_eq!(
            read_all(body, Framing::PayloadsOnly).expect("well formed stream"),
            vec![
                SseEvent::Payload(json!({"type": "message_start"})),
                SseEvent::Payload(json!({"type": "message_stop"})),
            ]
        );
    }

    #[test]
    fn a_done_line_is_a_sentinel_under_one_framing_and_a_refusal_under_the_other() {
        // The whole of what the two routes disagree about, in one test. Unifying it would have
        // taught the second route a sentinel it does not speak: `[DONE]` there is a frame whose
        // payload is not JSON, and reading it as an orderly end would turn a protocol failure into
        // a turn that stopped early and looked complete.
        assert_eq!(
            read_all("data: [DONE]\n\n", Framing::DoneSentinel).expect("a sentinel ends it"),
            vec![SseEvent::Done]
        );
        let error = read_all("data: [DONE]\n\n", Framing::PayloadsOnly)
            .expect_err("no sentinel on this framing");
        assert_eq!(error.code, WireErrorCode::Protocol);
        assert!(!error.retriable, "{error}");
    }

    #[test]
    fn draining_by_payload_stops_at_the_sentinel() {
        // What both wires' `drain` does: the sentinel is the end of the stream and never reaches a
        // decoder, and a stream without one ends the same way when the bytes run out.
        let mut reader = SseReader::new(
            "data: {\"type\":\"a\"}\n\ndata: [DONE]\n\n".as_bytes(),
            Framing::DoneSentinel,
        );
        assert_eq!(
            reader.next_payload().expect("a payload"),
            Some(json!({"type": "a"}))
        );
        assert_eq!(reader.next_payload().expect("the sentinel"), None);
    }

    #[test]
    fn joins_multi_line_data() {
        let body = "data: {\"type\":\ndata: \"a\"}\n\n";
        assert_eq!(
            read_all(body, Framing::DoneSentinel).expect("multi-line data"),
            vec![SseEvent::Payload(json!({"type": "a"}))]
        );
    }

    #[test]
    fn malformed_json_refuses_as_protocol() {
        let error =
            read_all("data: {not json}\n\n", Framing::DoneSentinel).expect_err("malformed refuses");
        assert_eq!(error.code, WireErrorCode::Protocol);
    }

    #[test]
    fn a_truncated_event_is_not_a_completion_and_is_worth_another_attempt() {
        let error =
            read_all("data: {\"type\":\"a\"}", Framing::DoneSentinel).expect_err("truncation");
        assert_eq!(error.code, WireErrorCode::Protocol);
        assert!(
            error.message.contains("inside an event"),
            "{}",
            error.message
        );
        assert!(
            error.retriable,
            "a peer that stopped mid-frame is a dropped connection, not a different protocol"
        );
    }

    #[test]
    fn a_malformed_payload_is_never_retried() {
        // The other side of the line the truncation case draws. These bytes arrived whole and are
        // wrong; sending the same request again is four ways to be told the same thing.
        let error =
            read_all("data: {not json}\n\n", Framing::DoneSentinel).expect_err("malformed refuses");
        assert!(!error.retriable, "{error}");
    }

    #[test]
    fn a_clean_end_of_stream_is_not_an_error() {
        assert_eq!(
            read_all("", Framing::DoneSentinel).expect("empty stream"),
            Vec::new()
        );
        assert_eq!(
            read_all("", Framing::PayloadsOnly).expect("empty stream"),
            Vec::new()
        );
    }

    #[test]
    fn an_oversized_event_refuses() {
        let body = format!("data: {{\"blob\":\"{}\"}}\n\n", "x".repeat(MAX_EVENT_BYTES));
        assert_eq!(
            read_all(&body, Framing::DoneSentinel)
                .expect_err("oversized event")
                .code,
            WireErrorCode::TooLarge
        );
    }

    #[test]
    fn cancellation_wins_before_the_next_event() {
        let cancel = Cancel::new();
        cancel.cancel();
        let mut reader = SseReader::new(
            "data: {\"type\":\"a\"}\n\n".as_bytes(),
            Framing::PayloadsOnly,
        )
        .with_cancel(cancel);
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
        let mut reader = SseReader::new(Endless, Framing::DoneSentinel);
        let error = reader
            .next_event()
            .expect_err("an unterminated line refuses");
        assert_eq!(error.code, WireErrorCode::TooLarge);
        // The bound named is the per-line one, reached long before the whole-stream one. If this
        // ever reports the stream bound again, the cap moved back to after the allocation.
        assert!(error.message.contains("one line"), "{}", error.message);
    }
}
