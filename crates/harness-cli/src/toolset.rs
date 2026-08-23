//! Every tool one run publishes: the read-only view, plus whatever the machine can confine.
//!
//! # One port, two sources, and the model cannot tell
//!
//! The read-only workspace tools are local and always there. The write and execute tools come from
//! `harness-substrate` and exist **only where a daemon says the machine can confine them**. A run
//! that reaches this with no substrate socket gets exactly the toolset this component has published
//! since it was written, which is why the absence of a daemon is not an error anywhere.
//!
//! The model is handed one list and never learns there were two sources. What decides its contents
//! is the machine, once, at startup — not a flag, not a policy file, and not the model's own
//! persistence.

use harness_wire::{Subject, ToolCall, ToolOutcome, ToolPort, ToolSpec};
use harness_substrate::ConfinedTools;

use crate::WorkspaceTools;

/// The published toolset of one run.
pub struct Toolset {
    reading: WorkspaceTools,
    confined: Option<ConfinedTools>,
    specs: Vec<ToolSpec>,
}

impl Toolset {
    /// The read-only toolset alone.
    pub fn read_only(reading: WorkspaceTools) -> Self {
        let specs = reading.specs().to_vec();
        Self {
            reading,
            confined: None,
            specs,
        }
    }

    /// The read-only toolset, plus what a confined workspace admits.
    pub fn with_confined(reading: WorkspaceTools, confined: ConfinedTools) -> Self {
        let mut specs = reading.specs().to_vec();
        specs.extend(confined.specs().iter().cloned());
        Self {
            reading,
            confined: Some(confined),
            specs,
        }
    }

    /// The directory the read-only tools see.
    pub fn root(&self) -> &std::path::Path {
        self.reading.root()
    }

    /// `true` when anything published here can change something that outlives the call.
    ///
    /// One question in one place, so a caller deciding whether a run needs a person watching it
    /// does not have to enumerate tools itself.
    pub fn mutates(&self) -> bool {
        self.specs.iter().any(|spec| spec.envelope.mutates())
    }
}

impl ToolPort for Toolset {
    fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }

    fn subjects(&self, call: &ToolCall) -> Vec<Subject> {
        let reading = self.reading.subjects(call);
        if !reading.is_empty() {
            return reading;
        }
        self.confined
            .as_ref()
            .map(|confined| confined.subjects(call))
            .unwrap_or_default()
    }

    fn call(&mut self, call: &ToolCall) -> ToolOutcome {
        if self
            .reading
            .specs()
            .iter()
            .any(|spec| spec.name == call.name)
        {
            return self.reading.call(call);
        }
        match &mut self.confined {
            Some(confined) => confined.call(call),
            // Unreachable through the loop, which refuses a call naming anything unpublished. Kept
            // as a refusal rather than a panic because *the loop guarantees it* is a claim about
            // another crate, and a port that trusted it would be trusting a comment.
            None => ToolOutcome::failed(format!("`{}` is not published here", call.name)),
        }
    }
}
