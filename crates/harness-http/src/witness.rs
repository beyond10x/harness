//! The sink that decides what may be retried.

use harness_wire::{StreamEvent, StreamSink};

/// A sink that remembers whether anything reached the caller.
///
/// **The whole of the retry rule.** Resending a request is safe on the routes this repository
/// speaks: turns are stateless (`AGENTS.md` invariant 4), nothing is retained on the far side, and
/// a second identical `POST` is a fresh turn. What is *not* safe is resending after the caller has
/// already seen part of the first attempt: the text deltas are out, a person has read them, and a
/// second attempt would append a second copy of the same sentence to the record. So an attempt
/// that has emitted **anything** is final, whatever went wrong.
///
/// In practice that keeps exactly the failures worth retrying **here**: a refused connection, a
/// rate limit, a gateway still starting a backend — all of which land before the first byte of the
/// stream.
///
/// # The loop retries what this sink will not
///
/// A turn whose stream broke after the first delta is still worth another attempt: losing a
/// twenty-turn run to one dropped connection is the most expensive way a run can fail. That
/// attempt cannot be made *here*, because the caller has already been handed half a sentence and
/// appending a second copy of it is not a retry but a corrupted transcript. So the two decisions
/// are taken in two places: the transport never resends after emitting and reports
/// [`harness_wire::WireError::retriable`] honestly, and the loop above it — which owns the
/// transcript and can therefore throw the partial turn away — decides whether to ask for the turn
/// again. Widening `retriable` is consequently a change the loop acts on, not a change to what
/// this sink does.
///
/// # Where this came from
///
/// `harness-responses`, copied unchanged into `harness-messages`. Nothing in it is vendor-shaped:
/// it counts emissions, which the neutral [`StreamSink`] already defines.
pub struct WitnessedSink<'a> {
    inner: &'a mut dyn StreamSink,
    emitted: bool,
}

impl<'a> WitnessedSink<'a> {
    pub fn new(inner: &'a mut dyn StreamSink) -> Self {
        Self {
            inner,
            emitted: false,
        }
    }

    /// Whether anything reached the caller through this sink.
    pub fn emitted(&self) -> bool {
        self.emitted
    }
}

impl StreamSink for WitnessedSink<'_> {
    fn emit(&mut self, event: StreamEvent) {
        self.emitted = true;
        self.inner.emit(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use harness_wire::VecSink;

    #[test]
    fn a_sink_nothing_passed_through_reports_nothing_emitted() {
        let mut inner = VecSink::new();
        let witnessed = WitnessedSink::new(&mut inner);
        assert!(!witnessed.emitted());
    }

    #[test]
    fn one_event_of_any_kind_makes_the_attempt_final() {
        // Any kind: a warning is enough. The rule is about what a reader has seen, not about
        // whether the turn produced an answer.
        let mut inner = VecSink::new();
        let mut witnessed = WitnessedSink::new(&mut inner);
        witnessed.emit(StreamEvent::Warning {
            code: "any".to_owned(),
            message: "any".to_owned(),
        });
        assert!(witnessed.emitted());
        assert_eq!(inner.events().len(), 1, "and it reached the caller");
    }
}
