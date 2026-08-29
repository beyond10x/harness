use harness_wire::{CallId, ToolCall, ToolName, Usage};
use serde::{Deserialize, Serialize};

use crate::LoopStop;

/// Everything a person or a shell can observe while the loop runs.
///
/// Deliberately closed and vendor-neutral: the terminal renderer, the bridge shell and the test
/// assertions all read the same stream, so there is one description of what happened rather than
/// three that can disagree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum LoopEvent {
    Started {
        model: String,
        published_tools: Vec<ToolName>,
        /// Every neutral operation this run could perform — see [`harness_wire::ToolPort::operations`].
        ///
        /// Beside the tool names rather than instead of them, because they answer different
        /// questions and a reader needs both: what the model was **offered**, and what the run
        /// could **do**. Behind three verbs the first is the same on every run and only the second
        /// varies.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        operations: Vec<String>,
    },
    TurnStarted {
        turn: u64,
    },
    TextDelta {
        text: String,
    },
    ToolArgumentsDelta {
        call_id: CallId,
        delta: String,
    },
    ToolRequested(ToolCall),
    ApprovalRequired {
        call_id: CallId,
        /// What is being decided. For a verb over a catalogue this is the **entry** — `run`,
        /// `file_write` — and not the verb it came through; the `ToolRequested` event with the
        /// same `call_id` carries the verb and its arguments.
        name: ToolName,
    },
    ApprovalResolved {
        call_id: CallId,
        approved: bool,
    },
    ToolCompleted {
        call_id: CallId,
        failed: bool,
    },
    /// Reported token counts for one turn. Absent usage produces no event at all.
    Usage(Usage),
    /// The rate card in force, emitted once before the first turn.
    ///
    /// Carried in the record rather than left on the command line so that every cost figure below
    /// can be traced to the rates that produced it — and so that a run priced from a stale card is
    /// distinguishable from one priced this morning. A run with no card emits no event here, and
    /// then reports no cost at all.
    Rates {
        source: String,
        as_of: String,
    },
    /// What one turn's reported tokens cost, in millionths of a US dollar.
    ///
    /// Emitted only where the card prices the model the provider served. **A turn nobody could
    /// price produces no event**, never a zero: silence means no rate was supplied, and zero would
    /// mean the turn was free.
    ///
    /// Rounded per turn, so the events below a run sum exactly to the total reported with it.
    Cost {
        model: String,
        micro_usd: u64,
    },
    Warning {
        code: String,
        message: String,
    },
    Finished {
        stop: LoopStop,
        /// How many model turns the run took, counting the first.
        ///
        /// Beside the stop rather than inside it, because only two `LoopStop` variants carry a
        /// turn count and both mean *a bound bound* — so a reader wanting "how long was this run"
        /// got an answer from a run that hit a ceiling and `null` from one that finished, which is
        /// backwards. An advisory bound on run length could not decide a single completed run.
        #[serde(default)]
        turns: u64,
    },
}

pub trait LoopSink {
    fn emit(&mut self, event: LoopEvent);
}

/// A sink that retains everything, for tests and for callers that only want the record.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VecLoopSink {
    events: Vec<LoopEvent>,
}

impl VecLoopSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> &[LoopEvent] {
        &self.events
    }

    pub fn into_events(self) -> Vec<LoopEvent> {
        self.events
    }

    /// Returns the streamed text as a reader saw it grow.
    pub fn text(&self) -> String {
        self.events
            .iter()
            .filter_map(|event| match event {
                LoopEvent::TextDelta { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    pub fn warnings(&self) -> impl Iterator<Item = (&str, &str)> {
        self.events.iter().filter_map(|event| match event {
            LoopEvent::Warning { code, message } => Some((code.as_str(), message.as_str())),
            _ => None,
        })
    }
}

impl LoopSink for VecLoopSink {
    fn emit(&mut self, event: LoopEvent) {
        self.events.push(event);
    }
}

/// Discards everything. Useful when the caller only wants the outcome.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NullLoopSink;

impl LoopSink for NullLoopSink {
    fn emit(&mut self, _: LoopEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vec_sink_reassembles_text_and_lists_warnings() {
        let mut sink = VecLoopSink::new();
        sink.emit(LoopEvent::TextDelta {
            text: "one ".to_owned(),
        });
        sink.emit(LoopEvent::Warning {
            code: "unknown-tool".to_owned(),
            message: "nope".to_owned(),
        });
        sink.emit(LoopEvent::TextDelta {
            text: "two".to_owned(),
        });
        assert_eq!(sink.text(), "one two");
        assert_eq!(
            sink.warnings().collect::<Vec<_>>(),
            vec![("unknown-tool", "nope")]
        );
    }

    #[test]
    fn events_round_trip() {
        let event = LoopEvent::ApprovalRequired {
            call_id: CallId::new("call-1").expect("valid"),
            name: ToolName::new("fs.write").expect("valid"),
        };
        let encoded = serde_json::to_value(&event).expect("serializes");
        assert_eq!(
            serde_json::from_value::<LoopEvent>(encoded).expect("deserializes"),
            event
        );
    }
}
