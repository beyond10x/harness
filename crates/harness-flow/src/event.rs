//! What a walk reports while it runs.
//!
//! The stream **nests**, because the document does. A reader following a run sees `shape` entered,
//! its steps, `shape` left — not nineteen step ids they have to reassemble into sections. That is
//! the reporting half of what sub-trees buy; the scheduling half is in [`crate::plan`].

use serde::{Deserialize, Serialize};

use crate::NodeId;

/// Which side of a section boundary a run was standing on when it was told no.
///
/// Two words rather than a boolean, because a reader of a record has to tell *it was not allowed to
/// start* from *it was not allowed to finish* at a glance, and `refused: true` says neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Moment {
    /// Before the section ran anything.
    Enter,
    /// After the section said what it hands over.
    Leave,
}

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
        /// Which attempt this is, from 1. Always present, so *first time* and *third time* are
        /// read the same way rather than one being inferred from the absence of a field.
        attempt: u32,
        /// How many attempts the document allows.
        of: u32,
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
    /// A sub-tree did not come out clean and is being re-entered.
    ///
    /// The retreat, as an event. A reader who sees `implement` twice in a stream must be able to
    /// tell a retreat from a duplicate, and the only place that can be said is here.
    GroupRepeating {
        path: String,
        /// The attempt that just failed.
        attempt: u32,
        of: u32,
    },
    /// A group promised something in `gives` and did not hand it over.
    ///
    /// The group fails. `gives` is a contract the document wrote down, and letting siblings run on
    /// after a broken one hands them a hole they cannot see.
    HandoffIncomplete { path: String, missing: Vec<NodeId> },
    /// A caller refused a section boundary.
    ///
    /// Emitted **before** the consequence, at either moment, so a record reads *why* ahead of
    /// *what happened next*: an enter refusal is followed by the section's steps as
    /// [`FlowEvent::NodeSkipped`] and a failed [`FlowEvent::GroupLeft`]; a leave refusal by a
    /// [`FlowEvent::GroupRepeating`] when the document still allows an attempt, and by a failed
    /// `GroupLeft` when it does not.
    ///
    /// `reason` is the caller's own words, carried and never read — this crate evaluates no gate.
    TransitionRefused {
        path: String,
        moment: Moment,
        /// The attempt the refusal was asked about, from 1.
        attempt: u32,
        reason: String,
    },
    /// A sub-tree was left.
    GroupLeft {
        path: String,
        failed: bool,
        /// What it handed its siblings, by name. The transcript stays inside.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        gave: Vec<NodeId>,
        /// How many attempts it took, or used up.
        attempts: u32,
        /// `true` when it failed *and* had no attempts left.
        ///
        /// Distinct from `failed` on purpose: *it broke* and *it kept breaking until the document
        /// stopped letting it try* are different facts, and a bounded repeat that silently reported
        /// the first would hide the bound doing its job.
        exhausted: bool,
    },
    /// The walk ended.
    ///
    /// `clean` is the verdict and the three counts are tallies over every attempt. A flow that
    /// retreated once and then succeeded reports a failure *and* `clean: true`, because both are
    /// true and folding them together would call every retreat a failed run.
    FlowFinished {
        flow: String,
        ran: usize,
        failed: usize,
        skipped: usize,
        retreats: usize,
        clean: bool,
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
