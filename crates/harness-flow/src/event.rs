//! What a walk reports while it runs.
//!
//! The stream **nests**, because the document does. A reader following a run sees `shape` entered,
//! its steps, `shape` left — not nineteen step ids they have to reassemble into sections. That is
//! the reporting half of what sub-trees buy; the scheduling half is in [`crate::plan`].

use serde::{Deserialize, Serialize};

use crate::NodeId;

/// One thing that happened during a walk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FlowEvent {
    /// A walk began.
    FlowStarted {
        flow: String,
        /// How many steps the document holds, counted before anything ran.
        steps: usize,
    },
    /// A sub-tree was entered.
    GroupEntered {
        path: String,
        /// Its layers, so a reader knows how deep this section is before it starts.
        layers: usize,
    },
    /// A layer of siblings became runnable together.
    ///
    /// Emitted even when the layer holds one node: *these could have run in parallel* is a fact
    /// about the document, and a stream that only mentioned it sometimes would make a reader infer
    /// concurrency from silence.
    LayerReady { path: String, nodes: Vec<NodeId> },
    /// A step began.
    StepStarted { path: String },
    /// A step ended.
    StepFinished { path: String, failed: bool },
    /// A node did not run, because something it needs failed.
    ///
    /// Named rather than silent: a step that never ran and a step that ran and passed are the two
    /// things a reader of a green run must be able to tell apart.
    NodeSkipped { path: String, because: String },
    /// A sub-tree was left.
    GroupLeft { path: String, failed: bool },
    /// The walk ended.
    FlowFinished {
        flow: String,
        ran: usize,
        failed: usize,
        skipped: usize,
    },
}

/// Where a walk reports.
pub trait FlowSink {
    fn emit(&mut self, event: FlowEvent);
}

/// A sink that keeps everything, for tests and for a caller that only wants the record.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VecFlowSink {
    events: Vec<FlowEvent>,
}

impl VecFlowSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> &[FlowEvent] {
        &self.events
    }

    pub fn into_events(self) -> Vec<FlowEvent> {
        self.events
    }

    /// The paths of every step that started, in order.
    pub fn steps_started(&self) -> Vec<&str> {
        self.events
            .iter()
            .filter_map(|event| match event {
                FlowEvent::StepStarted { path } => Some(path.as_str()),
                _ => None,
            })
            .collect()
    }
}

impl FlowSink for VecFlowSink {
    fn emit(&mut self, event: FlowEvent) {
        self.events.push(event);
    }
}
