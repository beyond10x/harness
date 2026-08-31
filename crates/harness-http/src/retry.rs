//! When another attempt is worth making, and how long to wait first.
//!
//! # Where this came from
//!
//! the first wire, and byte-for-byte from the second, which copied it unchanged when
//! the second wire was built. Nothing in it names a vendor: it decides on
//! [`WireError::retriable`], a count and a clock, all three of which are neutral. That is why the
//! copy was possible, and why it is one thing now.

use std::time::{Duration, Instant};

use harness_wire::{Cancel, WireError};

/// How many attempts one turn gets, and how the pauses between them grow.
///
/// A value rather than a set of constants because it is the thing the two wires must be shown to
/// agree about: the cross-wire transport test compares what each one asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Attempts in total, the first included.
    pub max_attempts: u32,
    /// Doubled once per attempt already made, so the pause before the second attempt is twice it.
    pub backoff_base: Duration,
    /// How many doublings before the pause stops growing.
    ///
    /// Capped rather than unbounded because the loop's own deadline is what should end a run that
    /// is going nowhere; a back-off that outlived it would take the decision away from the caller.
    pub max_doublings: u32,
}

impl RetryPolicy {
    /// Four attempts, pausing 1 s, 2 s and 4 s.
    ///
    /// # Why a turn gets extra attempts at all
    ///
    /// A rate limit and a gateway that is still warming up are not answers; they are the absence
    /// of one, and the run has already paid for every turn before this. Losing all of it to a 503
    /// is the most expensive way to fail.
    pub const DEFAULT: Self = Self {
        max_attempts: 4,
        backoff_base: Duration::from_millis(500),
        max_doublings: 4,
    };

    /// How long to wait before the attempt after `attempt`, doubling and capped.
    pub(crate) fn backoff(self, attempt: u32) -> Duration {
        self.backoff_base
            .saturating_mul(2u32.saturating_pow(attempt.min(self.max_doublings)))
    }

    /// The error a turn ends with once this transport has done all the retrying it will do.
    ///
    /// # Why an exhausted error stops being retriable
    ///
    /// The loop above retries a turn whose error says `retriable`, because it owns the transcript
    /// and knows a failed turn changed nothing. It cannot see how many attempts were already made
    /// down here. Left as it was, a gateway that is down would be tried [`Self::max_attempts`]
    /// times here, handed up as retriable, and tried three more rounds of [`Self::max_attempts`]
    /// by the loop — sixteen requests and half a minute to learn one thing. So an error this
    /// transport gave up on goes up as **final**, with the attempt count in its words.
    ///
    /// # Why the attempt count decides on its own, and `emitted` does not soften it
    ///
    /// This read `!emitted && attempts >= max_attempts`, on the argument that a stream which broke
    /// after its first byte has been retried nowhere yet and the loop should be its first chance.
    /// The argument holds for a *first* attempt that broke mid-stream — and that case never
    /// reaches this branch, because `attempts` is below the maximum and nothing is clamped. What
    /// it also admitted was the case it was not written for: a turn that failed three times before
    /// its first byte and then broke mid-stream on the fourth. That is a gateway that is not going
    /// to answer, and it went up retriable — buying another four attempts from the loop, three
    /// more times over. Sixteen requests is the worse failure, so the count decides regardless of
    /// what was emitted.
    ///
    /// Whether anything was emitted still matters, and it decides earlier: no attempt that emitted
    /// anything is ever retried, because a person has already read part of that answer. What it no
    /// longer does is reach this far. See [`crate::WitnessedSink`].
    pub(crate) fn exhausted(self, mut error: WireError, attempts: u32) -> WireError {
        if error.retriable && attempts >= self.max_attempts {
            error.retriable = false;
            error.message = format!(
                "{} (after {attempts} attempts, none of which produced a whole turn)",
                error.message
            );
        }
        error
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Sleeps for `duration` unless the caller cancels first.
///
/// In slices rather than one `sleep`: a person who pressed Ctrl-C during a back-off would
/// otherwise wait out the whole pause before the next attempt noticed, which is the harness
/// ignoring them for up to eight seconds.
pub(crate) fn pause(duration: Duration, cancel: &Cancel) {
    let end = Instant::now() + duration;
    let slice = Duration::from_millis(50);
    loop {
        if cancel.is_cancelled() {
            return;
        }
        let now = Instant::now();
        if now >= end {
            return;
        }
        std::thread::sleep(slice.min(end - now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_this_transport_gave_up_on_after_four_attempts_goes_up_as_final() {
        let policy = RetryPolicy::DEFAULT;
        let cold = WireError::transport("503");
        assert!(
            cold.retriable,
            "the precondition: a cold gateway is retriable here"
        );
        // Exhausted here: final, and the words say how hard it was tried. The attempt count
        // decides on its own — a turn that broke mid-stream on its fourth attempt is the same
        // gateway as one that never answered, and letting the loop buy four more attempts three
        // times over is sixteen requests to learn one thing.
        let final_error = policy.exhausted(cold.clone(), policy.max_attempts);
        assert!(!final_error.retriable);
        assert!(
            final_error.message.contains("after 4 attempts"),
            "{}",
            final_error.message
        );
        // Under the count, nothing is clamped: the first attempt of a turn whose stream broke
        // after its first byte has been retried nowhere yet, and the loop is its first chance.
        assert!(policy.exhausted(cold, 1).retriable);
        // A final error stays final and its words stay its own.
        let refused = WireError::protocol("malformed");
        assert_eq!(
            policy.exhausted(refused.clone(), policy.max_attempts),
            refused
        );
    }

    #[test]
    fn the_pauses_double_and_then_stop_growing() {
        let policy = RetryPolicy::DEFAULT;
        // What a run actually waits, in order: the first attempt has already failed when the first
        // of these is asked for.
        assert_eq!(policy.backoff(1), Duration::from_secs(1));
        assert_eq!(policy.backoff(2), Duration::from_secs(2));
        assert_eq!(policy.backoff(3), Duration::from_secs(4));
        // The cap holds however far a caller with a longer budget counts.
        assert_eq!(policy.backoff(4), Duration::from_secs(8));
        assert_eq!(policy.backoff(99), Duration::from_secs(8));
    }

    #[test]
    fn a_cancelled_pause_returns_at_once_rather_than_waiting_it_out() {
        let cancel = Cancel::new();
        cancel.cancel();
        let started = Instant::now();
        pause(Duration::from_secs(30), &cancel);
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "a person who pressed Ctrl-C waited {:?}",
            started.elapsed()
        );
    }
}
