use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::WireError;
use crate::envelope::Subject;
use crate::id::{CallId, ToolName, WireId};
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
    /// A fragment of the model's own reasoning, as the provider chose to show it.
    ///
    /// What a provider streams here is what it is willing to have read — a summary on one wire,
    /// the visible thinking on another; this crate does not know which and does not need to. It
    /// exists so a person watching a long think sees that something is happening — a turn that
    /// is silent for a minute is one they will interrupt. It is never replayed into the
    /// conversation; the opaque item the turn ends with is what carries the reasoning across a
    /// tool round trip.
    ReasoningDelta {
        text: String,
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

    /// A second handle on the same model API, for a caller running two loops at once.
    ///
    /// # Why a port has to say this rather than the caller assuming it
    ///
    /// [`ModelPort::turn`] takes `&mut self`, so one port is one turn at a time and a caller that
    /// wants two must have two. Whether that is possible is the port's own fact and nothing above
    /// it can tell: a client over HTTP forks into a second client sharing the connection pool and
    /// the credential source, and a port that is one end of a single duplex connection to somebody
    /// else's process cannot fork at all — a second turn down it would interleave with the first
    /// on the wire.
    ///
    /// [`None`] — the default, so a port written before this existed keeps meaning what it did —
    /// is *this cannot be run beside itself*. It is not a failure and nothing is refused for it:
    /// the caller runs its work in order instead, which is what every caller did before forking
    /// existed.
    ///
    /// A fork is the same endpoint, the same credential source and the same wire id. It is **not**
    /// a second run's worth of anything the first one bounds: budgets, approvals and cancellation
    /// live above this trait, and a caller that forks a port still owes every one of them.
    fn fork(&self) -> Option<Box<dyn ModelPort + Send + '_>> {
        None
    }
}

/// Where the loop's tools come from.
///
/// The one seam that makes the embedded and the bridged harness the same loop: in-process this is
/// backed by direct calls, and under a bridge by a callback over the wire. The loop cannot tell.
pub trait ToolPort {
    /// The complete set of tools published for the next turn.
    fn specs(&self) -> &[ToolSpec];

    /// The concrete things **this call** would touch.
    ///
    /// # A spec is a claim; this is the fact
    ///
    /// [`ToolSpec::envelope`] says what a tool *can* do and is fixed for the life of the tool.
    /// This says what one invocation *does*, and it is what a gate stops things on: a tool that
    /// honestly declares [`crate::Effect::Write`] and is handed a path outside the workspace is
    /// refused on the subject, because the declaration was right and the call was not.
    ///
    /// Answering with an empty list means *this call touches nothing a policy could name*, which
    /// is true of a tool gated by its bare name alone. It is not a way to avoid being gated: a
    /// port that hid its subjects would be declaring itself unreviewable, and the tools that can
    /// afford to say nothing are the ones that were never dangerous.
    ///
    /// Defaulted so the trait stays implementable in one line for a read-only toolset, which is
    /// the shape most tests and every existing port have.
    fn subjects(&self, _call: &ToolCall) -> Vec<Subject> {
        Vec::new()
    }

    /// The neutral operation this concrete call resolves to, when the port has one.
    ///
    /// Dynamic catalogues cannot be reconstructed after a run from a static name table. Recording
    /// this answer at call time preserves their meaning without teaching the wire their names.
    fn operation(&self, _call: &ToolCall) -> Option<String> {
        None
    }

    /// What **this call** invokes, for the gate that decides whether a person is asked.
    ///
    /// # Why this is per call and not per spec
    ///
    /// A port that publishes verbs over a catalogue — `tool_invoke` over `file_read`, `run`, … —
    /// has one spec whose envelope must honestly declare every effect any entry can have. A gate
    /// that read *that* would ask a person about every read. What decides is the **entry's** spec,
    /// unwrapped from the call the same way [`ToolPort::subjects`] unwraps its subjects — and it
    /// is the whole spec, not only the envelope, because the person asked, the event that says
    /// they were asked and the refusal the model reads all name it. A gate that decided on the
    /// entry's envelope and then reported the verb told an approver `tool_invoke` was refused and
    /// never said `file_write`.
    ///
    /// Defaulted to the published spec by name, which is right for a flat port. `None` when the
    /// call names nothing this port published; the loop refuses such a call before it asks anyone.
    fn invoked(&self, call: &ToolCall) -> Option<ToolSpec> {
        self.specs()
            .iter()
            .find(|spec| spec.name == call.name)
            .cloned()
    }

    /// Every name a call over this port can **reach**, whatever the port publishes them as.
    ///
    /// # The vocabulary a narrowing is written in
    ///
    /// A named agent's `tools:` list names the things a run may *do*, and behind a verb surface
    /// those are not the things it publishes: [`ToolPort::specs`] answers `tool_search`,
    /// `tool_describe`, `tool_invoke` on every run and `file_write` is an argument. A gate reading
    /// `specs()` therefore narrows the **route** instead of the reach — it takes the verb away from
    /// an agent granted a read, and lets every entry through to one granted the verb.
    ///
    /// This is the same vocabulary [`ToolPort::invoked`] answers in, and the two are one rule seen
    /// from two sides: this says which names a narrowing may name, `invoked` says which of them one
    /// call reached. **Neither decides anything.** Whether a name is admitted stays with the loop,
    /// which is what keeps this from being a second gate free to disagree with the first.
    ///
    /// Defaulted to the published names, which is exactly right for a flat port — what it publishes
    /// is what it performs — and is what makes this method invisible to every port that has no
    /// indirection.
    ///
    /// # A wrong answer costs the run reach and never boundary
    ///
    /// A narrowing is only ever an **intersection** with this list, so a name left out of it is a
    /// name no grant can hold: a port that under-reports produces a smaller grant, and a smaller
    /// grant refuses more. A port that over-reports names things it cannot reach, and a call for
    /// one of them is refused by the port itself. Neither direction can widen a run.
    ///
    /// That is a property of how the caller uses this list and not a courtesy: it holds because
    /// nothing is exempted from the narrowing on the strength of being *absent* here. An earlier
    /// caller did exactly that — it treated a published name missing from this list as indirection
    /// to be let through — and the result was that a port saying less about itself got a run that
    /// could do more, which is the failure this paragraph now rules out rather than describes.
    fn reachable(&self) -> Vec<ToolName> {
        self.specs().iter().map(|spec| spec.name.clone()).collect()
    }

