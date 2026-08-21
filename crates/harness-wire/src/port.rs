use serde::{Deserialize, Serialize};

use crate::WireError;
use crate::id::{CallId, WireId};
use crate::item::{ToolCall, ToolOutcome};
use crate::turn::{ToolSpec, TurnOutcome, TurnRequest};

/// What a caller can watch while a turn is still running.
///
/// This exists so text appears as it is produced rather than when the turn ends. A harness that
/// only reports at the end is one a person cannot interrupt on the strength of what they read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StreamEvent {
    TextDelta {
        text: String,
    },
    ToolArgumentsDelta {
        call_id: CallId,
        delta: String,
    },
    /// Something the wire saw and did not understand, preserved instead of dropped.
    Warning {
        code: String,
        message: String,
    },
}

pub trait StreamSink {
    fn emit(&mut self, event: StreamEvent);
}

/// A sink that retains everything, for tests and for callers that only want the transcript.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VecSink {
    events: Vec<StreamEvent>,
}

impl VecSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> &[StreamEvent] {
        &self.events
    }

    /// Returns the concatenated text deltas, which is the assistant text as a reader saw it grow.
    pub fn text(&self) -> String {
        self.events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn into_events(self) -> Vec<StreamEvent> {
        self.events
    }
}

impl StreamSink for VecSink {
    fn emit(&mut self, event: StreamEvent) {
        self.events.push(event);
    }
}

impl StreamSink for &mut dyn StreamSink {
    fn emit(&mut self, event: StreamEvent) {
        (**self).emit(event);
    }
}

/// One documented model API, projected into this crate's values.
pub trait ModelPort {
    /// Identifies this projection. Opaque items carry it so they cannot cross to another wire.
    fn wire(&self) -> &WireId;

    /// Runs exactly one turn.
    ///
    /// # Errors
    ///
    /// Returns a typed [`WireError`]. An implementation must not retry on another endpoint,
    /// downgrade to a different wire, or invent a completion it did not receive.
    fn turn(
        &mut self,
        request: &TurnRequest,
        sink: &mut dyn StreamSink,
    ) -> Result<TurnOutcome, WireError>;
}

/// Where the loop's tools come from.
///
/// The one seam that makes the embedded and the bridged harness the same loop: in-process this is
/// backed by direct calls, and under a bridge by a callback over the wire. The loop cannot tell.
pub trait ToolPort {
    /// The complete set of tools published for the next turn.
    fn specs(&self) -> &[ToolSpec];

    /// Runs exactly one call the loop already checked against [`ToolPort::specs`].
    ///
    /// A failure is an outcome with `failed` set, not an error: the model has to see that the tool
    /// ran and did not work, or it will assume the effect landed.
    fn call(&mut self, call: &ToolCall) -> ToolOutcome;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec_sink_reassembles_streamed_text() {
        let mut sink = VecSink::new();
        sink.emit(StreamEvent::TextDelta {
            text: "Hel".to_owned(),
        });
        sink.emit(StreamEvent::Warning {
            code: "unknown-event".to_owned(),
            message: "ignored".to_owned(),
        });
        sink.emit(StreamEvent::TextDelta {
            text: "lo".to_owned(),
        });
        assert_eq!(sink.text(), "Hello");
        assert_eq!(sink.events().len(), 3);
    }

    #[test]
    fn stream_events_round_trip() {
        let event = StreamEvent::ToolArgumentsDelta {
            call_id: CallId::new("call-1").expect("valid"),
            delta: "{\"p\":".to_owned(),
        };
        let encoded = serde_json::to_value(&event).expect("serializes");
        assert_eq!(
            serde_json::from_value::<StreamEvent>(encoded).expect("deserializes"),
            event
        );
    }
}