    /// The specs of everything a call over this port can reach, however it is published.
    ///
    /// [`ToolPort::reachable`] is enough to intersect a named grant. Scheduling needs the rest of
    /// each spec: two delegates are observationally safe to run together only if no reachable
    /// entry mutates or asks for approval. A verb surface therefore returns its catalogue entries,
    /// not the route specs which hide them.
    ///
    /// Defaulted to the published specs for a flat surface. A port whose [`ToolPort::invoked`]
    /// returns different entry specs must override this with the complete corresponding set.
    fn reachable_specs(&self) -> Vec<ToolSpec> {
        self.specs().to_vec()
    }

    /// Every neutral operation this port can perform, whatever it publishes them as.
    ///
    /// # The question `specs()` stopped being able to answer
    ///
    /// *Could this run write a file?* used to be readable off the tool list: a port that published
    /// `workspace_write` had a writer and one that did not, did not. Behind three verbs it is not:
    /// the list is `tool_search`, `tool_describe`, `tool_invoke` on **every** run, and what stands
    /// behind them is the catalogue — six entries on a machine that can confine a process, three on
    /// one that cannot.
    ///
    /// A reader of a finished run needs the answer, and it is not a detail of the surface. It is
    /// the whole of what an attribution control asks: *an absence of writes is a refusal only if
    /// there was a writer to refuse*. Without it that control reports a run with no writer at all,
    /// which reads as a defect and is a vocabulary mismatch.
    ///
    /// Empty by default — a flat port answers with its tool names and there is nothing behind them.
    fn operations(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Runs exactly one call the loop already checked against [`ToolPort::specs`].
    ///
    /// A failure is an outcome with `failed` set, not an error: the model has to see that the tool
    /// ran and did not work, or it will assume the effect landed.
    fn call(&mut self, call: &ToolCall) -> ToolOutcome;

    /// [`call`](Self::call), told how much of the run's wall-clock budget is left.
    ///
    /// The loop checks its deadline between calls, and a call already running is beyond its
    /// reach: a `run` of a test suite holds the turn open for its own timeout — ten or fifteen
    /// minutes — whatever a one-minute budget said. So the loop says how long is left and a port
    /// that starts something bounds it by that. `None` is a run with no deadline.
    ///
    /// Defaulted to an unbounded [`call`](Self::call), which is right for a port whose calls
    /// return promptly and for one that has nothing to bound. No clock is read here: the figure is
    /// the loop's, computed with the loop's clock and handed over.
    fn call_within(&mut self, call: &ToolCall, remaining: Option<Duration>) -> ToolOutcome {
        let _ = remaining;
        self.call(call)
    }

    /// Runs several calls the loop already checked, answering one outcome per call, in order.
    ///
    /// # Why a batch exists
    ///
    /// A turn that asks for six reads pays six round trips of tool latency when they run one after
    /// another, and nothing about a read requires that. The loop hands over **only calls whose
    /// invoked envelope does not mutate** — pure reads, already published, already inside every
    /// bound, and never ones that ask a person — so a port that can run them side by side may. A
    /// port that cannot runs them in order, which is what this default does, and the loop cannot
    /// tell the difference except by the clock.
    ///
    /// The outcomes are positional: `outcomes[i]` answers `calls[i]`. A port that answers fewer or
    /// more is a port the loop refuses to trust, and it falls back to calling each one itself —
    /// **after** this has already run whatever it ran, so a call in a miscounted group happens
    /// twice. That is survivable only because a group is pure reads.
    ///
    /// `remaining` is one figure for the whole group, not a share of one. The calls are meant to
    /// run side by side, so they all start at the same moment and the time left at that moment is
    /// what each of them has. The loop reads its deadline again the moment this returns; it does
    /// not read it, or the cancellation token, between the calls of one group, because there is no
    /// point between them at which it is in control.
    fn call_batch(&mut self, calls: &[ToolCall], remaining: Option<Duration>) -> Vec<ToolOutcome> {
        calls
            .iter()
            .map(|call| self.call_within(call, remaining))
            .collect()
    }

    /// A second handle on the same toolset, for a caller running two loops at once.
    ///
    /// # Not the same question as [`ToolPort::call_batch`]
    ///
    /// A batch is *these calls, now, decided by the loop that is holding this port*. A fork is a
    /// whole second agent loop that will decide its own calls, over the same catalogue, for as
    /// long as it runs. Both are the port saying it can do two things at once; only the fork
    /// hands the second one out.
    ///
    /// What a fork must be: the **same admitted set**, entry for entry. A fork that published one
    /// more tool than the port it came from would be a way to widen a run by delegating, which is
    /// the one thing delegation must never do. Narrowing is the caller's job and happens above
    /// this trait.
    ///
    /// [`None`] — the default — is *this cannot be run beside itself*, which is the honest answer
    /// for a port that is a callback over somebody else's connection. The caller then runs its
    /// work in order, and nothing is refused for it.
    fn fork(&self) -> Option<Box<dyn ToolPort + Send + '_>> {
        None
    }
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
